/// Types able to be transferred across thread boundaries.
///
/// If possible you should implement `std::marker::Send` instead of this trait. This trait
/// exists because `Send` cannot be implemented for `*mut T` and `*const T`.
pub unsafe trait Sendable { }

unsafe impl<T: Send+?Sized> Sendable for T { }
