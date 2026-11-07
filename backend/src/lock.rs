//! Scope: Locking a mutex poisoned by a panic; nothing else.
//!
//! A mutex poisoned by a panic (a job's work, `job::start`) holds data that is still usable:
//! the server and the other jobs go on with it instead of panicking in turn.
use std::sync::{Mutex, MutexGuard, PoisonError};

pub trait LockAnyway<T> {
    /// [`Mutex::lock`], taking the data of a poisoned mutex as it is.
    fn lock_anyway(&self) -> MutexGuard<'_, T>;
}

impl<T> LockAnyway<T> for Mutex<T> {
    fn lock_anyway(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
