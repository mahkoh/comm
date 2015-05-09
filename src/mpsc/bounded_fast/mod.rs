//! A bounded MPSC channel.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error, Sendable};
use std::ptr;
use std::raw::TraitObject;

mod imp;
#[cfg(test)] mod test;

/// Creates a new bounded MPSC channel with capacity at least `cap`.
///
/// # Safety
///
/// This is unsafe because under just the right circumstances this implementation can lead
/// to undefined behavior. Note that these circumstances are extremely rare and almost
/// impossible on 64 bit systems.
pub unsafe fn new<T: Sendable>(cap: usize) -> (Producer<T>, Consumer<T>) {
    let packet = Arc::new(imp::Packet::new(cap));
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// A producer of a bounded MPSC channel.
pub struct Producer<T: Sendable> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Sendable> Producer<T> {
    /// Sends a message over the channel. Blocks if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The consumer has disconnected.
    pub fn send_sync(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send_sync(val)
    }

    /// Sends a message over the channel. Does not block if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The consumer has disconnected.
    /// - `Full` - The buffer is full.
    pub fn send_async(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send_async(val, false)
    }
}

unsafe impl<T: Sendable> Send for Producer<T> { }

impl<T: Sendable> Drop for Producer<T> {
    fn drop(&mut self) {
        self.data.remove_sender();
    }
}

impl<T: Sendable> Clone for Producer<T> {
    fn clone(&self) -> Producer<T> {
        self.data.add_sender();
        Producer { data: self.data.clone(), }
    }
}

/// A consumer of a bounded SPMC channel.
pub struct Consumer<T: Sendable> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Sendable> Consumer<T> {
    /// Receives a message from the channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - All producers have disconnected and the channel is empty.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message over the channel. Does not block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - All producers have disconnected and the channel is empty.
    /// - `Empty` - The buffer is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async(false)
    }
}

unsafe impl<T: Sendable> Send for Consumer<T> { }

impl<T: Sendable> Drop for Consumer<T> {
    fn drop(&mut self) {
        self.data.remove_receiver();
    }
}

impl<T: Sendable> Selectable for Consumer<T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable> {
        unsafe { self.data.as_trait(ptr::read(&(&*self.data as &(_Selectable)) as *const _ as *const TraitObject)) }
    }
}
