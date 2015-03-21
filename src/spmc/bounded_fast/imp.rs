use std::{ptr, mem, cmp};
use std::num::{Int};
use std::sync::atomic::{AtomicUsize, AtomicBool};
use std::sync::atomic::Ordering::{SeqCst};
use std::sync::{Mutex, Condvar};
use std::rt::heap::{allocate, deallocate};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use alloc::{oom};
use {Error, Sendable};

const CACHE_LINE_SIZE: usize = 64;

struct CacheLinePad([u8; CACHE_LINE_SIZE]);

impl CacheLinePad {
    fn new() -> CacheLinePad {
        unsafe { mem::uninitialized() }
    }
}

struct Node<T: Sendable> {
    val: T,
    pos: AtomicUsize,
}

#[repr(C)]
pub struct Packet<'a, T: Sendable+'a> {
    // The id of this channel. The address of the `arc::Inner` that contains this channel.
    id: Cell<usize>,

    // The buffer we store the massages in.
    buf: *mut Node<T>,
    // One less than the capacity of the channel. Note that the capacity is a power of
    // two.
    cap_mask: usize,

    next_write: Cell<usize>,
    next_read: AtomicUsize,

    // Is the sender sleeping?
    have_sleeping_sender: AtomicBool,
    // Condvar the sender is sleeping on.
    send_condvar:         Condvar,

    // Number of receivers that are currently sleeping.
    sleeping_receivers: AtomicUsize,
    // Condvar the senders are sleeping on.
    recv_condvar:       Condvar,

    sender_disconnected: AtomicBool,
    num_receivers: AtomicUsize,

    // Mutex that protects the two atomic variables above.
    sleep_mutex: Mutex<()>,

    // Is any one selecting on this channel?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue<'a>>,
}

