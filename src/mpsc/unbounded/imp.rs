use std::sync::atomic::{AtomicPtr, AtomicUsize, AtomicBool};
use std::sync::atomic::Ordering::{SeqCst};
use std::sync::{Mutex, Condvar};
use std::{mem, ptr};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use {Error, Sendable};

pub struct Packet<T: Sendable> {
    // The id of this channel. The address of the `arc::Inner` containing this channel.
    id: Cell<usize>,

    // The next node we read from. This has to be an atomic variable for the same reasons
    // the field in the unbounded SPSC channel has to be atomic.
    read_end: AtomicPtr<Node<T>>,
    // The next node we write to.
    write_end: AtomicPtr<Node<T>>,

    // The number of senders.
    num_senders: AtomicUsize,
    // Do we still have a receiver?
    have_receiver: AtomicBool,

    // Are there any sleeping receivers?
    have_sleeping: AtomicBool,
    // Mutex protecting the boolean above.
    sleeping_mutex: Mutex<()>,
    // Condvar the receivers are waiting on.
    sleeping_condvar: Condvar,

    // Is anyone selecting on this channel?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue>,
}

struct Node<T: Sendable> {
    next: AtomicPtr<Node<T>>,
    val: Option<T>,
}

impl<T: Sendable> Node<T> {
    // Creates and forgets a new node.
    fn new() -> *mut Node<T> {
        let mut node: Box<Node<T>> = Box::new(Node {
            next: AtomicPtr::new(ptr::null_mut()),
            val: None
        });
        let ptr = &mut *node as *mut _;
        unsafe { mem::forget(node); }
        ptr
    }
}

impl<T: Sendable> Packet<T> {
    pub fn new() -> Packet<T> {
        let ptr = Node::new();
        Packet {
            id: Cell::new(0),

            read_end:  AtomicPtr::new(ptr),
            write_end: AtomicPtr::new(ptr),

            num_senders: AtomicUsize::new(1),
            have_receiver: AtomicBool::new(true),

            have_sleeping: AtomicBool::new(false),
            sleeping_mutex: Mutex::new(()),
            sleeping_condvar: Condvar::new(),

            wait_queue_used: AtomicBool::new(false),
            wait_queue: Mutex::new(WaitQueue::new()),
        }
    }

    /// Call this before any other function.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue.lock().unwrap().set_id(id);
    }

    /// Call this when you clone a sender.
    pub fn add_sender(&self) {
        self.num_senders.fetch_add(1, SeqCst);
    }

    /// Call this when you drop a sender.
    pub fn remove_sender(&self) {
        if self.num_senders.fetch_sub(1, SeqCst) == 1 {
            self.notify_sleeping();
            self.notify_wait_queue();
        }
    }

    fn notify_wait_queue(&self) {
        if self.wait_queue_used.load(SeqCst) {
            let mut wait_queue = self.wait_queue.lock().unwrap();
            if wait_queue.notify() == 0 {
                self.wait_queue_used.store(false, SeqCst);
            }
        }
    }

    /// Call this when you drop the receiver.
    pub fn remove_receiver(&self) {
        self.have_receiver.store(false, SeqCst);
    }

    /// Notify the sleeping receiver.
    fn notify_sleeping(&self) {
        if self.have_sleeping.load(SeqCst) {
            let _guard = self.sleeping_mutex.lock().unwrap();
            self.sleeping_condvar.notify_one();
        }
    }

    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        // If the receiver has been dropped we don't even try.
        if !self.have_receiver.load(SeqCst) {
            return Err((val, Error::Disconnected));
        }

        // Now this scales right up.
        let new_end = Node::new();
        let write_end = self.write_end.swap(new_end, SeqCst);
        unsafe {
            (*write_end).val = Some(val);
            (*write_end).next.store(new_end, SeqCst);
        }

        self.notify_sleeping();

        self.notify_wait_queue();

        Ok(())
    }

    pub fn recv_async(&self) -> Result<T, Error> {
        let read_end = unsafe { &mut *self.read_end.load(SeqCst) };
        let next = read_end.next.load(SeqCst);
        if next.is_null() {
            return if self.num_senders.load(SeqCst) == 0 {
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

unsafe impl<T: Sendable> Send for Packet<T> { }
unsafe impl<T: Sendable> Sync for Packet<T> { }

impl<T: Sendable> Drop for Packet<T> {
    fn drop(&mut self) {
        while self.recv_async().is_ok() { }
        unsafe { ptr::read(self.read_end.load(SeqCst)); }
    }
}

unsafe impl<T: Sendable> _Selectable for Packet<T> {
    fn ready(&self) -> bool {
        if self.num_senders.load(SeqCst) == 0 {
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
