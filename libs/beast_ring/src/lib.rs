#![no_std]

//! Beast OS SPSC (Single-Producer Single-Consumer) Lock-Free Ring Buffer
//!
//! Zero-copy IPC channel using cache-line-padded atomic indices.
//! Target latency: ~50ns per message transfer.
//!
//! # Design
//! - Power-of-2 capacity for branchless modulo (bitwise AND)
//! - Cache-line padding prevents false sharing between producer/consumer
//! - No locks, no CAS loops — pure SeqCst loads/stores
//! - Messages are fixed-size slots (configurable via const generics)

use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::UnsafeCell;

/// Cache line size for x86_64.
const CACHE_LINE: usize = 64;

/// A cache-line-padded atomic counter to prevent false sharing.
#[repr(C)]
struct PaddedAtomic {
    value: AtomicUsize,
    _pad: [u8; CACHE_LINE - core::mem::size_of::<AtomicUsize>()],
}

impl PaddedAtomic {
    const fn new(val: usize) -> Self {
        Self {
            value: AtomicUsize::new(val),
            _pad: [0u8; CACHE_LINE - core::mem::size_of::<AtomicUsize>()],
        }
    }
}

/// SPSC Ring Buffer with fixed-size message slots.
///
/// # Type Parameters
/// - `T`: Message type (must be Copy for zero-copy semantics)
/// - `N`: Capacity (MUST be a power of 2)
///
/// # Example
/// ```
/// let ring: SpscRing<u64, 1024> = SpscRing::new();
/// ring.try_push(42);
/// let val = ring.try_pop();
/// ```
#[repr(C)]
pub struct SpscRing<T: Copy, const N: usize> {
    /// Producer write index (only written by producer).
    head: PaddedAtomic,
    /// Consumer read index (only written by consumer).
    tail: PaddedAtomic,
    /// Ring buffer storage.
    buffer: UnsafeCell<[T; N]>,
}

// Safety: SpscRing is safe to share between exactly one producer and one consumer.
unsafe impl<T: Copy, const N: usize> Sync for SpscRing<T, N> {}
unsafe impl<T: Copy, const N: usize> Send for SpscRing<T, N> {}

impl<T: Copy + Default, const N: usize> SpscRing<T, N> {
    /// Create a new ring buffer. `N` MUST be a power of 2.
    pub const fn new() -> Self {
        // Compile-time check: N must be power of 2
        assert!(N > 0 && (N & (N - 1)) == 0, "SpscRing capacity must be power of 2");

        Self {
            head: PaddedAtomic::new(0),
            tail: PaddedAtomic::new(0),
            buffer: UnsafeCell::new(unsafe { core::mem::zeroed() }),
        }
    }

    /// Attempt to push a value into the ring.
    /// Returns `Err(value)` if the ring is full.
    pub fn try_push(&self, value: T) -> Result<(), T> {
        let head = self.head.value.load(Ordering::Relaxed);
        let tail = self.tail.value.load(Ordering::Acquire);

        // Full when head is one slot behind tail (modular)
        if head.wrapping_sub(tail) >= N {
            return Err(value);
        }

        // Write to slot (branchless modulo via bitmask)
        let index = head & (N - 1);
        unsafe {
            (*self.buffer.get())[index] = value;
        }

        // Publish the write
        self.head.value.store(head.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Attempt to pop a value from the ring.
    /// Returns `None` if the ring is empty.
    pub fn try_pop(&self) -> Option<T> {
        let tail = self.tail.value.load(Ordering::Relaxed);
        let head = self.head.value.load(Ordering::Acquire);

        // Empty when head == tail
        if tail == head {
            return None;
        }

        // Read from slot
        let index = tail & (N - 1);
        let value = unsafe { (*self.buffer.get())[index] };

        // Publish the read
        self.tail.value.store(tail.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Check if the ring is empty.
    pub fn is_empty(&self) -> bool {
        let head = self.head.value.load(Ordering::Acquire);
        let tail = self.tail.value.load(Ordering::Acquire);
        head == tail
    }

    /// Check if the ring is full.
    pub fn is_full(&self) -> bool {
        let head = self.head.value.load(Ordering::Acquire);
        let tail = self.tail.value.load(Ordering::Acquire);
        head.wrapping_sub(tail) >= N
    }

    /// Number of items currently in the ring.
    pub fn len(&self) -> usize {
        let head = self.head.value.load(Ordering::Acquire);
        let tail = self.tail.value.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    /// Maximum capacity.
    pub const fn capacity(&self) -> usize {
        N
    }
}

/// A fixed-size IPC message for kernel channels.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct IpcMessage {
    /// Message type/opcode.
    pub msg_type: u32,
    /// Flags (e.g., reply-expected, capability-transfer).
    pub flags: u32,
    /// Inline payload (up to 48 bytes).
    pub payload: [u8; 48],
}

impl Default for IpcMessage {
    fn default() -> Self {
        Self {
            msg_type: 0,
            flags: 0,
            payload: [0u8; 48],
        }
    }
}

/// Pre-defined ring size for IPC channels.
pub type IpcRing = SpscRing<IpcMessage, 256>;