impl<'a, T: Sendable+'a> Packet<'a, T> {
    pub fn new(mut buf_size: usize) -> Packet<'a, T> {
        buf_size = cmp::max(buf_size, 2);
        let cap = buf_size.checked_next_power_of_two().unwrap_or(!0);
        let size = cap.checked_mul(mem::size_of::<Node<T>>()).unwrap_or(!0);
        let buf = unsafe { allocate(size, mem::min_align_of::<T>()) };
        if buf.is_null() {
            oom();
        }
        let packet = Packet {
            id: Cell::new(0),

            buf: buf as *mut Node<T>,
            cap_mask: cap - 1,

            next_write: Cell::new(0),
            next_read: AtomicUsize::new(0),

            have_sleeping_sender: AtomicBool::new(false),
            send_condvar:         Condvar::new(),

            sleeping_receivers: AtomicUsize::new(0),
            recv_condvar:       Condvar::new(),

            sender_disconnected: AtomicBool::new(false),
            num_receivers: AtomicUsize::new(1),

            sleep_mutex: Mutex::new(()),

            wait_queue_used: AtomicBool::new(false),
            wait_queue: Mutex::new(WaitQueue::new()),
        };
        for i in 0..cap {
            packet.get_node(i).pos.store(i, SeqCst);
        }
        packet
    }

    /// Call this function before any other.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue.lock().unwrap().set_id(id);
    }

    /// Call this function when the receiver is cloned.
    pub fn add_receiver(&self) {
        self.num_receivers.fetch_add(1, SeqCst);
    }

    /// Call this function when a receiver is dropped.
    pub fn remove_receiver(&self) {
        if self.num_receivers.fetch_sub(1, SeqCst) == 1 {
            let _guard = self.sleep_mutex.lock().unwrap();
            if self.have_sleeping_sender.load(SeqCst) {
                self.send_condvar.notify_one();
            }
        }
    }

    /// Call this function when the producer is dropped.
    pub fn remove_sender(&self) {
        self.sender_disconnected.store(true, SeqCst);
        let _guard = self.sleep_mutex.lock().unwrap();
        if self.sleeping_receivers.load(SeqCst) > 0 {
            self.recv_condvar.notify_all();
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

    fn get_node(&self, pos: usize) -> &mut Node<T> {
        unsafe { &mut *self.buf.offset((pos & self.cap_mask) as isize) }
    }

    /// Get a position to write to if the queue isn't full
    fn get_write_pos(&self) -> Option<usize> {
        let next_write = self.next_write.get();
        let node = self.get_node(next_write);
        let diff = node.pos.load(SeqCst) as isize - next_write as isize;
        if diff < 0 {
            None
        } else {
            assert!(diff == 0);
            self.next_write.set(next_write + 1);
            Some(next_write)
        }
    }

    pub fn send_async(&self, val: T, have_lock: bool) -> Result<(), (T, Error)> {
        if self.num_receivers.load(SeqCst) == 0 {
            return Err((val, Error::Disconnected))
        }

        let write_pos = if let Some(w) = self.get_write_pos() {
            w
        } else {
            return if self.num_receivers.load(SeqCst) == 0 {
                Err((val, Error::Disconnected))
            } else {
                Err((val, Error::Full))
            };
        };
        {
            let node = self.get_node(write_pos);
            unsafe { ptr::write(&mut node.val, val); }
            node.pos.store(write_pos + 1, SeqCst);
        }

        if self.sleeping_receivers.load(SeqCst) > 0 {
            if have_lock {
                self.recv_condvar.notify_one();
            } else {
                let _guard = self.sleep_mutex.lock().unwrap();
                self.recv_condvar.notify_one();
            }
        }

        self.notify_wait_queue();

        Ok(())
    }

    pub fn send_sync(&self, mut val: T) -> Result<(), (T, Error)> {
        val = match self.send_async(val, false) {
            Err((v, Error::Full)) => v,
            e @ Err(_) => return e,
            Ok(_) => return Ok(()),
        };

        let mut rv = Ok(());
        let mut guard = self.sleep_mutex.lock().unwrap();
        self.have_sleeping_sender.store(true, SeqCst);
        loop {
            val = match self.send_async(val, true) {
                Err((v, Error::Full)) => v,
                e @ Err(_) => { rv = e; break; },
                Ok(_) => break,
            };
            guard = self.send_condvar.wait(guard).unwrap();
        }
        self.have_sleeping_sender.store(false, SeqCst);

        rv
    }

    /// Get a position to read from if the queue isn't empty
    fn get_read_pos(&self) -> Option<usize> {
        let mut next_read = self.next_read.load(SeqCst);
        loop {
            let node = self.get_node(next_read);
            let diff = node.pos.load(SeqCst) as isize - 1 - next_read as isize;
            if diff < 0 {
                return None;
            } else if diff > 0 {
                next_read = self.next_read.load(SeqCst);
            } else {
                let next_read_old = next_read;
                next_read = self.next_read.compare_and_swap(next_read, next_read + 1,
                                                            SeqCst);
                if next_read_old == next_read {
                    return Some(next_read);
                }
            }
        }
    }

    pub fn recv_async(&self, have_lock: bool) -> Result<T, Error> {
        let read_pos = if let Some(r) = self.get_read_pos() {
            r
        } else {
            return if self.sender_disconnected.load(SeqCst) {
                Err(Error::Disconnected)
            } else {
                Err(Error::Empty)
            };
        };
        let val;
        {
            let node = self.get_node(read_pos);
            val = unsafe { ptr::read(&node.val) };
            node.pos.store(read_pos + self.cap_mask + 1, SeqCst);
        }

        if self.have_sleeping_sender.load(SeqCst) {
            if have_lock {
                self.send_condvar.notify_one();
            } else {
                let _guard = self.sleep_mutex.lock().unwrap();
                self.send_condvar.notify_one();
            }
        }

        Ok(val)
    }

    pub fn recv_sync(&self) -> Result<T, Error> {
        match self.recv_async(false) {
            Err(Error::Empty) => { },
            e @ Err(_) => return e,
            v @ Ok(_) => return v,
        }

        let mut rv;
        let mut guard = self.sleep_mutex.lock().unwrap();
        self.sleeping_receivers.fetch_add(1, SeqCst);
        loop {
            match self.recv_async(true) {
                Err(Error::Empty) => { },
                e @ Err(_) => { rv = e; break; },
                v @ Ok(_) => { rv = v; break; },
            }
            guard = self.recv_condvar.wait(guard).unwrap();
        }
        self.sleeping_receivers.fetch_sub(1, SeqCst);

        rv
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Packet<'a, T> { }
unsafe impl<'a, T: Sendable+'a> Sync for Packet<'a, T> { }

#[unsafe_destructor]
impl<'a, T: Sendable+'a> Drop for Packet<'a, T> {
    fn drop(&mut self) {
        while self.recv_async(false).is_ok() { }
        
        unsafe {
            deallocate(self.buf as *mut u8,
                       (self.cap_mask as usize + 1) * mem::size_of::<Node<T>>(),
                       mem::min_align_of::<Node<T>>());
        }
    }
}

unsafe impl<'a, T: Sendable+'a> _Selectable<'a> for Packet<'a, T> {
    fn ready(&self) -> bool {
        if self.sender_disconnected.load(SeqCst) {
            return true;
        }
        let next_read = self.next_read.load(SeqCst);
        let node = self.get_node(next_read);
        node.pos.load(SeqCst) as isize - 1 - next_read as isize >= 0
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
