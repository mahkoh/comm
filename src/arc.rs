//! Fork of the `arc` module in the rust stdlib.
//!
//! Only the changes are documented here. See the stdlib for the rest.
//!
//! In contrast to the stdlib `arc` module, this one also supports `Arc` objects that
//! contain trait objects.
//!
//! ### Example
//!
//! ```
//! use comm::arc::{Arc, ArcTrait};
//!
//! struct X {
//!     x: u8
//! }
//!
//! trait Y {
//!     fn f(&self) -> u8;
//! }
//!
//! impl Y for X {
//!     fn f(&self) -> u8 {
//!         self.x
//!     }
//! }
//!
//! let arc_trait: ArcTrait<Y> = unsafe {
//!     let arc = Arc::new(X { x: 3 });
//!     arc.as_trait(&*arc as &(Y+'static))
//! };
//!
//! assert_eq!(arc_trait.f(), 3);
//! ```

use std::sync::atomic::Ordering::{Relaxed, Release, Acquire, SeqCst};
use std::sync::{atomic};
use std::{fmt, mem, ptr};
use std::mem::{min_align_of, size_of};
use core::nonzero::{NonZero};
use std::ops::{Deref};
use std::rt::heap::{deallocate};
use std::raw::{TraitObject};
use std::marker::{PhantomData};

#[unsafe_no_drop_flag]
pub struct Arc<T> {
    _ptr: NonZero<*mut ArcInner<T>>,
}

unsafe impl<T: Sync+Send> Send for Arc<T> { }
unsafe impl<T: Sync+Send> Sync for Arc<T> { }

#[unsafe_no_drop_flag]
pub struct Weak<T> {
    _ptr: NonZero<*mut ArcInner<T>>,
}

unsafe impl<T: Sync+Send> Send for Weak<T> { }
unsafe impl<T: Sync+Send> Sync for Weak<T> { }

/// An atomically reference counted wrapper of a trait object.
#[unsafe_no_drop_flag]
pub struct ArcTrait<Trait: ?Sized> {
    /// Size of ArcInner<X>
    _size: usize,
    /// Alignment of ArcInner<X>
    _alignment: usize,
    /// Destructor of X
    _destructor: fn(*mut ()),
    /// &Trait
    _trait: TraitObject,

    _ptr: NonZero<*mut ArcInner<u8>>,

    _marker: PhantomData<Trait>,
}

unsafe impl<Trait: ?Sized+Sync+Send> Send for ArcTrait<Trait> {}
unsafe impl<Trait: ?Sized+Sync+Send> Sync for ArcTrait<Trait> {}

/// A weak pointer to an `ArcTrait`.
#[unsafe_no_drop_flag]
pub struct WeakTrait<Trait: ?Sized> {
    /// Size of ArcInner<X>
    _size: usize,
    /// Alignment of ArcInner<X>
    _alignment: usize,
    /// Destructor of X
    _destructor: fn(*mut ()),
    /// &Trait
    _trait: TraitObject,

    _ptr: NonZero<*mut ArcInner<u8>>,

    _marker: PhantomData<Trait>,
}

unsafe impl<Trait: ?Sized+Sync+Send> Send for WeakTrait<Trait> {}
unsafe impl<Trait: ?Sized+Sync+Send> Sync for WeakTrait<Trait> {}

#[repr(C)]
struct ArcInner<T> {
    strong: atomic::AtomicUsize,
    weak: atomic::AtomicUsize,
    data: T,
}

unsafe impl<T: Sync+Send> Send for ArcInner<T> {}
unsafe impl<T: Sync+Send> Sync for ArcInner<T> {}

fn ptr_drop<T>(data: *mut ()) {
    unsafe { ptr::read(data as *mut T); }
}

