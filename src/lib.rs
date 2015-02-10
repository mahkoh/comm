#![crate_type = "lib"]
#![crate_name = "comm"]
#![feature(unsafe_destructor, box_syntax, core, alloc, collections, hash, std_misc)]
#![cfg_attr(test, feature(io))]
#![allow(dead_code)]

//! Communication primitives.
//!
//! This library provides types for message passing between threads and polling.
//! Concretely, it provides
//!
//! - Single-producer single-consumer (SPSC),
//! - Single-producer multiple-consumers (SPMC),
//! - Multiple-producers single-consumer (MPSC), and
//! - Multiple-producers multiple-consumers (MPMC)
//!
//! channels of different flavors and a `Select` object which can poll the consuming ends
//! of these channels for readiness.
//!
//! ### Examples
//!
//! Simple usage:
//!
//! ```
//! use std::thread::{Thread};
//! use comm::{spsc};
//! 
//! // Create a bounded SPSC channel.
//! let (send, recv) = spsc::bounded::new(10);
//! Thread::spawn(move || {
//!     send.send_sync(10).unwrap();
//! });
//! assert_eq!(recv.recv_sync().unwrap(), 10);
//! ```
//!
//! Shared usage:
//!
//! ```
//! use std::thread::{Thread};
//! use comm::{mpsc};
//! 
//! // Create an unbounded MPSC channel.
//! let (send, recv) = mpsc::unbounded::new();
//! for i in 0..10 {
//!     let send = send.clone();
//!     Thread::spawn(move || {
//!         send.send(i).unwrap();
//!     });
//! }
//! drop(send);
//! while let Ok(n) = recv.recv_sync() {
//!     println!("{}", n);
//! }
//! ```
//!
//! Selecting:
//!
//! ```
//! use std::thread::{Thread};
//! use std::old_io::{timer};
//! use std::time::duration::{Duration};
//! use comm::{spsc};
//! use comm::select::{Select, Selectable};
//! 
//! let mut channels = vec!();
//! for i in 0..10 {
//!     let (send, recv) = spsc::one_space::new();
//!     channels.push(recv);
//!     Thread::spawn(move || {
//!         timer::sleep(Duration::milliseconds(100));
//!         send.send(i).ok();
//!     });
//! }
//! let select = Select::new();
//! for recv in &channels {
//!     select.add(recv);
//! }
//! let first_ready = select.wait(&mut [0])[0];
//! for recv in &channels {
//!     if first_ready == recv.id() {
//!         println!("First ready: {}", recv.recv_sync().unwrap());
//!         return;
//!     }
//! }
//! ```

extern crate core;
extern crate alloc;

mod sortedvec;

pub mod arc;
pub mod select;
pub mod spsc;
pub mod spmc;
pub mod mpsc;
pub mod mpmc;

/// Errors that can happen during receiving and sending.
///
/// See the individual functions for a list of errors they can return and the specific
/// meaning.
#[derive(Debug, Copy, PartialEq)]
pub enum Error {
    Disconnected,
    Full,
    Empty,
    Deadlock,
}
