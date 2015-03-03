use std::sync::atomic::{AtomicPtr, AtomicUsize, AtomicBool};
use std::sync::atomic::Ordering::{SeqCst};
use std::sync::{Mutex, Condvar};
use std::{mem, ptr};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use {Error, Sendable};

pub struct Packet<'a, T: Sendable+'a> {
    // The id of this channel. The address of the `arc::Inner` that contains this channel.
    id: Cell<usize>,

    // The next node we can read from.
    read_end: AtomicPtr<Node<T>>,
    // The next node we write to.
    write_end: Cell<*mut Node<T>>,

    // The number of nodes ready for reading.
    num_queued: AtomicUsize,

    // Number of receivers.
    num_receivers: AtomicUsize,
    // Do we still have a sender?
    have_sender: AtomicBool,

    // Number of sleeping receivers.
    num_sleeping: AtomicUsize,
    // Mutex that protects the variable above.
    sleeping_mutex: Mutex<()>,
    // The condvar the receivers are waiting on.
    sleeping_condvar: Condvar,

    // Is someone selecting on this channel?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue<'a>>,
}

struct Node<T: Sendable> {
    next: AtomicPtr<Node<T>>,
    val: Option<T>,
}

impl<T: Sendable> Node<T> {
    // Creates and forgets a new node.
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

impl<'a, T: Sendable+'a> Packet<'a, T> {
    pub fn new() -> Packet<'a, T> {
        let ptr = Node::new();
        Packet {
            id: Cell::new(0),

            read_end: AtomicPtr::new(ptr),
            write_end: Cell::new(ptr),

            num_queued: AtomicUsize::new(0),

            num_receivers: AtomicUsize::new(1),
            have_sender: AtomicBool::new(true),

            num_sleeping: AtomicUsize::new(0),
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

    /// Call this when a receiver gets cloned.
    pub fn add_receiver(&self) {
        self.num_receivers.fetch_add(1, SeqCst);
    }

    /// Call this when a receiver gets dropped.
    pub fn remove_receiver(&self) {
        self.num_receivers.fetch_sub(1, SeqCst);
    }

    /// Call this when the sender gets dropped.
    pub fn remove_sender(&self) {
        self.have_sender.store(false, SeqCst);
        if self.num_sleeping.load(SeqCst) > 0 {
            let _guard = self.sleeping_mutex.lock().unwrap();
            self.sleeping_condvar.notify_all();
        }
        self.notify_wait_queue();
    }

    fn notify_wait_queue(&self) {
        if self.wait_queue_used.load(SeqCst) {
            let mut wait_queue = self.wait_queue.lock().unwrap();
            if wait_queue.notify() == 0 {
                self.wait_queue_used.store(false, SeqCst);
            }
        }
    }

    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        // Don't even try to send anything if all receivers are dead.
        if self.num_receivers.load(SeqCst) == 0 {
            return Err((val, Error::Disconnected));
        }
        
        let new_end = Node::new();

        // See the comment in the unbounded SPSC implementation.
        let write_end = unsafe { &mut *self.write_end.get() };
        write_end.val = Some(val);
        write_end.next.store(new_end, SeqCst);
        self.num_queued.fetch_add(1, SeqCst); // Maybe we should move this line around a
                                              // bit?
        self.write_end.set(new_end);

        if self.num_sleeping.load(SeqCst) > 0 {
            let _guard = self.sleeping_mutex.lock().unwrap();
            self.sleeping_condvar.notify_one();
        }

        self.notify_wait_queue();

        Ok(())
    }

    pub fn recv_async(&self) -> Result<T, Error> {
        if self.num_queued.load(SeqCst) == 0 {
            return if !self.have_sender.load(SeqCst) {
                Err(Error::Disconnected)
            } else {
                Err(Error::Empty)
            };
        }
        
        // We have to look at the node in read_end, read next, and then store next in
        // read_end. Unfortunately this is the classic ABA problem. Furthermore, if we
        // just load the value of read_end, then another thread could already deallocate
        // and we access invalid memory when we try to read next.
        //
        // Therefore we use the following highly effective algorithm. I'm sure this will
        // scale right up /s.
        let mut read_end = ptr::null_mut();
        while read_end.is_null() {
            read_end = self.read_end.swap(read_end, SeqCst);
        }
        let next = unsafe { (*read_end).next.load(SeqCst) };
        if !next.is_null() {
            self.read_end.store(next, SeqCst);
            self.num_queued.fetch_sub(1, SeqCst);
            let mut node = unsafe { mem::transmute::<_, Box<Node<T>>>(read_end) };
            Ok(node.val.take().unwrap())
        } else {
            self.read_end.store(read_end, SeqCst);
            Err(Error::Empty)
        }
    }

    pub fn recv_sync(&self) -> Result<T, Error> {
        match self.recv_async() {
            v @ Ok(..) => return v,
            Err(Error::Empty) => { },
            e => return e,
        }

        let rv;
        let mut guard = self.sleeping_mutex.lock().unwrap();
        self.num_sleeping.fetch_add(1, SeqCst);
        loop {
            match self.recv_async() {
                v @ Ok(..) => { rv = v; break; }
                Err(Error::Empty) => { },
                e => { rv = e; break; }
            }
            guard = self.sleeping_condvar.wait(guard).unwrap();
        }
        self.num_sleeping.fetch_sub(1, SeqCst);
        rv
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Packet<'a, T> { }
unsafe impl<'a, T: Sendable+'a> Sync for Packet<'a, T> { }

#[unsafe_destructor]
impl<'a, T: Sendable+'a> Drop for Packet<'a, T> {
    fn drop(&mut self) {
        while self.recv_async().is_ok() { }
        unsafe { ptr::read(self.read_end.load(SeqCst)); }
    }
}

unsafe impl<'a, T: Sendable+'a> _Selectable<'a> for Packet<'a, T> {
    fn ready(&self) -> bool {
        !self.have_sender.load(SeqCst) || self.num_queued.load(SeqCst) > 0
    }

    fn register(&self, load: Payload<'a>) {
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
