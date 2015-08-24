#![crate_type = "lib"]
#![crate_name = "comm"]
#![feature(box_syntax, core, alloc, oom, heap_api,
           unsafe_no_drop_flag, filling_drop, wait_timeout, wait_timeout_with,
           static_mutex, raw, nonzero, drain, num_bits_bytes)]
#![cfg_attr(test, feature(test, scoped))]
#![cfg_attr(test, allow(deprecated))]
#![allow(dead_code, trivial_casts, trivial_numeric_casts,
         drop_with_repr_extern)]

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
//! use std::{thread};
//! use comm::{spsc};
//!
//! // Create a bounded SPSC channel.
//! let (send, recv) = spsc::bounded::new(10);
//! thread::spawn(move || {
//!     send.send_sync(10).unwrap();
//! });
//! assert_eq!(recv.recv_sync().unwrap(), 10);
//! ```
//!
//! Shared usage:
//!
//! ```
//! use std::{thread};
//! use comm::{mpsc};
//!
//! // Create an unbounded MPSC channel.
//! let (send, recv) = mpsc::unbounded::new();
//! for i in 0..10 {
//!     let send = send.clone();
//!     thread::spawn(move || {
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
//! #![feature(std_misc, thread_sleep)]
//!
//! use std::thread::{self, sleep_ms};
//! use comm::{spsc};
//! use comm::select::{Select, Selectable};
//!
//! let mut channels = vec!();
//! for i in 0..10 {
//!     let (send, recv) = spsc::one_space::new();
//!     channels.push(recv);
//!     thread::spawn(move || {
//!         sleep_ms(100);
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
#[cfg(test)] extern crate test;

pub use marker::{Sendable};

mod sortedvec;
mod marker;

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
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error {
    Disconnected,
    Full,
    Empty,
    Deadlock,
}
