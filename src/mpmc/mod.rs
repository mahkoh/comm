//! Multiple-producers multiple-consumers (MPMC) channels.
//!
//! MPMC channels are the most general channel type. There is no distinction between peers
//! that can send messages and peers that can receive messages. Every channel endpoint can
//! receive and send messages and channel endpoints can be cloned.
//!
//! MPMC channels can suffer from deadlocks if all endpoints are trying to send to a full
//! channel or receive from an empty channel at the same time. We try to avoid this by
//! counting the number of endpoints blocked on each operation, but this only works if
//! there is only one endpoint per thread.

pub mod bounded;
