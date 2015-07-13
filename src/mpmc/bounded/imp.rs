//! This code might still contain bugs. In either case it's very inefficient because we
//! want to avoid reading invalid memory at all costs. Note that the implementation from
//! 1024cores does not handle ABA!

use std::{ptr, mem};
use std::sync::atomic::{AtomicUsize, AtomicBool};
use std::sync::atomic::Ordering::{SeqCst};
use std::sync::{Mutex, Condvar};
use std::rt::heap::{allocate, deallocate};
use std::cell::{Cell};

use select::{_Selectable, WaitQueue, Payload};
use alloc::{oom};
use {Error, Sendable};

#[cfg(target_pointer_width = "64")]
type HalfPointer = u32;
#[cfg(target_pointer_width = "32")]
type HalfPointer = u16;

const HALF_POINTER_BITS: usize = ::std::usize::BITS as usize / 2;

fn decompose_pointer(val: usize) -> (HalfPointer, HalfPointer) {
    let lower = val as HalfPointer;
    let higher = (val >> HALF_POINTER_BITS) as HalfPointer;
    (lower, higher)
}

fn compose_pointer(lower: HalfPointer, higher: HalfPointer) -> usize {
    (lower as usize) | ((higher as usize) << HALF_POINTER_BITS)
}

pub struct Packet<'a, T: Sendable+'a> {
    // The id of this channel. The address of the `arc::Inner` that contains this channel.
    id: Cell<usize>,

    // The buffer we store the massages in.
    buf: *mut T,
    // One less than the capacity of the channel. Note that the capacity is a power of
    // two.
    cap_mask: HalfPointer,

    // read_start and next_write HalfPointer variables encoded in one usize. read_start is
    // the id before which all elements in the buffer have been read. next_write is the
    // next place that's free for writing.
    //
    // Note that this implies that, next_write - read_start <= capacity at all times.
    read_start_next_write: AtomicUsize,
    // write_end and next_read HalfPointer variables encoded in one usize. write_end is
    // the id before which all elements in the buffer have been written. next_read is the
    // next place that's free for reading.
    //
    // Note that this implies that, ignoring overflow, next_read <= write_end.
    //
    // See the docs below for why we have to store these four variables this way.
    write_end_next_read:   AtomicUsize,

    // Number of senders that are currently sleeping.
    sleeping_senders: AtomicUsize,
    // Condvar the senders are sleeping on.
    send_condvar:     Condvar,

    // Number of receivers that are currently sleeping.
    sleeping_receivers: AtomicUsize,
    // Condvar the senders are sleeping on.
    recv_condvar:       Condvar,

    // Mutex that protects the two atomic variables above and the one below.
    sleep_mutex: Mutex<()>,
    // Number of peers that are awake.
    peers_awake: AtomicUsize,

    // Is any one selecting on this channel?
    wait_queue_used: AtomicBool,
    wait_queue: Mutex<WaitQueue<'a>>,
}

