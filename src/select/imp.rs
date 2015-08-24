use std::collections::{HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, Condvar};
use std::cmp::{self, Ordering};
use std::time::{Duration};
use std::{mem};

use arc::{Arc, Weak, WeakTrait};
use sortedvec::{SortedVec};
use super::{Selectable, _Selectable};

/// Container for all targets being selected on.
pub struct Select<'a> {
    condvar: Arc<Condvar>,
    inner: Arc<Mutex<Inner<'a>>>,
}

impl<'a> Select<'a> {
    /// Creates a new `Select` object.
    pub fn new() -> Select<'a> {
        let condvar = Arc::new(Condvar::new());
        Select {
            condvar: condvar.clone(),
            inner: Arc::new(Mutex::new(Inner::new(condvar)))
        }
    }

    fn as_payload(&self) -> Payload<'a> {
        Payload { data: self.inner.downgrade() }
    }

    /// Adds a target to the select object.
    pub fn add<T: Selectable<'a>+'a>(&self, sel: &T) {
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
    pub fn remove<T: Selectable<'a>>(&self, sel: &T) -> bool {
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
    pub fn wait<'b>(&self, ready: &'b mut [usize]) -> &'b mut [usize] {
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

    /// Waits for any of the targets in the `Select` object to become ready. The semantics
    /// are as for the `wait` function except that
    ///
    /// - if `duration` is some duration, then it will sleep for at most `duration`, and
    /// - if `duration` is none, then it will only check if at the time of the call any
    ///   targets are ready and return immediately.
    ///
    /// # Return value
    ///
    /// Returns `None` if the timeout expired.
    pub fn wait_timeout<'b>(&self, ready: &'b mut [usize],
                            duration: Option<Duration>) -> Option<&'b mut [usize]> {
        let mut inner = self.inner.lock().unwrap();

        if inner.wait_list.is_empty() {
            return Some(&mut []);
        }

        if let Some(n) = inner.check_ready_list(ready) {
            return Some(&mut ready[..n]);
        }

        let duration = match duration {
            Some(d) => d,
            _ => return Some(&mut []),
        };

        let (inner, notified) = self.condvar.wait_timeout_with(inner, duration, |inner| {
            inner.unwrap().ready_list.len() > 0
        }).unwrap();

        if !notified.timed_out() {
            return None;
        }

        let min = cmp::min(ready.len(), inner.ready_list.len());
        for i in 0..min {
            ready[i] = inner.ready_list[i];
        }
        Some(&mut ready[..min])
    }
}

unsafe impl<'a> Sync for Select<'a> { }
unsafe impl<'a> Send for Select<'a> { }

struct Inner<'a> {
    wait_list: HashMap<usize, Entry<'a>>,

    ready_list: SortedVec<usize>,
    ready_list2: SortedVec<usize>,

    condvar: Arc<Condvar>,
}

impl<'a> Inner<'a> {
    fn new(condvar: Arc<Condvar>) -> Inner<'a> {
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
        let all = 0..self.ready_list.len();
        for id in self.ready_list.drain(all) {
            if let Some(target) = self.wait_list.get(&id) {
                if target.data.upgrade().map(|e| e.ready()).unwrap_or(false) {
                    self.ready_list2.push(id);
                }
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

unsafe impl<'a> Send for Inner<'a> { }

#[derive(Clone)]
struct Entry<'a> {
    data: WeakTrait<_Selectable<'a>+'a>,
}

impl<'a> PartialEq for Entry<'a> {
    fn eq(&self, other: &Entry<'a>) -> bool {
        self.data.unique_id() == other.data.unique_id()
    }
}

impl<'a> Eq for Entry<'a> { }

impl<'a> PartialOrd for Entry<'a> {
    fn partial_cmp(&self, other: &Entry<'a>) -> Option<Ordering> {
        self.data.unique_id().partial_cmp(&other.data.unique_id())
    }
}

impl<'a> Ord for Entry<'a> {
    fn cmp(&self, other: &Entry<'a>) -> Ordering {
        self.data.unique_id().cmp(&other.data.unique_id())
    }
}

impl<'a> Hash for Entry<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.data.unique_id().hash(state);
    }
}

/// A structure stored by `Selectable` objects to interact with `Select` objects that want
/// to be notified when the `Selectable` object becomes ready.
pub struct WaitQueue<'a> {
    queue: Vec<Weak<Mutex<Inner<'a>>>>,
    id: usize,
}

impl<'a> WaitQueue<'a> {
    /// Creates a new `WaitQueue`. This function does not allocate.
    pub fn new() -> WaitQueue<'a> {
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
    pub fn add(&mut self, load: Payload<'a>) -> usize {
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
        let all = 0..self.queue.len();
        for el in self.queue.drain(all) {
            if let Some(strong) = el.upgrade() {
                let mut select = strong.lock().unwrap();
                select.going_away(self.id);
            }
        }
    }
}

impl<'a> Drop for WaitQueue<'a> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Container passed from the `Select` object to a `WaitQueue`.
pub struct Payload<'a> {
    data: Weak<Mutex<Inner<'a>>>,
}
