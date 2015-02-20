//! An unbounded SPMC channel.
//!
//! See the unbounded SPSC documentation.

use arc::{Arc, ArcTrait};
use select::{Selectable, _Selectable};
use {Error};

mod imp;
#[cfg(test)] mod test;

/// Creates a new unbounded SPMC channel.
pub fn new<T: Send+'static>() -> (Producer<T>, Consumer<T>) {
    let packet = Arc::new(imp::Packet::new());
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing end of an unbounded SPMC channel.
pub struct Producer<T: Send+'static> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Send+'static> Producer<T> {
    /// Appends a message to the channel.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - All receivers have disconnected.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send(val)
    }
}

#[unsafe_destructor]
impl<T: Send+'static> Drop for Producer<T> {
    fn drop(&mut self) {
        self.data.remove_sender()
    }
}

unsafe impl<T: Send+'static> Send for Producer<T> { }

/// The receiving end of an unbounded SPMC channel.
pub struct Consumer<T: Send+'static> {
    data: Arc<imp::Packet<T>>,
}

impl<T: Send+'static> Consumer<T> {
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

impl<T: Send+'static> Clone for Consumer<T> {
    fn clone(&self) -> Consumer<T> {
        self.data.add_receiver();
        Consumer { data: self.data.clone() }
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
