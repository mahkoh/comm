//! Implementation of the bounded SPSC channel.

use std::{ptr, mem};
use std::num::{UnsignedInt, Int};
use std::sync::atomic::{AtomicUsize, AtomicBool};
use std::sync::atomic::Ordering::{SeqCst};
use std::sync::{Mutex, Condvar};
use std::rt::heap::{allocate, deallocate};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use alloc::{oom};
use {Error};

pub struct Packet<T: Send> {
    // Id of the channel. Address of the arc::Inner that contains us.
    id: Cell<usize>,

    // Buffer where we store the messages.
    buf: *mut T,
    // One less than the capacity. Note that the capacity is a power of two.
    cap_mask: usize,

    // The position in the buffer (modulo capacity) where we read the next message from
    read_pos:  AtomicUsize,
    // The position in the buffer (modulo capacity) where we write the next message to
    write_pos: AtomicUsize,

    // Is one of the endpoints sleeping?
    have_sleeping: AtomicBool,
    // Mutex to control `have_sleeping` access
    sleeping_mutex: Mutex<()>,
    // Convar the sleeping thread is waiting on
    sleeping_condvar: Condvar,

    // Has one of the endpoints been dropped?
    disconnected: AtomicBool,

    // Is someone selecting on this channel?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue>,
}

impl<T: Send> Packet<T> {
    pub fn new(buf_size: usize) -> Packet<T> {
        let cap = buf_size.checked_next_power_of_two().expect("capacity overflow");
        let size = cap.checked_mul(mem::size_of::<T>()).unwrap_or(!0);
        if size >= !0 >> 1 {
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

            have_sleeping: AtomicBool::new(false),
            sleeping_mutex: Mutex::new(()),
            sleeping_condvar: Condvar::new(),

            disconnected: AtomicBool::new(false),

            wait_queue_used: AtomicBool::new(false),
            wait_queue: Mutex::new(WaitQueue::new()),
        }
    }

    /// This has to be called before any other function.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue.lock().unwrap().set_id(id);
    }

    /// Wake a sleeping thread if it exists. have_lock is so that we don't deadlock when
    /// we call this function inside the sleep-loop.
    fn notify_sleeping(&self, have_lock: bool) {
        // See the docs in send_sync
        if self.have_sleeping.load(SeqCst) {
            if have_lock {
                self.sleeping_condvar.notify_one();
            } else {
                let _guard = self.sleeping_mutex.lock().unwrap();
                self.sleeping_condvar.notify_one();
            }
        }
    }

    fn get_pos(&self) -> (usize, usize) {
        (self.write_pos.load(SeqCst), self.read_pos.load(SeqCst))
    }

    /// Call this when one of the endpoints disconnects.
    pub fn disconnect(&self) {
        if !self.disconnected.swap(true, SeqCst) {
            // We're the first one to disconnect. Notify sleeping threads.
            self.notify_sleeping(false);
        }
    }

    pub fn send_async(&self, val: T, have_lock: bool) -> Result<(), (T, Error)> {
        // If the other end disconnected then don't even try to store anything new in the
        // channel.
        if self.disconnected.load(SeqCst) {
            return Err((val, Error::Disconnected));
        }

        let (write_pos, read_pos) = self.get_pos();
        if write_pos - read_pos == self.cap_mask + 1 {
            return Err((val, Error::Full));
        }

        unsafe {
            ptr::write(self.buf.offset((write_pos & self.cap_mask) as isize), val);
        }
        self.write_pos.store(write_pos + 1, SeqCst);

        self.notify_sleeping(have_lock);

        if self.wait_queue_used.load(SeqCst) {
            let mut wait_queue = self.wait_queue.lock().unwrap();
            if wait_queue.notify() == 0 {
                self.wait_queue_used.store(false, SeqCst);
            }
        }

        Ok(())
    }

    pub fn send_sync(&self, mut val: T) -> Result<(), (T, Error)> {
        val = match self.send_async(val, false) {
            Ok(()) => return Ok(()),
            e @ Err((_, Error::Disconnected)) => return e,
            Err((v, _)) => v,
        };

        let mut rv = Ok(());
        // We store have_sleeping after acquiring the lock so that another thread sees
        // this has to wait for us to go to sleep before it can acquire the lock and
        // notify the condvar.
        let mut guard = self.sleeping_mutex.lock().unwrap();
        self.have_sleeping.store(true, SeqCst);
        loop {
            val = match self.send_async(val, true) {
                Ok(()) => break,
                e @ Err((_, Error::Disconnected)) => { rv = e; break; },
                Err((v, _)) => v,
            };
            guard = self.sleeping_condvar.wait(guard).unwrap();
        }
        self.have_sleeping.store(false, SeqCst);
        rv
    }

    pub fn recv_async(&self, have_lock: bool) -> Result<T, Error> {
        let (write_pos, read_pos) = self.get_pos();
        if write_pos == read_pos {
            return if self.disconnected.load(SeqCst) {
                Err(Error::Disconnected)
            } else {
                Err(Error::Empty)
            };
        }

        let val = unsafe {
            ptr::read(self.buf.offset((read_pos & self.cap_mask) as isize))
        };
        self.read_pos.store(read_pos + 1, SeqCst);

        self.notify_sleeping(have_lock);

        Ok(val)
    }

    pub fn recv_sync(&self) -> Result<T, Error> {
        // See the docs in send_sync.

        match self.recv_async(false) {
            v @ Ok(..) => return v,
            Err(Error::Empty) => { },
            e => return e,
        }

        let rv;
        let mut guard = self.sleeping_mutex.lock().unwrap();
        self.have_sleeping.store(true, SeqCst);
        loop {
            match self.recv_async(true) {
                v @ Ok(..) => { rv = v; break; },
                Err(Error::Empty) => { },
                e => { rv = e; break; },
            }
            guard = self.sleeping_condvar.wait(guard).unwrap();
        }
        self.have_sleeping.store(false, SeqCst);
        rv
    }
}

unsafe impl<T: Send> Send for Packet<T> { }
unsafe impl<T: Send> Sync for Packet<T> { }

#[unsafe_destructor]
impl<T: Send> Drop for Packet<T> {
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

unsafe impl<T: Send> _Selectable for Packet<T> {
    fn ready(&self) -> bool {
        if self.disconnected.load(SeqCst) {
            return true;
        }
        let (write_pos, read_pos) = self.get_pos();
        write_pos != read_pos
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
