//! An unbounded SPSC channel.
//!
//! This channel can store an unbounded number of messages, however, sending a message
//! might be more expensive than with bounded channels, and if the receiver can't dequeue
//! messages fast enough, one might run out of memory.
//!
//! ### Example
//!
//! Consider the case of a short-lived producer. The producer might produce an unknown
//! number of messages in a short amount of time and then disconnect. With an unbounded
//! channel the producer will never block and the consumer can start processing the
//! messages before the producer is finished.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error, Sendable};

mod imp;
#[cfg(test)] mod test;
#[cfg(test)] mod bench;

/// Creates a new unbounded SPSC channel.
pub fn new<'a, T: Sendable+'a>() -> (Producer<'a, T>, Consumer<'a, T>) {
    let packet = Arc::new(imp::Packet::new());
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing half on an unbounded SPSC channel.
pub struct Producer<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Producer<'a, T> {
    /// Appends a new message to the channel.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The receiver has disconnected.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
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

/// The consuming half on an unbounded SPSC channel.
pub struct Consumer<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Consumer<'a, T> {
    /// Receives a message from the channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the receiver has disconnected.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message from the channel. Does not block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the receiver has disconnected.
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
