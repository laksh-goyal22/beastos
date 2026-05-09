//! Scheduler-Integrated SPSC Channel
//!
//! A `Channel<T, N>` wraps a `beast_ring::SpscRing<T, N>` with two
//! scheduler-aware operations:
//!
//! - **`send()`**: Pushes a value. If the consumer was blocked waiting,
//!   it is woken with `BlockReason::SpscRingEmpty` → boosted to MLFQ
//!   level 0 (highest priority) so it drains the ring immediately.
//!
//! - **`recv()`**: Pops a value. If the ring is empty, the caller is
//!   blocked with `BlockReason::SpscRingEmpty` and yields the CPU.
//!   The scheduler won't run this task again until `send()` wakes it.
//!
//! # Zero-Overhead Fast Path
//!
//! When the ring is non-empty, `recv()` is a single atomic load + copy.
//! No scheduler interaction, no syscall, no lock. This is what makes
//! Beast OS IPC competitive with L4 and seL4.
//!
//! # Why This Matters
//!
//! Standard MLFQ schedulers guess: "this task blocked, maybe it was I/O,
//! let's give it a small boost." Beast OS **knows**: "this task blocked
//! because the SPSC ring was empty — boost it to level 0 immediately
//! when data arrives so it drains the ring before the producer stalls."

use beast_ring::SpscRing;
use crate::scheduler::task::{TaskId, BlockReason, NO_TASK};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A scheduler-integrated SPSC channel.
///
/// `T` must be `Copy + Default` (required by `SpscRing`).
/// `N` must be a power of 2.
///
/// # Ownership Model
/// - One producer task calls `send()`.
/// - One consumer task calls `recv()` / `try_recv()`.
/// - The channel itself is shared (e.g., via `&'static` or a kernel registry).
pub struct Channel<T: Copy + Default, const N: usize> {
    /// The underlying lock-free ring buffer.
    ring: SpscRing<T, N>,

    /// Task slot of the consumer (set on first `recv()`).
    /// Used by `send()` to wake the right task.
    consumer_slot: AtomicUsize,

    /// Task slot of the producer (set on first `send()`).
    /// Used by `recv()` to wake the producer if ring was full.
    producer_slot: AtomicUsize,

    /// True when the consumer is blocked waiting for data.
    /// Checked by `send()` to decide whether to wake.
    consumer_waiting: AtomicBool,

    /// True when the producer is blocked because the ring is full.
    /// Checked by `recv()` to decide whether to wake.
    producer_waiting: AtomicBool,
}

// Safety: Channel is designed for exactly 1 producer + 1 consumer.
// The AtomicBool flags ensure correct synchronization.
unsafe impl<T: Copy + Default, const N: usize> Sync for Channel<T, N> {}
unsafe impl<T: Copy + Default, const N: usize> Send for Channel<T, N> {}

impl<T: Copy + Default, const N: usize> Channel<T, N> {
    /// Create a new channel. `N` must be a power of 2.
    pub const fn new() -> Self {
        Self {
            ring: SpscRing::new(),
            consumer_slot: AtomicUsize::new(NO_TASK),
            producer_slot: AtomicUsize::new(NO_TASK),
            consumer_waiting: AtomicBool::new(false),
            producer_waiting: AtomicBool::new(false),
        }
    }

    // ── Producer API ────────────────────────────────────────────────────

    /// Register the calling task as the producer.
    /// Must be called once before `send()`.
    pub fn set_producer(&self, slot: TaskId) {
        self.producer_slot.store(slot, Ordering::Release);
    }

    /// Non-blocking send. Returns `Err(value)` if the ring is full.
    pub fn try_send(&self, value: T) -> Result<(), T> {
        match self.ring.try_push(value) {
            Ok(()) => {
                // Fast path: data pushed. Check if consumer needs waking.
                self.maybe_wake_consumer();
                Ok(())
            }
            Err(v) => Err(v),
        }
    }

