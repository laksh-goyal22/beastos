use crate::scheduler::task::{BlockReason, TaskId, NO_TASK};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::cell::UnsafeCell;

/// SPSC Ring with automatic scheduler boosting
/// This is the core innovation of Beast OS
pub struct BeastSPSCRing<T: Copy, const N: usize> {
    buffer: UnsafeCell<[T; N]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    consumer_task: AtomicUsize,
    waiting: AtomicBool,
}

unsafe impl<T: Copy, const N: usize> Sync for BeastSPSCRing<T, N> {}
unsafe impl<T: Copy, const N: usize> Send for BeastSPSCRing<T, N> {}

impl<T: Copy + Default, const N: usize> BeastSPSCRing<T, N> {
    pub const fn new() -> Self {
        Self {
            buffer: UnsafeCell::new(unsafe { core::mem::zeroed() }),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            consumer_task: AtomicUsize::new(NO_TASK),
            waiting: AtomicBool::new(false),
        }
    }
    
    /// Set the consumer task ID (called once during setup)
    pub fn set_consumer(&self, task_id: TaskId) {
        self.consumer_task.store(task_id, Ordering::Release);
    }
    
    /// Producer: enqueue data, boost consumer if waiting
    pub fn enqueue(&self, value: T) -> Result<(), T> {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Relaxed);
        
        if tail.wrapping_sub(head) >= N {
            return Err(value);
        }
        
        let idx = tail & (N - 1);
        unsafe { (*self.buffer.get())[idx] = value; }
        
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        
        // CRITICAL: Boost consumer if it was waiting
        if self.waiting.swap(false, Ordering::Acquire) {
            let consumer = self.consumer_task.load(Ordering::Acquire);
            if consumer != NO_TASK {
                // Boost to real-time priority using standard wake_task which handles boosting
                crate::scheduler::wake_task(consumer);
            }
        }
        
        Ok(())
    }
    
    /// Consumer: try to dequeue (non-blocking)
    pub fn try_dequeue(&self) -> Option<T> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        
        if head == tail {
            // Mark that consumer is waiting (for producer to boost)
            self.waiting.store(true, Ordering::Release);
            return None;
        }
        
        let idx = head & (N - 1);
        let value = unsafe { (*self.buffer.get())[idx] };
        
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }
    
    /// Consumer: blocking dequeue (calls scheduler to yield)
    pub fn wait_dequeue(&self) -> T {
        loop {
            if let Some(value) = self.try_dequeue() {
                return value;
            }
            // Tell scheduler we're blocked on this ring
            crate::scheduler::block_current(BlockReason::SpscRingEmpty { producer_slot: NO_TASK });
        }
    }
}

// For interrupt events specifically:
use crate::syscall::driver_syscalls::InterruptEvent;
pub type InterruptRing = BeastSPSCRing<InterruptEvent, 256>;
