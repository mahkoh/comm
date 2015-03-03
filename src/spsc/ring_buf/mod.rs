//! A bounded SPSC channel that overwrites older messages when the buffer is full.
//!
//! ### Example
//!
//! Consider the case of an audio producer and consumer. If, at some point, the consumer
//! is slow, you might not want to block the producer and instead overwrite older,
//! unconsumed audio samples so that the delay between producer and consumer is bounded
//! above by the buffer size of the channel.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error, Sendable};

mod imp;
#[cfg(test)] mod test;

/// Creates a new SPSC ring buffer channel.
///
/// ### Panic
///
/// Panics if `next_power_of_two(cap) * sizeof(T) >= isize::MAX`.
pub fn new<'a, T: Sendable+'a>(cap: usize) -> (Producer<'a, T>, Consumer<'a, T>) {
    let packet = Arc::new(imp::Packet::new(cap));
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing half of an SPSC ring buffer channel.
pub struct Producer<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Producer<'a, T> {
    /// Sends a message over this channel. Returns an older message if the buffer is full.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The receiver has disconnected.
    pub fn send(&self, val: T) -> Result<Option<T>, (T, Error)> {
        self.data.send(val)
    }
}

#[unsafe_destructor]
impl<'a, T: Sendable+'a> Drop for Producer<'a, T> {
    fn drop(&mut self) {
        self.data.disconnect_sender()
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Producer<'a, T> { }

/// The sending half of an SPSC channel.
pub struct Consumer<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Consumer<'a, T> {
    /// Receives a message from the channel. Blocks if the buffer is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the sender has disconnected.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message from the channel. Does not block if the buffer is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the sender has disconnected.
    /// - `Empty` - The channel is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async()
    }
}

#[unsafe_destructor]
impl<'a, T: Sendable+'a> Drop for Consumer<'a, T> {
    fn drop(&mut self) {
        self.data.disconnect_receiver()
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Consumer<'a, T> { }

impl<'a, T: Sendable+'a> Selectable<'a> for Consumer<'a, T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable<'a>+'a> {
        unsafe { self.data.as_trait(&*self.data as &(_Selectable+'a)) }
    }
}
