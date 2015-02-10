//! Single-producer single-consumer (SPSC) channels.
//!
//! An SPSC channel has exactly two endpoints which cannot be cloned.

pub mod one_space;
pub mod bounded;
pub mod ring_buf;
pub mod unbounded;
