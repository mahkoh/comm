use std::sync::atomic::{AtomicPtr, AtomicBool};
use std::sync::atomic::Ordering::{SeqCst};
use std::sync::{Mutex, Condvar};
use std::{mem, ptr};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use {Error};

pub struct Packet<T: Send+'static> {
    // The id of this channel. The address of the `arc::Inner` that contains this channel.
    id: Cell<usize>,

    // The address of the Node we'll write the next message to. Unfortunately this has to
    // be an atomic pointer because it's accessed from the threads that select on this
    // channel and written to by the thread that's receiving which don't have to be the
    // same threads.
    read_end: AtomicPtr<Node<T>>,
    // The address of the Node we'll read the next message to.
    write_end: Cell<*mut Node<T>>,

    // Has one of the endpoints disconnected?
    disconnected: AtomicBool,

    // Is the receiver sleeping?
    have_sleeping: AtomicBool,
    // Mutex to protect the boolean above. XXX: Maybe it doesn't have to be atomic?
    sleeping_mutex: Mutex<()>,
    // Condvar the receiver is waiting on.
    sleeping_condvar: Condvar,

    // Is someone selecting on this channel?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue>,
}

struct Node<T: Send+'static> {
    next: AtomicPtr<Node<T>>,
    val: Option<T>,
}

impl<T: Send+'static> Node<T> {
    // Creates and forgets a new empty Node.
    fn new() -> *mut Node<T> {
        let mut node = Box::new(Node {
            next: AtomicPtr::new(ptr::null_mut()),
            val: None
        });
        let ptr = &mut *node as *mut _;
        unsafe { mem::forget(node); }
        ptr
    }
}

impl<T: Send+'static> Packet<T> {
    pub fn new() -> Packet<T> {
        let ptr = Node::new();
        Packet {
            id: Cell::new(0),

            read_end:  AtomicPtr::new(ptr),
            write_end: Cell::new(ptr),

            disconnected: AtomicBool::new(false),

            have_sleeping: AtomicBool::new(false),
            sleeping_mutex: Mutex::new(()),
            sleeping_condvar: Condvar::new(),

            wait_queue_used: AtomicBool::new(false),
            wait_queue: Mutex::new(WaitQueue::new()),
        }
    }

    /// Call this function before any other.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue.lock().unwrap().set_id(id);
    }

    /// Call this when one of the endpoints disconnects.
    pub fn disconnect(&self) {
        if !self.disconnected.swap(true, SeqCst) {
            self.notify_sleeping();
        }
    }

    /// Wakes up the receiver if it's sleeping.
    fn notify_sleeping(&self) {
        if self.have_sleeping.load(SeqCst) {
            let _guard = self.sleeping_mutex.lock().unwrap();
            self.sleeping_condvar.notify_one();
        }
    }

    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        // Don't append another message if nobody can receive it.
        if self.disconnected.load(SeqCst) {
            return Err((val, Error::Disconnected));
        }
        
        let new_end = Node::new();

        // Some things to think about:
        //
        // - We synchronize new nodes with the receiver via the `next` field in the node.
        // When the reader sees that the field is not null, then it knows that the `val`
        // field contains a valid entry.
        //
        // - We are the ones who put the `write_end` node where it currently is. Therefore
        // our thread sees that the `val` field is None before we set it to anything.
        let write_end = unsafe { &mut *self.write_end.get() };
        write_end.val = Some(val);
        write_end.next.store(new_end, SeqCst);
        self.write_end.set(new_end);

        self.notify_sleeping();

        if self.wait_queue_used.load(SeqCst) {
            let mut wait_queue = self.wait_queue.lock().unwrap();
            if wait_queue.notify() == 0 {
                self.wait_queue_used.store(false, SeqCst);
            }
        }

        Ok(())
    }

    pub fn recv_async(&self) -> Result<T, Error> {
        let read_end = unsafe { &mut *self.read_end.load(SeqCst) };
        let next = read_end.next.load(SeqCst);
        if next.is_null() {
            return if self.disconnected.load(SeqCst) {
                Err(Error::Disconnected)
            } else {
                Err(Error::Empty)
            };
        }
        self.read_end.store(next, SeqCst);
        let mut node = unsafe { mem::transmute::<_, Box<Node<T>>>(read_end) };
        Ok(node.val.take().unwrap())
    }

    pub fn recv_sync(&self) -> Result<T, Error> {
        match self.recv_async() {
            v @ Ok(..) => return v,
            Err(Error::Empty) => { },
            e => return e,
        }

        let rv;
        let mut guard = self.sleeping_mutex.lock().unwrap();
        self.have_sleeping.store(true, SeqCst);
        loop {
            match self.recv_async() {
                v @ Ok(..) => { rv = v; break; }
                Err(Error::Empty) => { },
                e => { rv = e; break; }
            }
            guard = self.sleeping_condvar.wait(guard).unwrap();
        }
        self.have_sleeping.store(false, SeqCst);
        rv
    }
}

unsafe impl<T: Send+'static> Send for Packet<T> { }
unsafe impl<T: Send+'static> Sync for Packet<T> { }

#[unsafe_destructor]
impl<T: Send+'static> Drop for Packet<T> {
    fn drop(&mut self) {
        while self.recv_async().is_ok() { }
        unsafe { ptr::read(self.read_end.load(SeqCst)); }
    }
}

unsafe impl<T: Send+'static> _Selectable for Packet<T> {
    fn ready(&self) -> bool {
        if self.disconnected.load(SeqCst) {
            return true;
        }
        let read_end = unsafe { &mut *self.read_end.load(SeqCst) };
        !read_end.next.load(SeqCst).is_null()
    }

    fn register(&self, load: Payload) {
        let mut wait_queue = self.wait_queue.lock().unwrap();
        if wait_queue.add(load) > 0 {
            self.wait_queue_used.store(true, SeqCst);
        }
    }

    fn unregister(&self, id: usize) {
        let mut wait_queue = self.wait_queue.lock().unwrap();
        if wait_queue.remove(id) == 0 {
            self.wait_queue_used.store(false, SeqCst);
        }
    }
}
