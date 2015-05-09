//! An SPSC channel with a buffer size of one stored on the stack.

use std::{mem};
use super::imp::{Packet};
use {Error, Sendable};

/// Creates a new SPSC one space channel.
pub fn new<T: Sendable>() -> Slot<T> {
    Slot { data: Packet::new() }
}

/// Storage for an SPSC one space channel.
pub struct Slot<T: Sendable> {
    data: Packet<T>,
}

impl<T: Sendable> Slot<T> {
    /// Split the slot into a producing and a consuming end.
    pub fn split(&mut self) -> (&Producer<T>, &Consumer<T>) {
        unsafe {
            let prod = mem::transmute_copy(&self);
            let cons = mem::transmute(self);
            (prod, cons)
        }
    }
}

/// The producing half of an SPSC one space channel.
pub struct Producer<T: Sendable> {
    data: Packet<T>,
}

impl<T: Sendable> Producer<T> {
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

unsafe impl<T: Sendable> Send for Producer<T> { }

impl<T: Sendable> Drop for Producer<T> {
    fn drop(&mut self) {
        self.data.sender_disconnect();
    }
}

/// The consuming half of an SPSC one space channel.
pub struct Consumer<T: Sendable> {
    data: Packet<T>,
}

impl<T: Sendable> Consumer<T> {
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
}

unsafe impl<T: Sendable> Send for Consumer<T> { }

impl<T: Sendable> Drop for Consumer<T> {
    fn drop(&mut self) {
        self.data.recv_disconnect();
    }
}
