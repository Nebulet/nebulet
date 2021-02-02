use nabi::{Result, Error, HandleRights};
use super::dispatcher::{Dispatch, Dispatcher};
use core::marker::PhantomData;
use core::ops::Deref;

/// A Handle represents an atomically reference-counted object with specfic rights.
/// Handles can be duplicated if they have the `HandleRights::DUPLICATE` right.
pub struct Handle<T: Dispatcher + ?Sized> {
    /// Reference-counted ptr to the stored object.
    dispatch: Dispatch<T>,
    /// This handle's access rights to the `Ref<T>`.
    rights: HandleRights,
}

impl<T: Dispatcher + ?Sized> Handle<T>
{
    pub fn new(dispatch: Dispatch<T>, rights: HandleRights) -> Handle<T> {
        Handle {
            dispatch,
            rights,
        }
    }
    
    pub fn duplicate(&self, new_rights: HandleRights) -> Option<Self> {
        if self.rights.contains(new_rights | HandleRights::DUPLICATE) {
            Some(Handle {
                dispatch: self.dispatch.copy_ref(),
                rights: new_rights,
            })
        } else {
            None
        }
    }

    pub fn check_rights(&self, rights: HandleRights) -> Result<&Self> {
        if self.rights.contains(rights) {
            Ok(self)
        } else {
            Err(Error::ACCESS_DENIED)
        }
    }

    pub fn rights(&self) -> HandleRights {
        self.rights
    }

    pub fn dispatcher(&self) -> &Dispatch<T> {
        &self.dispatch
    }
}

impl<T> Handle<T>
where
    T: Dispatcher + Sized
{
    pub fn upcast(self) -> Handle<dyn Dispatcher> {
        Handle {
            dispatch: self.dispatch.upcast(),
            rights: self.rights,
        }
    }
}

impl Handle<dyn Dispatcher> {
    pub fn cast<T: Dispatcher>(&self) -> Result<Handle<T>> {
        let dispatch = self.dispatch.cast()?;

        Ok(Handle {
            dispatch,
            rights: self.rights,
        })
    }
}

impl<T> Deref for Handle<T>
where
    T: Dispatcher + ?Sized
{
    type Target = Dispatch<T>;
    fn deref(&self) -> &Dispatch<T> {
        &self.dispatch
    }
}

#[repr(transparent)]
pub struct UserHandle<T: Dispatcher + ?Sized>(u32, PhantomData<T>);

impl<T> UserHandle<T>
where
    T: Dispatcher + ?Sized
{
    #[inline]
    pub fn new(index: u32) -> UserHandle<T> {
        UserHandle(index, PhantomData)
    }

    #[inline]
    pub fn inner(&self) -> u32 {
        self.0
    }
}
