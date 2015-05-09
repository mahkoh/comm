use std::{ptr, mem};
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::{Mutex, Condvar};
use std::rt::heap::{allocate, deallocate};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use alloc::{oom};
use {Error, Sendable};

pub struct Packet<T: Sendable> {
    // The id of the channel. The address of the `arc::Inner` that contains the channel.
    id: Cell<usize>,

    // The buffer in which we store the messages.
    buf: *mut T,
    // One less than the capacity of the buffer. Note that the capacity is a power of two.
    cap_mask: usize,

    // The next position we read from (modulo the capacity).
    read_pos:  AtomicUsize,
    // The next position we write to (modulo the capacity).
    write_pos: AtomicUsize,

    // Is one of the endpoints sleeping?
    have_sleeping: AtomicBool,
    // Mutex to protect the boolean above.
    sleeping_mutex: Mutex<()>,
    // Convar the sleeping thread is waiting on.
    sleeping_condvar: Condvar,

    // Has the sender disconnected?
    sender_disconnected: AtomicBool,
    // Has the receiver disconnected?
    receiver_disconnected: AtomicBool,

    // Is anyone selecting on us?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue>,
}

impl<T: Sendable> Packet<T> {
    pub fn new(buf_size: usize) -> Packet<T> {
        let cap = buf_size.checked_next_power_of_two().expect("capacity overflow");
        let size = cap.checked_mul(mem::size_of::<T>()).unwrap_or(!0);
        if size > !0 >> 1 {
            panic!("capacity overflow");
        }
        let buf = if mem::size_of::<T>() == 0 {
            1 as *mut u8
        } else {
            unsafe { allocate(size, mem::min_align_of::<T>()) }
        };
        if buf.is_null() {
            oom();
        }
        Packet {
            id: Cell::new(0),

            buf: buf as *mut T,
            cap_mask: cap - 1,

            read_pos:  AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),

            have_sleeping:    AtomicBool::new(false),
            sleeping_mutex:   Mutex::new(()),
            sleeping_condvar: Condvar::new(),

            sender_disconnected: AtomicBool::new(false),
            receiver_disconnected: AtomicBool::new(false),

            wait_queue_used: AtomicBool::new(false),
            wait_queue: Mutex::new(WaitQueue::new()),
        }
    }

    /// This must be called before any other function.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue.lock().unwrap().set_id(id);
    }

    /// Notify the sleeping thread if it exists.
    fn notify_sleeping(&self) {
        if self.have_sleeping.load(Ordering::SeqCst) {
            let _guard = self.sleeping_mutex.lock().unwrap();
            self.sleeping_condvar.notify_one();
        }
    }

    fn get_pos(&self) -> (usize, usize) {
        (self.write_pos.load(Ordering::SeqCst), self.read_pos.load(Ordering::SeqCst))
    }

    /// Call this when the receiver disconnects.
    pub fn disconnect_receiver(&self) {
        self.receiver_disconnected.store(true, Ordering::SeqCst);
        if !self.sender_disconnected.load(Ordering::SeqCst) {
            self.notify_sleeping();
        }
    }

    /// Call this when the sender disconnects.
    pub fn disconnect_sender(&self) {
        self.sender_disconnected.store(true, Ordering::SeqCst);
        if !self.receiver_disconnected.load(Ordering::SeqCst) {
            self.notify_sleeping();
        }
        self.notify_wait_queue();
    }

    fn notify_wait_queue(&self) {
        if self.wait_queue_used.load(Ordering::SeqCst) {
            let mut wait_queue = self.wait_queue.lock().unwrap();
            if wait_queue.notify() == 0 {
                self.wait_queue_used.store(false, Ordering::SeqCst);
            }
        }
    }

    pub fn send(&self, val: T) -> Result<Option<T>, (T, Error)> {
        // Don't even try to store anything in the buffer if the receiver is dead.
        if self.receiver_disconnected.load(Ordering::SeqCst) {
            return Err((val, Error::Disconnected));
        }

        let (write_pos, read_pos) = self.get_pos();
        // Check if we have to overwrite anything.
        let old = if write_pos - read_pos != self.cap_mask + 1 {
            // Nope, lots of space.
            None
        } else if self.read_pos.compare_and_swap(read_pos, read_pos + 1,
                                                 Ordering::SeqCst) == read_pos {
            // Yo, there was no space and we're the ones who moved the read_pos. Now it's
            // our job to return the data to the sender.
            unsafe {
                Some(ptr::read(self.buf.offset((read_pos & self.cap_mask) as isize)))
            }
        } else {
            // Yo, but the reader was faster than we were and removed an element.
            None
        };

        unsafe {
            ptr::write(self.buf.offset((write_pos & self.cap_mask) as isize), val);
        }
        self.write_pos.store(write_pos + 1, Ordering::SeqCst);

        self.notify_sleeping();

        self.notify_wait_queue();

        Ok(old)
    }

    pub fn recv_async(&self) -> Result<T, Error> {
        let (write_pos, mut read_pos) = self.get_pos();
        if write_pos == read_pos {
            return if self.sender_disconnected.load(Ordering::SeqCst) {
                Err(Error::Disconnected)
            } else {
                Err(Error::Empty)
            };
        }

        // We might be fighting with a fast sender that overwrites our read_pos. Therefore
        // we need a CAS loop.
        loop {
            let new_read_pos = self.read_pos.compare_and_swap(read_pos, read_pos + 1,
                                                              Ordering::SeqCst);
            if new_read_pos == read_pos {
                break;
            } else {
                read_pos = new_read_pos;
            }
        }

        unsafe {
            Ok(ptr::read(self.buf.offset((read_pos & self.cap_mask) as isize)))
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
        self.have_sleeping.store(true, Ordering::SeqCst);
        loop {
            match self.recv_async() {
                v @ Ok(..) => { rv = v; break; },
                Err(Error::Empty) => { },
                e => { rv = e; break; },
            }
            guard = self.sleeping_condvar.wait(guard).unwrap();
        }
        self.have_sleeping.store(false, Ordering::SeqCst);
        rv
    }
}

unsafe impl<T: Sendable> Send for Packet<T> { }
unsafe impl<T: Sendable> Sync for Packet<T> { }

impl<T: Sendable> Drop for Packet<T> {
    fn drop(&mut self) {
        let (write_pos, read_pos) = self.get_pos();

        unsafe {
            for i in (0..write_pos-read_pos) {
                ptr::read(self.buf.offset(((read_pos + i) & self.cap_mask) as isize));
            }

            if mem::size_of::<T>() > 0 {
                deallocate(self.buf as *mut u8,
                           (self.cap_mask as usize + 1) * mem::size_of::<T>(),
                           mem::min_align_of::<T>());
            }
        }
    }
}

unsafe impl<T: Sendable> _Selectable for Packet<T> {
    fn ready(&self) -> bool {
        if self.sender_disconnected.load(Ordering::SeqCst) {
            return true;
        }
        let (write_pos, read_pos) = self.get_pos();
        write_pos != read_pos
    }

    fn register(&self, load: Payload) {
        let mut wait_queue = self.wait_queue.lock().unwrap();
        if wait_queue.add(load) > 0 {
            self.wait_queue_used.store(true, Ordering::SeqCst);
        }
    }

    fn unregister(&self, id: usize) {
        let mut wait_queue = self.wait_queue.lock().unwrap();
        if wait_queue.remove(id) == 0 {
            self.wait_queue_used.store(false, Ordering::SeqCst);
        }
    }
}
