//! Task Control Block and CPU Context
//!
//! Each task has a unique ID, priority level, saved CPU context,
//! and its own kernel stack for context switching.

use crate::memory::{pmm, vmm};

/// Page size (4 KiB).
const PAGE_SIZE: usize = 4096;

/// Kernel stack: 4 pages = 16 KiB per task.
pub const TASK_STACK_PAGES: usize = 4;
pub const TASK_STACK_SIZE: usize = TASK_STACK_PAGES * PAGE_SIZE;

/// Maximum concurrent tasks.
pub const MAX_TASKS: usize = 64;

/// MLFQ priority levels (0 = highest, 5 = background).
pub const PRIORITY_LEVELS: usize = 6;

/// Task identifier.
pub type TaskId = usize;

/// Sentinel value: no task selected.
pub const NO_TASK: TaskId = usize::MAX;

/// Task lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Dead,
}

/// Why a task blocked — enables SPSC-aware priority boosting.
///
/// Standard MLFQ guesses based on "did it block on I/O?".
/// Beast OS knows the *exact* reason because of SPSC ring integration,
/// allowing 10x better scheduling decisions with zero overhead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// Consumer waiting on an empty SPSC ring → boost the *producer* to fill it.
    SpscRingEmpty { producer_slot: TaskId },
    /// Producer waiting on a full SPSC ring → boost the *consumer* instead.
    SpscRingFull { consumer_slot: TaskId },
    /// Waiting on a mutex/spinlock.
    MutexContention,
    /// Voluntary sleep (timer-based).
    Sleep,
    /// Disk / file I/O.
    FileRead,
    /// Yielded voluntarily (cooperative).
    Yield,
}

/// Saved CPU context — only the stack pointer.
///
/// Callee-saved registers (rbx, rbp, r12–r15) live on the task's
/// own stack and are pushed/popped by the `context_switch` stub.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub rsp: u64,
}

impl Context {
    pub const fn empty() -> Self {
        Self { rsp: 0 }
    }
}

/// Task Control Block.
pub struct Task {
    pub id: TaskId,
    pub state: TaskState,
    /// Current MLFQ level (0 = highest priority).
    pub priority: usize,
    /// Saved priority before inheritance boost.
    pub base_priority: usize,
    /// Ticks consumed at current priority level.
    pub ticks_used: u64,
    /// CPU context (saved stack pointer).
    pub context: Context,
    /// Physical base of the allocated kernel stack.
    pub stack_phys: u64,
    /// Virtual base of the kernel stack (via HHDM).
    pub stack_virt: u64,
    /// Debug name (fixed buffer, no alloc).
    pub name: [u8; 32],
    pub name_len: usize,
    /// Flag to prevent lost wakeups if wake_task is called before block_current.
    pub awake_pending: bool,
}

impl Task {
    /// Allocate a kernel stack and prepare the task for first dispatch.
    ///
    /// The stack is set up so that `context_switch` will "return" into
    /// `entry` the first time the task is scheduled.
    pub fn new(
        id: TaskId,
        entry: extern "C" fn(),
        name_str: &str,
        priority: usize,
    ) -> Option<Self> {
        // Allocate physical pages for the kernel stack
        let phys = pmm::alloc_contiguous(TASK_STACK_PAGES)?;
        let virt = vmm::phys_to_virt(phys);

        // Zero the stack
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, TASK_STACK_SIZE);
        }

        // Build the initial stack frame so context_switch can "return" into
        // the task_start_trampoline, which does `sti; ret` into the real entry.
        //
        // Layout (growing downward):
        //   [stack_top - 8 ]  task_exit_trampoline  ← if entry() returns
        //   [stack_top - 16]  entry                 ← trampoline's `ret` target
        //   [stack_top - 24]  task_start_trampoline ← context_switch's `ret` target
        //   [stack_top - 32]  rbp  = 0
        //   [stack_top - 40]  rbx  = 0
        //   [stack_top - 48]  r12  = 0
        //   [stack_top - 56]  r13  = 0
        //   [stack_top - 64]  r14  = 0
        //   [stack_top - 72]  r15  = 0    ← initial rsp
        let stack_top = virt + TASK_STACK_SIZE as u64;
        let sp = stack_top as *mut u64;
        unsafe {
            sp.offset(-1).write(super::task_exit_trampoline as *const () as u64);         // if entry() returns
            sp.offset(-2).write(entry as *const () as u64);                               // trampoline ret → entry
            sp.offset(-3).write(super::switch::task_start_trampoline as *const () as u64); // context_switch ret → trampoline
            sp.offset(-4).write(0); // rbp
            sp.offset(-5).write(0); // rbx
            sp.offset(-6).write(0); // r12
            sp.offset(-7).write(0); // r13
            sp.offset(-8).write(0); // r14
            sp.offset(-9).write(0); // r15
        }

        let mut name_buf = [0u8; 32];
        let len = name_str.len().min(32);
        name_buf[..len].copy_from_slice(&name_str.as_bytes()[..len]);

        Some(Self {
            id,
            state: TaskState::Ready,
            priority,
            base_priority: priority,
            ticks_used: 0,
            context: Context { rsp: stack_top - 72 },
            stack_phys: phys,
            stack_virt: virt,
            name: name_buf,
            name_len: len,
            awake_pending: false,
        })
    }

    /// Debug name as &str.
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}
