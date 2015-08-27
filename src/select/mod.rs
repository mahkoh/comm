//! A structure for polling channels and other objects.
//!
//! This structure makes it possible to suspend the current task until at least one of
//! several objects we're waiting for is ready.
//!
//! ### Example
//!
//! ```ignore
//! let user_input_chan = ...
//! let network_event_chan = ...
//!
//! let select = Select::new();
//! select.add(&user_input_chan);
//! select.add(&network_event_chan);
//!
//! loop {
//!     for &mut id in select.wait(&mut [0, 0]) {
//!         if id == user_input_chan.id() {
//!             // handle user input
//!         } else if id == network_event_chan.id() {
//!             // handle network events
//!         }
//!     }
//! }
//! ```
//!
//! An arbitrary number of targets can be added to the `Select` object without affecting
//! the `wait` performance. The performance of `wait` only depends on the number of
//! objects that are ready. On the other hand, creating a `Select` object and adding
//! targets to the `Select` objects are non-trivial operations.
//!
//! The same `Select` object can be shared and sent between multiple threads. If multiple
//! threads are waiting on the same `Select` object, exactly one of them will be woken
//! when a target becomes ready. The others will continue to sleep until another target
//! becomes ready.
//!
//! `wait` will return an increasing number of unique ids that should be compared to the
//! return values of the `id` functions of `Selectable` objects. Therefore, all ready
//! targets can be found in `O(number_of_targets)` or
//! `number_of_ready_targets*O(log(number_of_targets))`.
//!
//! ### Implementation
//!
//! The following strategy (very similar to epoll) is used in the implementation;
//!
//! Each `Select` object stores two lists: `wait_list` and `ready_list`. The `wait_list`
//! contains all targets that are currently in the `Select` object. The `ready_list`
//! contains targets that were at some point ready.
//!
//! When a target is added to the `Select` object, it first tells the target that it wants
//! to be notified when the target becomes ready. Then it checks if the target is ready
//! and, if so, adds it to the `ready_list`. If a target becomes ready while it's
//! registered with the `Select` object, the target adds itself to the `ready_list`.
//!
//! When `wait` is called, the `Select` object first removes all elements from the
//! `ready_list` that are no longer ready. If the list isn't empty afterwards, the
//! `Select` object copies a prefix of the `ready_list` into the user-supplied buffer and
//! returns immediately. Otherwise it suspends the current thread until one of the targets
//! adds itself to the `ready_list` and wakes the thread up. Then the `Select` object
//! copies a prefix of the `ready_list` into the user-supplied buffer and returns.
//!
//! To keep the API simple, this module also provides a `WaitQueue` structure which the
//! targets have to store to interact with `Select` objects.

pub use self::imp::{Select, WaitQueue, Payload};

use arc::{ArcTrait};
use {Sendable};

mod imp;
//#[cfg(test)] mod test;

// Traits are here because https://github.com/rust-lang/rust/issues/16264

/// An object that can be selected on.
pub trait Selectable {
    /// Returns the id stored by `Select::wait` when this object is ready.
    fn id(&self) -> usize;
    /// Returns the interior object that will be stored in the `Select` object.
    fn as_selectable(&self) -> ArcTrait<_Selectable>;
}

/// The object that will be stored in a `Select` structure while the `Selectable` object
/// is registered.
///
/// Implementing this trait is unsafe because the behavior is undefined if the
/// implementation doesn't follow the rules below.
pub unsafe trait _Selectable: Sync+Sendable {
    /// Returns `true` if the object is ready, `false` otherwise.
    ///
    /// This function must not try to acquire any locks that are also held while the
    /// implementation interacts with the `WaitQueue` object.
    fn ready(&self) -> bool;
    /// Registers a `Select` object with the `Selectable` object. The payload must be
    /// passed to the `WaitQueue`.
    fn register(&self, Payload);
    /// Unregisters a `Select` objects from the `Selectable` object. The id must be passed
    /// to the `WaitQueue`.
    fn unregister(&self, id: usize);
}
