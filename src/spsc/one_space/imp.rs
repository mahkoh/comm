use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, Thread};
use std::cell::{Cell, UnsafeCell};
use std::sync::{StaticMutex, MUTEX_INIT};
use std::{mem};
use select::{_Selectable, Payload, WaitQueue};

use {Error};

const NONE:                  usize = 0b000000;
// Set if the sender has disconnected.
const SENDER_DISCONNECTED:   usize = 0b000001;
// Set if data is available for receiving.
const DATA_AVAILABLE:        usize = 0b000010;
// Set if the receiver is in the process of deciding whether it's going to sleep or not.
const RECEIVER_WORKING:      usize = 0b000100;
// Set if the receiver is sleeping.
const RECEIVER_SLEEPING:     usize = 0b001000;
// Set if the receiver has disconnected.
const RECEIVER_DISCONNECTED: usize = 0b010000;
// Set if someone is selecting on this channel.
const WAIT_QUEUE_USED:       usize = 0b100000;

const RECEIVER_FLAGS: usize = RECEIVER_WORKING|RECEIVER_SLEEPING|RECEIVER_DISCONNECTED;

pub struct Packet<'a, T: Send+'a> {
    // Id of this channel. Address of the arc::Inner that contains this channel.
    id:               Cell<usize>,
    // A collection of flags, see above.
    flags:            AtomicUsize,
    // A sleeping receiver thread.
    receiver_thread:  UnsafeCell<Option<Thread>>,
    // Data stored in this channel.
    data:             UnsafeCell<Option<T>>,
    // Mutex to synchronize wait_queue access.
    wait_queue_mutex: StaticMutex,
    wait_queue:       UnsafeCell<WaitQueue<'a>>,
}

