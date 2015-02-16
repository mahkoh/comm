//! Single-producer multiple-consumers (SPMC) channels.
//!
//! An SPMC channel has exactly one producer and an arbitrary number of consumers which
//! can be cloned. Unless otherwise noted, each message is received by at most one
//! consumer, i.e., messages are not cloned.

pub mod unbounded;
pub mod bounded_fast;