    /// Blocking send. If the ring is full, the producer is suspended
    /// until the consumer drains at least one slot.
    pub fn send(&self, value: T) {
        loop {
            if self.try_send(value).is_ok() {
                return;
            }

            // Signal that we're about to block.
            self.producer_waiting.store(true, Ordering::Release);

            // Re-check the ring AFTER setting the flag.
            if self.try_send(value).is_ok() {
                // Space appeared — clear the flag and return.
                self.producer_waiting.store(false, Ordering::Release);
                return;
            }

            // Ring is truly full and flag is set — block.
            // The consumer's recv() will see producer_waiting=true and wake us.
            let consumer = self.consumer_slot.load(Ordering::Acquire);
            crate::scheduler::block_current(
                BlockReason::SpscRingFull { consumer_slot: consumer }
            );
        }
    }

    // ── Consumer API ────────────────────────────────────────────────────

    /// Register the calling task as the consumer.
    /// Must be called once before `recv()`.
    pub fn set_consumer(&self, slot: TaskId) {
        self.consumer_slot.store(slot, Ordering::Release);
    }

    /// Non-blocking receive. Returns `None` if the ring is empty.
    pub fn try_recv(&self) -> Option<T> {
        match self.ring.try_pop() {
            Some(v) => {
                // Consumed a slot — check if producer needs waking.
                self.maybe_wake_producer();
                Some(v)
            }
            None => None,
        }
    }

    /// Blocking receive. If the ring is empty, the consumer is
    /// suspended until the producer pushes data.
    ///
    /// When data arrives via `send()`, the consumer is woken with
    /// `BlockReason::SpscRingEmpty` → MLFQ level 0 boost. This means
    /// the consumer runs *immediately* after the producer pushes,
    /// minimising latency and preventing ring overflow.
    ///
    /// # Wake Safety
    /// The `consumer_waiting` flag is set before a final re-check of
    /// the ring to close the race window between `try_recv()` returning
    /// `None` and the producer calling `maybe_wake_consumer()`.
    pub fn recv(&self) -> T {
        loop {
            if let Some(v) = self.try_recv() {
                return v;
            }
            // Signal that we're about to block.
            self.consumer_waiting.store(true, Ordering::Release);

            // Re-check the ring AFTER setting the flag.
            // If the producer pushed between our try_recv() and the flag set,
            // we'd miss the wake. This double-check closes that window.
            if let Some(v) = self.try_recv() {
                // Data appeared — clear the flag and return.
                self.consumer_waiting.store(false, Ordering::Release);
                return v;
            }

            // Ring is truly empty and flag is set — block.
            // The producer's send() will see consumer_waiting=true and wake us.
            crate::scheduler::block_current(
                BlockReason::SpscRingEmpty { producer_slot: self.producer_slot.load(Ordering::Acquire) }
            );
        }
    }

    // ── Query API ───────────────────────────────────────────────────────

    /// Number of items currently in the ring.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    // ── Internal Wake Logic ─────────────────────────────────────────────

    /// If the consumer was blocked waiting for data, wake it with a
    /// priority boost. This is the key scheduler integration point.
    fn maybe_wake_consumer(&self) {
        if self.consumer_waiting.swap(false, Ordering::AcqRel) {
            let consumer = self.consumer_slot.load(Ordering::Acquire);
            if consumer != NO_TASK {
                crate::scheduler::wake_task(
                    consumer,
                    BlockReason::SpscRingEmpty { producer_slot: self.producer_slot.load(Ordering::Acquire) }
                );
            }
        }
    }

    /// If the producer was blocked because the ring was full, wake it
    /// with a gentle boost (one MLFQ level up).
    fn maybe_wake_producer(&self) {
        if self.producer_waiting.swap(false, Ordering::AcqRel) {
            let producer = self.producer_slot.load(Ordering::Acquire);
            if producer != NO_TASK {
                // Wake producer — boost is handled by the consumer_slot
                // reference in SpscRingFull, which boosts the *consumer*
                // (us). This is correct: we (consumer) got CPU time to
                // drain, so producer can now push again.
                crate::scheduler::wake_task(
                    producer,
                    BlockReason::SpscRingFull { consumer_slot: self.consumer_slot.load(Ordering::Acquire) }
                );
            }
        }
    }
}
