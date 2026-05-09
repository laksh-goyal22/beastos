//! Synchronization primitives for Beast OS
//!
//! Includes a spinlock-based Mutex that is interrupt-safe.

use spin::{Mutex, MutexGuard};
use x86_64::instructions::interrupts;

/// A spinlock-based Mutex that disables interrupts while held.
///
/// This prevents deadlocks if an interrupt handler attempts to acquire
/// the same lock as the code it interrupted.
pub struct Spinlock<T> {
    inner: Mutex<T>,
}

impl<T> Spinlock<T> {
    /// Create a new Spinlock wrapping the given data.
    pub const fn new(data: T) -> Self {
        Self {
            inner: Mutex::new(data),
        }
    }

    /// Acquire the lock, disabling interrupts.
    ///
    /// Returns a guard that will re-enable interrupts (if they were enabled)
    /// when dropped.
    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        let interrupts_enabled = interrupts::are_enabled();
        if interrupts_enabled {
            interrupts::disable();
        }
        SpinlockGuard {
            inner: self.inner.lock(),
            interrupts_enabled,
        }
    }
}

// Safety: Spinlock is safe to share between threads.
unsafe impl<T: Send> Sync for Spinlock<T> {}
unsafe impl<T: Send> Send for Spinlock<T> {}

/// A guard that releases the spinlock and restores interrupt state when dropped.
pub struct SpinlockGuard<'a, T> {
    inner: MutexGuard<'a, T>,
    interrupts_enabled: bool,
}

impl<'a, T> core::ops::Deref for SpinlockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.inner
    }
}

impl<'a, T> core::ops::DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.inner
    }
}

impl<'a, T> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        if self.interrupts_enabled {
            interrupts::enable();
        }
    }
}
