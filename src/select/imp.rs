use std::collections::{HashMap};
use std::hash::{Hash, Hasher, Writer};
use std::sync::{Mutex, Condvar};
use std::cmp::{self, Ordering};
use std::{mem};

use arc::{Arc, Weak, WeakTrait};
use sortedvec::{SortedVec};
use super::{Selectable, _Selectable};

/// Container for all targets being selected on.
pub struct Select {
    condvar: Arc<Condvar>,
    inner: Arc<Mutex<Inner>>,
}

impl Select {
    /// Creates a new `Select` object.
    pub fn new() -> Select {
        let condvar = Arc::new(Condvar::new());
        Select {
            condvar: condvar.clone(),
            inner: Arc::new(Mutex::new(Inner::new(condvar)))
        }
    }

    fn as_payload(&self) -> Payload {
        Payload { data: self.inner.downgrade() }
    }

    /// Adds a target to the select object.
    pub fn add<T: Selectable>(&self, sel: &T) {
        let sel = sel.as_selectable();

        // Careful not to deadlock in `register`.
        sel.register(self.as_payload());

        let mut inner = self.inner.lock().unwrap();

        let id = sel.unique_id();

        if sel.ready() {
            inner.ready_list.insert(id);
        }

        inner.wait_list.insert(id, Entry { data: sel.downgrade() });
    }

    /// Removes a target from the `Select` object. Returns `true` if the target was
    /// previously registered in the `Select` object, `false` otherwise.
    pub fn remove<T: Selectable>(&self, sel: &T) -> bool {
        let sel = sel.as_selectable();

        let mut inner = self.inner.lock().unwrap();

        if inner.wait_list.remove(&sel.unique_id()).is_none() {
            return false;
        }
        inner.ready_list.remove(&sel.unique_id());

        // Careful not to deadlock in `unregister`.
        drop(inner);

        sel.unregister(self.inner.unique_id());

        true
    }

    /// Waits for any of the targets in the `Select` object to become ready. The ids of
    /// the ready targets will be stored in `ready`. Returns the prefix containing the set
    /// of stored `ids`.
    ///
    /// If the select object is empty, an empty slice is returned immediately.
    pub fn wait<'a>(&self, ready: &'a mut [usize]) -> &'a mut [usize] {
        let mut inner = self.inner.lock().unwrap();

        if inner.wait_list.is_empty() {
            return &mut [];
        }

        if let Some(n) = inner.check_ready_list(ready) {
            return &mut ready[..n];
        }

        while inner.ready_list.len() == 0 {
            inner = self.condvar.wait(inner).unwrap();
        }

        let min = cmp::min(ready.len(), inner.ready_list.len());
        for i in 0..min {
            ready[i] = inner.ready_list[i];
        }
        &mut ready[..min]
    }

    /// Checks if any of the targets are ready and, if so, copies them into the array.
    /// Does not block if no target is ready.
    pub fn check_ready_list<'a>(&self, ready: &'a mut [usize]) -> &'a mut [usize] {
        let mut inner = self.inner.lock().unwrap();

        if inner.wait_list.is_empty() {
            return &mut [];
        }

        if let Some(n) = inner.check_ready_list(ready) {
            &mut ready[..n]
        } else {
            &mut []
        }
    }
}

unsafe impl Sync for Select { }
unsafe impl Send for Select { }

struct Inner {
    wait_list: HashMap<usize, Entry>,

    ready_list: SortedVec<usize>,
    ready_list2: SortedVec<usize>,

    condvar: Arc<Condvar>,
}

impl Inner {
    fn new(condvar: Arc<Condvar>) -> Inner {
        Inner {
            wait_list: HashMap::new(),
            ready_list: SortedVec::new(),
            ready_list2: SortedVec::new(),
            condvar: condvar
        }
    }

    fn add_ready(&mut self, id: usize) -> bool {
        if !self.wait_list.contains_key(&id) {
            return false;
        }

        self.ready_list.insert(id);
        self.condvar.notify_one();

        true
    }

