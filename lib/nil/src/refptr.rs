use core::any::{Any, TypeId};
use core::marker::Unsize;
use core::ops::{Deref, CoerceUnsized};
use core::sync::atomic::{self, AtomicUsize, Ordering};
use core::ptr::NonNull;
use mem::Bin;
use nabi::Result;

/// All handle objects must implement this trait.
/// Handle objects are refcounted.
pub trait HandleRef: Any + Send + Sync {}

struct RefInner<T: ?Sized> {
    count: AtomicUsize,
    data: T,
}

/// Reference counted ptr for
/// ensuring `KernelObject` lifetimes.
#[repr(transparent)]
#[derive(Debug)]
pub struct Ref<T: ?Sized> {
    ptr: NonNull<RefInner<T>>,
}

unsafe impl<T: ?Sized + Sync + Send> Send for Ref<T> {}
unsafe impl<T: ?Sized + Sync + Send> Sync for Ref<T> {}

impl<T: ?Sized + Unsize<U>, U: ?Sized> CoerceUnsized<Ref<U>> for Ref<T> {}

impl<T> Ref<T> {
    pub fn new(data: T) -> Result<Ref<T>> {
        let bin = Bin::new(RefInner {
            count: AtomicUsize::new(1),
            data,
        })?;

        Ok(Ref {
            ptr: bin.into_nonnull(),
        })
    }

    pub unsafe fn dangling() -> Ref<T> {
        Ref {
            ptr: NonNull::dangling(),
        }
    }
}

impl<T: ?Sized> Ref<T> {
    #[inline]
    fn inner(&self) -> &RefInner<T> {
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    pub fn ptr_eq(&self, other: &Ref<T>) -> bool {
        self.ptr == other.ptr
    }

    fn copy_ref(&self) -> Self {
        self.inc_ref();
        
        Self {
            ptr: self.ptr,
        }
    }

    fn inc_ref(&self) -> usize {
        self.inner().count.fetch_add(1, Ordering::Relaxed)
    }

    fn dec_ref(&self) -> usize {
        self.inner().count.fetch_sub(1, Ordering::Release)
    }
}

impl Ref<HandleRef> {
    pub fn cast<T: HandleRef>(&self) -> Option<Ref<T>> {
        let self_: &HandleRef = &**self;
        if self_.get_type_id() == TypeId::of::<T>() {
            let ptr: NonNull<RefInner<T>> = self.ptr.cast();
            let refptr = Ref { ptr, };
            refptr.inc_ref();
            Some(refptr)
        } else {
            None
        }
    }

    pub fn cast_ref<T: HandleRef>(&self) -> Option<&T> {
        self.cast()
            .map(|refptr: Ref<T>| unsafe { &(&*refptr.ptr.as_ptr()).data })
    }
}

impl<T: ?Sized> Clone for Ref<T> {
    fn clone(&self) -> Ref<T> {
        self.copy_ref()
    }
}

unsafe impl<#[may_dangle] T: ?Sized> Drop for Ref<T> {
    fn drop(&mut self) {
        if self.dec_ref() != 1 {
            return;
        }

        atomic::fence(Ordering::Acquire);

        let ptr = self.ptr;

        unsafe {
            let _ = Bin::from_nonnull(ptr);

            atomic::fence(Ordering::Acquire);
        }     
    }
}

impl<T: ?Sized + PartialEq> PartialEq for Ref<T> {
    #[inline]
    fn eq(&self, other: &Ref<T>) -> bool {
        let self_: &T = &*self;
        let other_: &T = &*other;
        self_ == other_
    }
}

impl<T: ?Sized> Deref for Ref<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.inner().data
    }
}
