//! A bounded SPMC channel.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error};

mod imp;
#[cfg(test)] mod test;

/// Creates a new bounded MPMC channel with capacity at least `cap`.
///
/// # Safety
///
/// This is unsafe because under just the right circumstances this implementation can lead
/// to undefined behavior. Note that these circumstances are extremely rare and almost
/// impossible on 64 bit systems.
pub unsafe fn new<T: Send+'static>(cap: usize) -> (Producer<T>, Consumer<T>) {
    let packet = Arc::new(imp::Packet::new(cap));
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// A producer of a bounded SPMC channel.
pub struct Producer<T: Send+'static> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Send+'static> Producer<T> {
    /// Sends a message over the channel. Blocks if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - All receivers have disconnected.
    pub fn send_sync(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send_sync(val)
    }

    /// Sends a message over the channel. Does not block if the channel is full.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - All receivers have disconnected and the buffer is full.
    /// - `Full` - The buffer is full.
    pub fn send_async(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send_async(val, false)
    }
}

unsafe impl<T: Send+'static> Send for Producer<T> { }

#[unsafe_destructor]
impl<T: Send+'static> Drop for Producer<T> {
    fn drop(&mut self) {
        self.data.remove_sender();
    }
}

/// A consumer of a bounded SPMC channel.
pub struct Consumer<T: Send+'static> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Send+'static> Consumer<T> {
    /// Receives a message from the channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The sender has disconnected and the channel is empty.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message over the channel. Does not block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The sender has disconnected and the channel is empty.
    /// - `Empty` - The buffer is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async(false)
    }
}

unsafe impl<T: Send+'static> Send for Consumer<T> { }

impl<T: Send+'static> Clone for Consumer<T> {
    fn clone(&self) -> Consumer<T> {
        self.data.add_receiver();
        Consumer { data: self.data.clone(), }
    }
}

#[unsafe_destructor]
impl<T: Send+'static> Drop for Consumer<T> {
    fn drop(&mut self) {
        self.data.remove_receiver();
    }
}

impl<T: Send+'static> Selectable for Consumer<T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable> {
        unsafe { self.data.as_trait(&*self.data as &(_Selectable+'static)) }
    }
}