impl<'a, T: Send+'a> Packet<'a, T> {
    /// Create a new Packet
    pub fn new() -> Packet<'a, T> {
        Packet {
            id:               Cell::new(0),
            flags:            AtomicUsize::new(NONE),
            receiver_thread:  UnsafeCell::new(None),
            data:             UnsafeCell::new(None),
            wait_queue_mutex: MUTEX_INIT,
            wait_queue:       UnsafeCell::new(WaitQueue::new()),
        }
    }

    /// Must be called before any other function.
    pub fn set_id(&self, id: usize) {
        self.id.set(id);
        self.wait_queue(|q| q.set_id(id));
    }

    /// Store `val` if the packet is empty and the receiver hasn't disconnected.
    ///
    /// This function must only be called by the Sender in the parent module.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        let mut flags = self.flags.load(Ordering::SeqCst);

        if flags & RECEIVER_DISCONNECTED != 0 {
            return Err((val, Error::Disconnected));
        }
        if flags & DATA_AVAILABLE != 0 {
            return Err((val, Error::Full));
        }

        unsafe { *self.data.get() = Some(val); }
        flags = self.flags.fetch_or(DATA_AVAILABLE, Ordering::SeqCst) | DATA_AVAILABLE;

        // Either the receiver has disconnected or it's currently working in another
        // thread or it's gone to sleep. If it's working then it'll either end up going to
        // sleep or clearing all receiver flags.
        //
        // It's also possible that `recv` is called twice while we're spinning and that
        // the data is no longer available during the second call. In that case we can't
        // do anything and just let it sleep.
        while flags & RECEIVER_FLAGS != 0 && flags & DATA_AVAILABLE != 0 {
            if flags & RECEIVER_SLEEPING != 0 {
                let receiver_thread = unsafe {
                    (*self.receiver_thread.get()).take().unwrap()
                };
                flags = self.flags.fetch_and(!RECEIVER_SLEEPING, Ordering::SeqCst);
                receiver_thread.unpark();
                break;
            }
            if flags & RECEIVER_DISCONNECTED != 0 {
                // It's guaranteed that if a receiver disconnects then it will not read
                // the data afterwards. Since we're here we know that the data must still
                // be available.
                let data = unsafe { (&mut *self.data.get()).take().unwrap() };
                self.flags.fetch_and(!DATA_AVAILABLE, Ordering::SeqCst);
                return Err((data, Error::Disconnected));
            }
            flags = self.flags.load(Ordering::SeqCst);
        }

        if flags & WAIT_QUEUE_USED != 0 {
            if self.wait_queue(|q| q.notify()) == 0 {
                self.flags.fetch_and(!WAIT_QUEUE_USED, Ordering::SeqCst);
            }
        }

        Ok(())
    }

    /// Disconnect the sender.
    ///
    /// This function must only be called from the Sender in the parent module.
    pub fn sender_disconnect(&self) {
        let mut flags = self.flags.fetch_or(SENDER_DISCONNECTED,
                                            Ordering::SeqCst) | SENDER_DISCONNECTED;

        // If the receiver is sleeping we wake it up without giving it data. The receiver
        // will interpret this as the Sender having disconnected.
        while flags & RECEIVER_FLAGS != 0 {
            if flags & RECEIVER_SLEEPING != 0 {
                let receiver_thread = unsafe {
                    (*self.receiver_thread.get()).take().unwrap()
                };
                self.flags.fetch_and(!RECEIVER_SLEEPING, Ordering::SeqCst);
                receiver_thread.unpark();
                break;
            }
            if flags & RECEIVER_DISCONNECTED != 0 {
                break;
            }
            flags = self.flags.load(Ordering::SeqCst);
        }

        if flags & WAIT_QUEUE_USED != 0 {
            self.wait_queue(|q| q.notify());
        }
    }

    /// Receive an element and block if none are available.
    ///
    /// This function must only be called by the Receiver in the parent module.
    pub fn recv_sync(&self) -> Result<T, Error> {
        let mut flags = self.flags.fetch_or(RECEIVER_WORKING, Ordering::SeqCst);

        // No data is available and the sender hasn't disconnected yet. We sleep until the
        // sender wakes us up, either because data becomes available or it disconnected.
        if (flags & DATA_AVAILABLE == 0) && (flags & SENDER_DISCONNECTED == 0) {
            unsafe { *self.receiver_thread.get() = Some(thread::current()); }
            self.flags.fetch_or(RECEIVER_SLEEPING, Ordering::SeqCst);
            flags |= RECEIVER_SLEEPING;

            // There are two subtleties here:
            //
            // 1) We cannot check the DATA_AVAILABLE flag. This is because the
            //    `RECEIVER_SLEEPING` flag signals to the sender thread that the
            //    `receiver_thread` variable is set to Some(...). We can never unset the
            //    `RECEIVER_SLEEPING` flag ourselves because then the receiver thread
            //    might call `unwrap` on an empty `receiver_thread`.
            //
            // 2) There is a short moment here between us setting `RECEIVER_SLEEPING` and
            //    us actually going to sleep. One should ask oneself what happens if the
            //    sender thread calls `Thread::unpark` before we've actually gone to
            //    sleep. This works because `Thread::park` is backed by a semaphore.
            //    `Thread::park` will wake up immediately if the situation described here
            //    happens. On the other hand, since we're calling `Thread::park` in a
            //    loop and set `RECEIVER_SLEEPING` right before the loop, two subsequent
            //    calls to `recv_sync` won't influence each other, even if the semaphore
            //    is in the wrong state after the first call.
            while flags & RECEIVER_SLEEPING != 0 {
                thread::park();
                flags = self.flags.load(Ordering::SeqCst);
            }
        }

        let ret = if flags & DATA_AVAILABLE == 0 {
            // If we woke up without data being available then that means the sender woke
            // us up because it disconnected.
            Err(Error::Disconnected)
        } else {
            let data = unsafe { (*self.data.get()).take().unwrap() };
            self.flags.fetch_and(!DATA_AVAILABLE, Ordering::SeqCst);
            Ok(data)
        };
        self.flags.fetch_and(!RECEIVER_WORKING, Ordering::SeqCst);

        ret
    }

    /// Receive an element without blocking.
    ///
    /// This function must only be called by the Receiver in the parent module.
    pub fn recv_async(&self) -> Result<T, Error> {
        let flags = self.flags.load(Ordering::SeqCst);
        if flags & DATA_AVAILABLE == 0 {
            if flags & SENDER_DISCONNECTED != 0 {
                Err(Error::Disconnected)
            } else {
                Err(Error::Empty)
            }
        } else {
            let data = unsafe { (*self.data.get()).take().unwrap() };
            self.flags.fetch_and(!DATA_AVAILABLE, Ordering::SeqCst);
            Ok(data)
        }
    }

    /// Disconnect the receiver.
    ///
    /// This function must only be called from the Receiver in the parent module.
    pub fn recv_disconnect(&self) {
        self.flags.fetch_or(RECEIVER_DISCONNECTED, Ordering::SeqCst);
    }

    /// Get the wait queue.
    pub fn wait_queue<F, U>(&self, f: F) -> U where F: FnOnce(&mut WaitQueue<'a>) -> U {
        unsafe {
            let mutex: &'static StaticMutex = mem::transmute(&self.wait_queue_mutex);
            let _guard = mutex.lock().unwrap();
            f(&mut *self.wait_queue.get())
        }
    }
}

unsafe impl<'a, T: Send+'a> Sync for Packet<'a, T> { }
unsafe impl<'a, T: Send+'a> Send for Packet<'a, T> { }

unsafe impl<'a, T: Send+'a> _Selectable<'a> for Packet<'a, T> {
    fn ready(&self) -> bool {
        self.flags.load(Ordering::SeqCst) & (DATA_AVAILABLE | SENDER_DISCONNECTED) != 0
    }

    fn register(&self, load: Payload<'a>) {
        if self.wait_queue(|q| q.add(load)) > 0 {
            self.flags.fetch_or(WAIT_QUEUE_USED, Ordering::SeqCst);
        }
    }

    fn unregister(&self, id: usize) {
        if self.wait_queue(|q| q.remove(id)) == 0 {
            self.flags.fetch_and(!WAIT_QUEUE_USED, Ordering::SeqCst);
        }
    }
}

#[unsafe_destructor]
impl<'a, T: Send+'a> Drop for Packet<'a, T> {
    fn drop(&mut self) {
        unsafe {
            let mutex: &'static StaticMutex = mem::transmute(&self.wait_queue_mutex);
            mutex.destroy();
        }
    }
}
