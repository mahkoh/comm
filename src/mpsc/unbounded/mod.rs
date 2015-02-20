//! An unbounded MPSC channel.
//!
//! See the unbounded SPSC docs.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error};

mod imp;
#[cfg(test)] mod test;
#[cfg(test)] mod bench;

/// Creates a new unbounded MPSC channel.
pub fn new<T: Send+'static>() -> (Producer<T>, Consumer<T>) {
    let packet = Arc::new(imp::Packet::new());
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing end of an unbounded MPSC channel.
pub struct Producer<T: Send+'static> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Send+'static> Producer<T> {
    /// Appends a message to the channel.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The receiver has disconnected.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send(val)
    }
}

impl<T: Send+'static> Clone for Producer<T> {
    fn clone(&self) -> Producer<T> {
        self.data.add_sender();
        Producer { data: self.data.clone() }
    }
}

#[unsafe_destructor]
impl<T: Send+'static> Drop for Producer<T> {
    fn drop(&mut self) {
        self.data.remove_sender()
    }
}

unsafe impl<T: Send+'static> Send for Producer<T> { }

/// The consuming end of an unbounded MPSC channel.
pub struct Consumer<T: Send+'static> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Send+'static> Consumer<T> {
    /// Receives a message from this channel. Blocks if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and all senders have disconnected.
    pub fn recv_sync(&self) -> Result<T, Error> {
        self.data.recv_sync()
    }

    /// Receives a message from this channel. Does not block if the channel is empty.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The channel is empty and all senders have disconnected.
    /// - `Empty` - The channel is empty.
    pub fn recv_async(&self) -> Result<T, Error> {
        self.data.recv_async()
    }
}

#[unsafe_destructor]
impl<T: Send+'static> Drop for Consumer<T> {
    fn drop(&mut self) {
        self.data.remove_receiver()
    }
}

unsafe impl<T: Send+'static> Send for Consumer<T> { }

impl<T: Send+'static> Selectable for Consumer<T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable> {
        unsafe { self.data.as_trait(self.data.static_ref() as &_Selectable) }
    }
}
