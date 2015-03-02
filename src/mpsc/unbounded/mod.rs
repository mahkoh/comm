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
pub fn new<'a, T: Send+'a>() -> (Producer<'a, T>, Consumer<'a, T>) {
    let packet = Arc::new(imp::Packet::new());
    packet.set_id(packet.unique_id());
    (Producer { data: packet.clone() }, Consumer { data: packet })
}

/// The producing end of an unbounded MPSC channel.
pub struct Producer<'a, T: Send+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Send+'a> Producer<'a, T> {
    /// Appends a message to the channel.
    ///
    /// ### Error
    ///
    /// - `Disconnected` - The receiver has disconnected.
    pub fn send(&self, val: T) -> Result<(), (T, Error)> {
        self.data.send(val)
    }
}

impl<'a, T: Send+'a> Clone for Producer<'a, T> {
    fn clone(&self) -> Producer<'a, T> {
        self.data.add_sender();
        Producer { data: self.data.clone() }
    }
}

#[unsafe_destructor]
impl<'a, T: Send+'a> Drop for Producer<'a, T> {
    fn drop(&mut self) {
        self.data.remove_sender()
    }
}

unsafe impl<'a, T: Send+'a> Send for Producer<'a, T> { }

/// The consuming end of an unbounded MPSC channel.
pub struct Consumer<'a, T: Send+'a> {
    data: Arc<imp::Packet<'a, T>>,
}

impl<'a, T: Send+'a> Consumer<'a, T> {
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
impl<'a, T: Send+'a> Drop for Consumer<'a, T> {
    fn drop(&mut self) {
        self.data.remove_receiver()
    }
}

unsafe impl<'a, T: Send+'a> Send for Consumer<'a, T> { }

impl<'a, T: Send+'a> Selectable<'a> for Consumer<'a, T> {
    fn id(&self) -> usize {
        self.data.unique_id()
    }

    fn as_selectable(&self) -> ArcTrait<_Selectable<'a>+'a> {
        unsafe { self.data.as_trait(&*self.data as &(_Selectable+'a)) }
    }
}
