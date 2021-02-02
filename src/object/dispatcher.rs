use signals::Signal;
use nabi::{Result, Error};
use common::table::{Table, TableSlot};
use alloc::sync::Arc;
use spin::Mutex;
use super::Handle;
use sync::atomic::{Atomic, Ordering};
use core::any::{Any, TypeId};
use core::ops::Deref;
use object::wait_observer::WaitObserver;

unsafe impl Send for Context {}
unsafe impl Sync for Context {}

struct Context {
    signals: Atomic<Signal>,
    observers: Mutex<Table<*mut (dyn StateObserver)>>,
}

impl Context {
    fn new() -> Context {
        Context {
            signals: Atomic::new(Signal::empty()),
            observers: Mutex::new(Table::new()),
        }
    }

    fn signals(&self) -> Signal {
        self.signals.load(Ordering::Relaxed)
    }

    fn set_signals(&self, signals: Signal) {
        self.signals.store(signals, Ordering::Relaxed);
    }
}

struct DispatchInner<T: Dispatcher + ?Sized> {
    ctx: Context,
    dispatcher: T,
}

pub struct Dispatch<T: Dispatcher + ?Sized> {
    inner: Arc<DispatchInner<T>>,
}

impl<T> Dispatch<T>
where
    T: Dispatcher
{
    pub fn new(dispatcher: T) -> Dispatch<T> {
        Dispatch {
            inner: Arc::new(DispatchInner {
                ctx: Context::new(),
                dispatcher,
            }),
        }
    }
}

impl<T> Dispatch<T>
where
    T: Dispatcher + ?Sized
{
    pub unsafe fn add_observer(&self, observer: *mut (dyn StateObserver)) -> Option<TableSlot> {
        let initial_signals = self.ctx().signals();

        if (*observer).on_init(initial_signals) == ObserverResult::Remove {
            // don't insert it into the observer list
            return None;
        }

        let mut observers = self.ctx().observers.lock();
        Some(observers.allocate(observer))
    }

    pub fn remove_observer(&self, slot: TableSlot) -> Option<*mut (dyn StateObserver)> {
        let mut observers = self.ctx().observers.lock();
        observers.free(slot)
    }

    pub fn copy_ref(&self) -> Dispatch<T> {
        Dispatch {
            inner: Arc::clone(&self.inner),
        }
    }

    fn ctx(&self) -> &Context {
        &self.inner.ctx
    }

    pub fn signal(&self, set_signals: Signal, clear_signals: Signal) -> Result<()> {
        if !self.allows_observers() {
            return Err(Error::NOT_SUPPORTED);
        }

        let allowed_signals = self.allowed_user_signals();

        if !allowed_signals.contains(set_signals) || !allowed_signals.contains(clear_signals) {
            return Err(Error::INVALID_ARG);
        }

        self.update_state(set_signals, clear_signals);

        Ok(())
    }

    fn update_state(&self, set_signals: Signal, clear_signals: Signal) {
        debug_assert!(self.allows_observers());

        let ctx = self.ctx();

        let previous_signals = ctx.signals();
        let mut new_signals = previous_signals;
        new_signals.remove(clear_signals);
        new_signals.insert(set_signals);

        if previous_signals == new_signals {
            return;
        }

        ctx.set_signals(new_signals);

        let mut observers = ctx.observers.lock();

        for mut entry in observers.entries() {
            if ( unsafe { &mut **entry.get_mut() } ).on_state_change(new_signals) == ObserverResult::Remove {
                entry.remove();
            }
        }
    }
}
impl Dispatch<dyn Dispatcher> {
    pub fn cast<T: Dispatcher>(&self) -> Result<Dispatch<T>> {
        if self.inner.dispatcher.type_id() == TypeId::of::<T>() {
            use core::mem;

            let this = self.copy_ref();

            let ptr = &this as *const Dispatch<dyn Dispatcher> as *const Dispatch<T>;
            mem::forget(this);

            Ok(unsafe { ptr.read() })
        } else {
            Err(Error::WRONG_TYPE)
        }
    }
}

impl<T: Dispatcher + Sized> Dispatch<T> {
    pub fn upcast(self) -> Dispatch<dyn Dispatcher> {
        let inner = self.inner as Arc<DispatchInner<dyn Dispatcher>>;
        Dispatch {
            inner,
        }
    }
}

impl<T> Deref for Dispatch<T>
where
    T: Dispatcher + ?Sized
{
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner.dispatcher
    }
}

/// All handle objects must implement this trait.
/// Handle objects are refcounted.
pub trait Dispatcher: Any + Send + Sync {
    fn allowed_user_signals(&self) -> Signal {
        Signal::empty()
    }

    fn allows_observers(&self) -> bool { false }

    fn get_name(&self) -> Option<&str> { None }
    fn set_name(&self) -> Result<()> { Err(Error::NOT_SUPPORTED) }

    fn on_zero_handles(&self) {}
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ObserverResult {
    Keep,
    Remove,
}

pub struct LocalObserver<'local, 'dispatch, S: StateObserver + 'local> {
    slot: TableSlot,
    observer: &'local mut S,
    dispatch: &'dispatch Dispatch<dyn Dispatcher>,
}

impl<'local, 'dispatch, S: StateObserver + Send + Any> LocalObserver<'local, 'dispatch, S> {
    pub fn new(observer: &'local mut S, dispatch: &'dispatch mut Dispatch<dyn Dispatcher>) -> Option<LocalObserver<'local, 'dispatch, S>> {
        let slot = unsafe { dispatch.add_observer(observer as *mut _)? };

        Some(LocalObserver {
            slot,
            observer,
            dispatch,
        })
    }
}

impl<'local, 'dispatch> LocalObserver<'local, 'dispatch, WaitObserver> {
    pub fn wait(&self) {
        self.observer.wait();
    }
}

impl<'local, 'dispatch, S: StateObserver> Drop for LocalObserver<'local, 'dispatch, S> {
    fn drop(&mut self) {
        if let Some(observer_ptr) = self.dispatch.remove_observer(self.slot) {
            assert_eq!(observer_ptr, self.observer as *mut _);
        }
    }
}

pub trait StateObserver: Send {
    fn on_init(&mut self, initial_state: Signal) -> ObserverResult;
    fn on_state_change(&mut self, new_state: Signal) -> ObserverResult;
    fn on_destruction(&mut self, handle: &Handle<dyn Dispatcher>) -> ObserverResult;
    fn on_removal(&mut self) {}
}
