//! An unbounded SPMC channel.
//!
//! See the unbounded SPSC documentation.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error, Sendable};
use std::ptr;
use std::raw::TraitObject;

mod imp;
#[cfg(test)] mod test;

/// Creates a new unbounded SPMC channel.
pub fn new<T: Sendable>() -> (Producer<T>, Consumer<T>) {
    let packet = Arc::new(imp::Packet::new());
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing end of an unbounded SPMC channel.
pub struct Producer<T: Sendable> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Sendable> Producer<T> {
    /// Appends a message to the channel.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - All receivers have disconnected.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send(val)
    }
}

impl<T: Sendable> Drop for Producer<T> {
    fn drop(&mut self) {
        self.data.remove_sender()
    }
}

unsafe impl<T: Sendable> Send for Producer<T> { }

/// The receiving end of an unbounded SPMC channel.
pub struct Consumer<T: Sendable> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Sendable> Consumer<T> {
    /// Receives a message from the channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the sender has disconnected.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message from the channel. Does not block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and the sender has disconnected.
    /// - `Empty` - The channel is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async()
    }
}

impl<T: Sendable> Clone for Consumer<T> {
    fn clone(&self) -> Consumer<T> {
        self.data.add_receiver();
        Consumer { data: self.data.clone() }
    }
}

impl<T: Sendable> Drop for Consumer<T> {
    fn drop(&mut self) {
        self.data.remove_receiver()
    }
}

unsafe impl<T: Sendable> Send for Consumer<T> { }

impl<T: Sendable> Selectable for Consumer<T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable> {
        unsafe { self.data.as_trait(ptr::read(&(&*self.data as &(_Selectable)) as *const _ as *const TraitObject)) }
    }
}
