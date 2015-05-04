//! An SPSC channel with a buffer size of one.
//!
//! This channel should mostly be used if the sender only sends a single message.
//!
//! ### Example
//!
//! Consider the case of an event loop. To request information from the event loop,
//! another thread might send the event loop a message and the event loop will send the
//! answer over the channel that was sent together with the request.

use arc::{Arc, ArcTrait};
use self::imp::{Packet};
use select::{Selectable, _Selectable};
use {Error, Sendable};

mod imp;
pub mod stack;
#[cfg(test)] mod test;
#[cfg(test)] mod bench;

/// Creates a new SPSC one space channel.
pub fn new<'a, T: Sendable+'a>() -> (Producer<'a, T>, Consumer<'a, T>) {
    let packet = Arc::new(Packet::new());
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing half of an SPSC one space channel.
pub struct Producer<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Producer<'a, T> {
    /// Sends a message over this channel. Doesn't block if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The receiver has disconnected.
    /// - `Full` - The channel is full.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send(val)
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Producer<'a, T> { }

impl<'a, T: Sendable+'a> Drop for Producer<'a, T> {
    fn drop(&mut self) {
        self.data.sender_disconnect();
    }
}

/// The consuming half of an SPSC one space channel.
pub struct Consumer<'a, T: Sendable+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Sendable+'a> Consumer<'a, T> {
    /// Receives a message from this channel. Doesn't block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the sender has disconnected.
    /// - `Empty` - The channel is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async()
    }

    /// Receives a message over this channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The sender has disconnected.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Returns whether the channel is non-empty.
    pub fn can_recv(&self) -> bool {
        self.data.ready()
    }
}

unsafe impl<'a, T: Sendable+'a> Send for Consumer<'a, T> { }

impl<'a, T: Sendable+'a> Drop for Consumer<'a, T> {
    fn drop(&mut self) {
        self.data.recv_disconnect();
    }
}

impl<'a, T: Sendable+'a> Selectable<'a> for Consumer<'a, T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable<'a>+'a> {
        unsafe { self.data.as_trait(&*self.data as &(_Selectable+'a)) }
    }
}
