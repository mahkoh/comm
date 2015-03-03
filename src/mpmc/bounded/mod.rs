//! A bounded MPMC channel.
//!
//! See the documentation of the parent module and the bounded SPSC docs for details.
//!
//! ### Performance
//!
//! This implementation suffers from some performance problems when the number of active
//! endpoints is larger than the number of cpu cores.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error, Sendable};

mod imp;
#[cfg(test)] mod test;

/// An endpoint of a bounded MPMC channel.
pub struct Channel<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Channel<'a, T> {
    /// Creates a new bounded MPMC channel with capacity at least `cap`.
    ///
    /// ### Panic
    ///
    /// Panics under any of the following conditions:
    ///
    /// - `sizeof(usize) == 4 && cap > 2^15`,
    /// - `sizeof(usize) == 8 && cap > 2^31`,
    /// - `next_power_of_two(cap) * sizeof(T) >= isize::MAX`.
    pub fn new(cap: usize) -> Channel<'a, T> {
        let packet = Arc::new(imp::Packet::new(cap));
        packet.set_id(packet.unique_id());
        Channel { data: packet }
    }

    /// Sends a message over the channel. Blocks if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Deadlock` - All other endpoints are currently blocked trying to send a message.
    pub fn send_sync(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send_sync(val)
    }

    /// Sends a message over the channel. Does not block if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Full` - The buffer is full.
    pub fn send_async(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send_async(val, false)
    }

    /// Receives a message from the channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Deadlock` - All other endpoints are currently blocked trying to receive a
    ///   message.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message over the channel. Does not block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Empty` - The buffer is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async(false)
    }
}

unsafe impl<'a, T: Sendable> Sync for Channel<'a, T> { }
unsafe impl<'a, T: Sendable> Send for Channel<'a, T> { }

impl<'a, T: Sendable+'a> Clone for Channel<'a, T> {
    fn clone(&self) -> Channel<'a, T> {
        self.data.add_peer();
        Channel { data: self.data.clone(), }
    }
}

#[unsafe_destructor]
impl<'a, T: Sendable+'a> Drop for Channel<'a, T> {
    fn drop(&mut self) {
        self.data.remove_peer();
    }
}

impl<'a, T: Sendable+'a> Selectable<'a> for Channel<'a, T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable<'a>+'a> {
        unsafe { self.data.as_trait(&*self.data as &(_Selectable+'a)) }
    }
}