    fn going_away(&mut self, id: usize) -> bool {
        if self.wait_list.remove(&id).is_none() {
            return false;
        }

        self.ready_list.insert(id);
        self.condvar.notify_one();

        true
    }

    fn check_ready_list(&mut self, ready: &mut [usize]) -> Option<usize> {
        for id in self.ready_list.drain() {
            if self.wait_list[id].data.upgrade().map(|e| e.ready()).unwrap_or(false) {
                self.ready_list2.push(id);
            }
        }
        mem::swap(&mut self.ready_list, &mut self.ready_list2);

        match cmp::min(ready.len(), self.ready_list.len()) {
            0 => None,
            n => {
                for i in 0..n {
                    ready[i] = self.ready_list[i];
                }
                Some(n)
            }
        }
    }
}

unsafe impl Send for Inner { }

#[derive(Clone)]
struct Entry {
    data: WeakTrait<_Selectable+'static>,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Entry) -> bool {
        self.data.unique_id() == other.data.unique_id()
    }
}

impl Eq for Entry { }

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Entry) -> Option<Ordering> {
        self.data.unique_id().partial_cmp(&other.data.unique_id())
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Entry) -> Ordering {
        self.data.unique_id().cmp(&other.data.unique_id())
    }
}

impl<H: Hasher+Writer> Hash<H> for Entry {
    fn hash(&self, state: &mut H) {
        self.data.unique_id().hash(state);
    }
}

/// A structure stored by `Selectable` objects to interact with `Select` objects that want
/// to be notified when the `Selectable` object becomes ready.
pub struct WaitQueue {
    queue: Vec<Weak<Mutex<Inner>>>,
    id: usize,
}

impl WaitQueue {
    /// Creates a new `WaitQueue`. This function does not allocate.
    pub fn new() -> WaitQueue {
        WaitQueue {
            queue: vec!(),
            id: 0,
        }
    }

    /// Sets the `id` of the `Selectable` object containing the `WaitQueue`. This id must
    /// be the id returned by `Selectable::as_selectable().unique_id()`. This function
    /// must be called with the correct id before any other functions are called.
    pub fn set_id(&mut self, id: usize) {
        self.id = id;
    }

    /// Add a `Select` object to the `WaitQueue`. Returns the number of `Select` objects
    /// contained in the `WaitQueue` after this call.
    pub fn add(&mut self, load: Payload) -> usize {
        self.queue.push(load.data);
        self.queue.len()
    }

    /// Removes a `Select` object from the `WaitQueue`. Returns the number of `Select`
    /// objects contained in the `WaitQueue` after this call.
    pub fn remove(&mut self, id: usize) -> usize {
        if let Some(p) = self.queue.iter().position(|el| el.unique_id() == id) {
            self.queue.remove(p);
        }
        self.queue.len()
    }

    /// Notifies all `Select` objects contained in this `WaitQueue` that the `Selectable`
    /// object has become ready. Returns the number of `Select` objects contained in the
    /// `WaitQueue` after this call. This function might remove `Select` objects from the
    /// `WaitQueue`.
    pub fn notify(&mut self) -> usize {
        let mut i = 0;
        while i < self.queue.len() {
            let strong = match self.queue[i].upgrade() {
                Some(s) => s,
                _ => {
                    self.queue.swap_remove(i);
                    continue;
                },
            };
            let mut select = strong.lock().unwrap();
            select.add_ready(self.id);
            i += 1;
        }
        self.queue.len()
    }

    /// Removes all `Select` objects from this wait queue and signals them that the
    /// `Selectable` object will no longer be available. This happens automatically when
    /// the `WaitQueue` is dropped.
    pub fn clear(&mut self) {
        for el in self.queue.drain() {
            if let Some(strong) = el.upgrade() {
                let mut select = strong.lock().unwrap();
                select.going_away(self.id);
            }
        }
    }
}

impl Drop for WaitQueue {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Container passed from the `Select` object to a `WaitQueue`.
pub struct Payload {
    data: Weak<Mutex<Inner>>,
}