impl<T> Arc<T> {
    #[inline]
    pub fn new(data: T) -> Arc<T> {
        // Start the weak pointer count as 1 which is the weak pointer that's
        // held by all the strong pointers (kinda), see std/rc.rs for more info
        let x = box ArcInner {
            strong: atomic::AtomicUsize::new(1),
            weak: atomic::AtomicUsize::new(1),
            data: data,
        };
        Arc { _ptr: unsafe { NonZero::new(mem::transmute(x)) } }
    }

    pub fn downgrade(&self) -> Weak<T> {
        // See the clone() impl for why this is relaxed
        self.inner().weak.fetch_add(1, Relaxed);
        Weak { _ptr: self._ptr }
    }

    #[inline]
    fn inner(&self) -> &ArcInner<T> {
        // This unsafety is ok because while this arc is alive we're guaranteed that the
        // inner pointer is valid. Furthermore, we know that the `ArcInner` structure
        // itself is `Sync` because the inner data is `Sync` as well, so we're ok loaning
        // out an immutable pointer to these contents.
        unsafe { &**self._ptr }
    }

    /// Creates an ArcTrait from an Arc. `t` must be a trait object created by calling
    /// `&*self as &Trait`. Otherwise the behavior is undefined.
    pub unsafe fn as_trait<Trait: ?Sized>(&self, t: &Trait) -> ArcTrait<Trait> {
        assert!(mem::size_of::<&Trait>() == mem::size_of::<TraitObject>());

        let _trait = ptr::read(&t as *const _ as *const TraitObject);
        assert!(_trait.data as usize == &self.inner().data as *const _ as usize);

        self.inner().strong.fetch_add(1, Relaxed);

        ArcTrait {
            _size: mem::size_of::<ArcInner<T>>(),
            _alignment: mem::min_align_of::<ArcInner<T>>(),
            _destructor: ptr_drop::<T>,
            _trait: _trait,

            _ptr: mem::transmute(self._ptr),

            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn weak_count(&self) -> usize {
        self.inner().weak.load(SeqCst) - 1
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        self.inner().strong.load(SeqCst)
    }

    /// Returns a unique id shared among all clones of this `Arc`.
    pub fn unique_id(&self) -> usize {
        *self._ptr as usize
    }
}

impl<T> Clone for Arc<T> {
    #[inline]
    fn clone(&self) -> Arc<T> {
        // Using a relaxed ordering is alright here, as knowledge of the original
        // reference prevents other threads from erroneously deleting the object.
        //
        // As explained in the [Boost documentation][1], Increasing the reference counter
        // can always be done with memory_order_relaxed: New references to an object can
        // only be formed from an existing reference, and passing an existing reference
        // from one thread to another must already provide any required synchronization.
        //
        // [1]: (www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html)
        self.inner().strong.fetch_add(1, Relaxed);
        Arc { _ptr: self._ptr }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner().data
    }
}

#[unsafe_destructor]
impl<T> Drop for Arc<T> {
    fn drop(&mut self) {
        // This structure has #[unsafe_no_drop_flag], so this drop glue may run more than
        // once (but it is guaranteed to be zeroed after the first if it's run more than
        // once)
        let ptr = *self._ptr;
        if ptr.is_null() { return }

        // If we return here and another thread doesn't, then all of our modifications of
        // the inner data must be made visible before the destructor below runs.
        // Therefore we use release here and ...
        if self.inner().strong.fetch_sub(1, Release) != 1 { return }

        // ... acquire here so that the dropping thread runs only after all modifications
        // are visible.
        atomic::fence(Acquire);

        // Destroy the data at this time, even though we may not free the box allocation
        // itself (there may still be weak pointers lying around).
        unsafe { drop(ptr::read(&self.inner().data)); }

        if self.inner().weak.fetch_sub(1, Release) == 1 {
            atomic::fence(Acquire);
            unsafe { deallocate(ptr as *mut u8, size_of::<ArcInner<T>>(),
                                min_align_of::<ArcInner<T>>()) }
        }
    }
}

impl<T> Weak<T> {
    pub fn upgrade(&self) -> Option<Arc<T>> {
        // We use a CAS loop to increment the strong count instead of a fetch_add because
        // once the count hits 0 is must never be above 0.
        let inner = self.inner();
        loop {
            let n = inner.strong.load(SeqCst);
            if n == 0 { return None }
            let old = inner.strong.compare_and_swap(n, n + 1, SeqCst);
            if old == n { return Some(Arc { _ptr: self._ptr }) }
        }
    }

    #[inline]
    pub fn weak_count(&self) -> usize {
        self.inner().weak.load(SeqCst) - 1
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        self.inner().strong.load(SeqCst)
    }

    #[inline]
    fn inner(&self) -> &ArcInner<T> {
        // See comments above for why this is "safe"
        unsafe { &**self._ptr }
    }

    /// Returns a unique id shared among all clones of this `Arc`.
    pub fn unique_id(&self) -> usize {
        *self._ptr as usize
    }
}

impl<T> Clone for Weak<T> {
    #[inline]
    fn clone(&self) -> Weak<T> {
        // See comments in Arc::clone() for why this is relaxed
        self.inner().weak.fetch_add(1, Relaxed);
        Weak { _ptr: self._ptr }
    }
}

#[unsafe_destructor]
impl<T> Drop for Weak<T> {
    fn drop(&mut self) {
        let ptr = *self._ptr;

        // see comments above for why this check is here
        if ptr.is_null() { return }

        // If we find out that we were the last weak pointer, then its time to deallocate
        // the data entirely. See the discussion in Arc::drop() about the memory orderings
        if self.inner().weak.fetch_sub(1, Release) == 1 {
            atomic::fence(Acquire);
            unsafe { deallocate(ptr as *mut u8, size_of::<ArcInner<T>>(),
                                min_align_of::<ArcInner<T>>()) }
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Arc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<Trait: ?Sized> ArcTrait<Trait> {
    pub fn downgrade(&self) -> WeakTrait<Trait> {
        // See the clone() impl for why this is relaxed
        self.inner().weak.fetch_add(1, Relaxed);
        WeakTrait {
            _size: self._size,
            _alignment: self._alignment,
            _destructor: self._destructor,
            _trait: self._trait,

            _ptr: self._ptr,

            _marker: PhantomData,
        }
    }

    #[inline]
    fn inner(&self) -> &ArcInner<u8> {
        unsafe { &**self._ptr }
    }

    #[inline]
    pub fn weak_count(&self) -> usize {
        self.inner().weak.load(SeqCst) - 1
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        self.inner().strong.load(SeqCst)
    }

    /// Returns a unique id shared among all clones of this `Arc`.
    pub fn unique_id(&self) -> usize {
        *self._ptr as usize
    }
}

impl<Trait: ?Sized> Clone for ArcTrait<Trait> {
    #[inline]
    fn clone(&self) -> ArcTrait<Trait> {
        self.inner().strong.fetch_add(1, Relaxed);
        ArcTrait {
            _size: self._size,
            _alignment: self._alignment,
            _destructor: self._destructor,
            _trait: self._trait,

            _ptr: self._ptr,

            _marker: PhantomData,
        }
    }
}

impl<Trait: ?Sized> Deref for ArcTrait<Trait> {
    type Target = Trait;

    #[inline]
    fn deref(&self) -> &Trait {
        unsafe { ptr::read(&self._trait as *const _ as *const _) }
    }
}

#[unsafe_destructor]
impl<Trait: ?Sized> Drop for ArcTrait<Trait> {
    fn drop(&mut self) {
        // This structure has #[unsafe_no_drop_flag], so this drop glue may run more than
        // once (but it is guaranteed to be zeroed after the first if it's run more than
        // once)
        let ptr = *self._ptr;
        if ptr.is_null() { return }

        // If we return here and another thread doesn't, then all of our modifications of
        // the inner data must be made visible before the destructor below runs.
        // Therefore we use release here and ...
        if self.inner().strong.fetch_sub(1, Release) != 1 { return }

        // ... acquire here so that the dropping thread runs only after all modifications
        // are visible.
        atomic::fence(Acquire);

        // Destroy the data at this time, even though we may not free the box allocation
        // itself (there may still be weak pointers lying around).
        (self._destructor)(self._trait.data);

        if self.inner().weak.fetch_sub(1, Release) == 1 {
            atomic::fence(Acquire);
            unsafe { deallocate(ptr as *mut u8, self._size, self._alignment); }
        }
    }
}

impl<Trait: ?Sized> WeakTrait<Trait> {
    pub fn upgrade(&self) -> Option<ArcTrait<Trait>> {
        // We use a CAS loop to increment the strong count instead of a fetch_add because
        // once the count hits 0 is must never be above 0.
        let inner = self.inner();
        loop {
            let n = inner.strong.load(SeqCst);
            if n == 0 { return None }
            let old = inner.strong.compare_and_swap(n, n + 1, SeqCst);
            if old == n {
                let at = ArcTrait {
                    _size: self._size,
                    _alignment: self._alignment,
                    _destructor: self._destructor,
                    _trait: self._trait,

                    _ptr: self._ptr,

                    _marker: PhantomData,
                };
                return Some(at);
            }
        }
    }

    #[inline]
    pub fn weak_count(&self) -> usize {
        self.inner().weak.load(SeqCst) - 1
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        self.inner().strong.load(SeqCst)
    }

    #[inline]
    fn inner(&self) -> &ArcInner<u8> {
        // See comments above for why this is "safe"
        unsafe { &**self._ptr }
    }

    /// Returns a unique id shared among all clones of this `Arc`.
    pub fn unique_id(&self) -> usize {
        *self._ptr as usize
    }
}

impl<Trait: ?Sized> Clone for WeakTrait<Trait> {
    #[inline]
    fn clone(&self) -> WeakTrait<Trait> {
        // See comments in Arc::clone() for why this is relaxed
        self.inner().weak.fetch_add(1, Relaxed);
        WeakTrait {
            _size: self._size,
            _alignment: self._alignment,
            _destructor: self._destructor,
            _trait: self._trait,

            _ptr: self._ptr,

            _marker: PhantomData,
        }
    }
}

#[unsafe_destructor]
impl<Trait: ?Sized> Drop for WeakTrait<Trait> {
    fn drop(&mut self) {
        let ptr = *self._ptr;

        // see comments above for why this check is here
        if ptr.is_null() { return }

        // If we find out that we were the last weak pointer, then its time to deallocate
        // the data entirely. See the discussion in Arc::drop() about the memory orderings
        if self.inner().weak.fetch_sub(1, Release) == 1 {
            atomic::fence(Acquire);
            unsafe { deallocate(ptr as *mut u8, self._size, self._alignment); }
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Arc, ArcTrait};

    struct X {
        x: u8
    }

    trait Y {
        fn f(&self) -> u8;
    }

    impl Y for X {
        fn f(&self) -> u8 {
            self.x
        }
    }

    #[test]
    fn test1() {
        let arc_trait: ArcTrait<Y> = unsafe {
            let arc = Arc::new(X { x: 3 });
            arc.as_trait(&*arc as &(Y+'static))
        };

        assert_eq!(arc_trait.f(), 3);
    }

    #[test]
    fn test2() {
        let arc_trait: ArcTrait<Y> = unsafe {
            let arc = Arc::new(X { x: 3 });
            arc.as_trait(&*arc as &(Y+'static))
        };
        let weak = arc_trait.downgrade();
        let strong = weak.upgrade().unwrap();

        assert_eq!(strong.f(), 3);
    }

    #[test]
    fn test3() {
        let arc_trait: ArcTrait<Y> = unsafe {
            let arc = Arc::new(X { x: 3 });
            arc.as_trait(&*arc as &(Y+'static))
        };
        let weak = arc_trait.downgrade();
        drop(arc_trait);
        assert!(weak.upgrade().is_none());
    }
}