impl<'a, T: Sendable+'a> Packet<'a, T> {
    pub fn new(buf_size: usize) -> Packet<'a, T> {
        if buf_size > 1 << (HALF_POINTER_BITS - 1) {
            panic!("capacity overflow");
        }
        let cap = buf_size.next_power_of_two();
        let size = cap.checked_mul(mem::size_of::<T>()).unwrap_or(!0);
        if size > !0 >> 1 {
            panic!("capacity overflow");
        }
        let buf = if mem::size_of::<T>() == 0 {
            1 as *mut u8
        } else {
            unsafe { allocate(size, mem::align_of::<T>()) }
        };
        if buf.is_null() {
            oom();
        }
        Packet {
            id: Cell::new(0),

            buf: buf as *mut T,
            cap_mask: (cap - 1) as HalfPointer,

            read_start_next_write: AtomicUsize::new(0),
            write_end_next_read:   AtomicUsize::new(0),

            sleeping_senders: AtomicUsize::new(0),
            send_condvar:     Condvar::new(),

            sleeping_receivers: AtomicUsize::new(0),
            recv_condvar:       Condvar::new(),

            sleep_mutex: Mutex::new(()),
            peers_awake: AtomicUsize::new(1),

            wait_queue_used: AtomicBool::new(false),
            wait_queue: Mutex::new(WaitQueue::new()),
        }
    }

    /// Call this function before any other.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue.lock().unwrap().set_id(id);
    }

    /// Call this function when the channel is cloned.
    pub fn add_peer(&self) {
        self.peers_awake.fetch_add(1, SeqCst);
    }

    /// Call this function when a peer is dropped.
    pub fn remove_peer(&self) {
        if self.peers_awake.fetch_sub(1, SeqCst) == 1 {
            let _guard = self.sleep_mutex.lock().unwrap();
            if self.sleeping_receivers.load(SeqCst) > 0 {
                self.recv_condvar.notify_one();
            } else {
                self.send_condvar.notify_one();
            }
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

    /// Get a position to write to if the queue isn't full
    fn get_write_pos(&self) -> Option<HalfPointer> {
        // See the get_read_pos docs for details.
        loop {
            let rsnw = self.read_start_next_write.load(SeqCst);
            let (read_start, next_write) = decompose_pointer(rsnw);
            if next_write - read_start == self.cap_mask + 1 {
                return None;
            }
            let rsnw_new = compose_pointer(read_start, next_write + 1);
            if self.read_start_next_write.compare_and_swap(rsnw, rsnw_new,
                                                           SeqCst) == rsnw {
                return Some(next_write);
            }
        }
    }

    /// `pos` is the position we've written to
    fn set_write_end(&self, pos: HalfPointer) {
        // See the get_read_pos docs for details.
        loop {
            let wenr = self.write_end_next_read.load(SeqCst);
            let (write_end, next_read) = decompose_pointer(wenr);
            if write_end != pos {
                continue;
            }
            let wenr_new = compose_pointer(pos + 1, next_read);
            if self.write_end_next_read.compare_and_swap(wenr, wenr_new,
                                                         SeqCst) == wenr {
                return;
            }
        }
    }

    fn set_mem(&self, pos: HalfPointer, val: T) {
        unsafe {
            ptr::write(self.buf.offset((pos & self.cap_mask) as isize), val);
        }
    }

    pub fn send_async(&self, val: T, have_lock: bool) -> Result<(), (T, Error)> {
        let write_pos = match self.get_write_pos() {
            Some(w) => w,
            _ => return Err((val, Error::Full)),
        };
        self.set_mem(write_pos, val);
        self.set_write_end(write_pos);

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
            Err(v) => v.0,
            _ => return Ok(()),
        };

        let mut rv = Ok(());
        let mut guard = self.sleep_mutex.lock().unwrap();
        self.sleeping_senders.fetch_add(1, SeqCst);
        loop {
            val = match self.send_async(val, true) {
                Err(v) => v.0,
                _ => break,
            };
            // It is possible that all peers sleep at the same time, however, it can be
            // shown that, as long as not all of them sleep sending and not all of them
            // sleeping receiving, one of them will wake up again because the condition
            // variable has already been notified.
            if self.peers_awake.fetch_sub(1, SeqCst) == 1 &&
                    self.sleeping_receivers.load(SeqCst) == 0 {
                self.peers_awake.fetch_add(1, SeqCst);
                rv = Err((val, Error::Deadlock));
                break;
            } else {
                guard = self.send_condvar.wait(guard).unwrap();
                self.peers_awake.fetch_add(1, SeqCst);
            }
        }
        self.sleeping_senders.fetch_sub(1, SeqCst);

        rv
    }

    /// Get a position to read from if the queue isn't empty
    fn get_read_pos(&self) -> Option<HalfPointer> {
        // The write_end_next_read field contains two variables: write_end and next_read.
        //
        // next_read is the next position we can read from, write_end is the first
        // position we can not read from because it has not necessarily been written yet.
        //
        // We have to store both of them in the same variable because of ABA. Consider the
        // following events:
        //
        // - This thread reads next_read == 0 and write_end == 1 and therefore there is no
        // early return in the `if` below.
        // - This thread gets suspended right after the `if`.
        // - Other threads continuous read from and write to the channel until both
        // write_end and next_read overflow.
        // - next_read == 0 and write_end == 0 holds now.
        // - This thread wakes up again.
        // - If we store next_read in its own variable, then the CAS can only test
        // next_read. Since next_read is 0, the CAS succeeds and we arrive at next_read ==
        // 1 and write_end == 0.
        // - The function that called this function reads from position 0 even though
        // nothing has been written to that position yet.
        //
        // Therefore we store next_read and write_end in the same variable. The overflow
        // above can still happen but if write_end gets smaller (or changes in any way),
        // the CAS will fail and we can never read uninitialized memory.
        //
        // It's highly unlikely for this ABA to happen, and on 64bit one might even
        // consider it impossible. After a more careful analysis, a future implementation
        // might change the implementation.
        loop {
            let wenr = self.write_end_next_read.load(SeqCst);
            let (write_end, next_read) = decompose_pointer(wenr);
            if write_end == next_read {
                return None;
            }
            let wenr_new = compose_pointer(write_end, next_read + 1);
            if self.write_end_next_read.compare_and_swap(wenr, wenr_new,
                                                         SeqCst) == wenr {
                return Some(next_read);
            }
        }
    }

    /// `pos` is the position we've read from
    fn set_read_start(&self, pos: HalfPointer) {
        loop {
            let rsnw = self.read_start_next_write.load(SeqCst);
            let (read_start, next_write) = decompose_pointer(rsnw);
            if read_start != pos {
                continue;
            }
            let rsnw_new = compose_pointer(pos + 1, next_write);
            if self.read_start_next_write.compare_and_swap(rsnw, rsnw_new,
                                                           SeqCst) == rsnw {
                return;
            }
        }
    }

    fn get_mem(&self, pos: HalfPointer) -> T {
        unsafe {
            ptr::read(self.buf.offset((pos & self.cap_mask) as isize))
        }
    }

    pub fn recv_async(&self, have_lock: bool) -> Result<T, Error> {
        let read_pos = match self.get_read_pos() {
            Some(r) => r,
            _ => return Err(Error::Empty),
        };
        let val = self.get_mem(read_pos);
        self.set_read_start(read_pos);

        if self.sleeping_senders.load(SeqCst) > 0 {
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
        let mut rv = self.recv_async(false);
        if rv.is_ok() {
            return rv;
        }

        let mut guard = self.sleep_mutex.lock().unwrap();
        self.sleeping_receivers.fetch_add(1, SeqCst);
        loop {
            rv = self.recv_async(true);
            if rv.is_ok() {
                break;
            }
            // See the docs in send_sync.
            if self.peers_awake.fetch_sub(1, SeqCst) == 1 &&
                    self.sleeping_senders.load(SeqCst) == 0 {
                self.peers_awake.fetch_add(1, SeqCst);
                rv = Err(Error::Deadlock);
                break;
            } else {
                guard = self.recv_condvar.wait(guard).unwrap();
                self.peers_awake.fetch_add(1, SeqCst);
            }
        }
        self.sleeping_receivers.fetch_sub(1, SeqCst);

        rv
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Packet<'a, T> { }
unsafe impl<'a, T: Sendable+'a> Sync for Packet<'a, T> { }

impl<'a, T: Sendable+'a> Drop for Packet<'a, T> {
    fn drop(&mut self) {
        let wenr = self.write_end_next_read.load(SeqCst);
        let (write_end, read_start) = decompose_pointer(wenr);

        unsafe {
            for i in (0..write_end-read_start) {
                self.get_mem(read_start + i);
            }

            if mem::size_of::<T>() > 0 {
                deallocate(self.buf as *mut u8,
                           (self.cap_mask as usize + 1) * mem::size_of::<T>(),
                           mem::align_of::<T>());
            }
        }
    }
}

unsafe impl<'a, T: Sendable+'a> _Selectable<'a> for Packet<'a, T> {
    fn ready(&self) -> bool {
        if self.peers_awake.load(SeqCst) == 0 {
            return true;
        }
        let wenr = self.write_end_next_read.load(SeqCst);
        let (write_end, next_read) = decompose_pointer(wenr);
        write_end != next_read
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
