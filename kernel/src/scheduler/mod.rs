//! Beast OS Scheduler — MLFQ with SPSC-Aware Boosting
//!
//! Public API for the kernel scheduler subsystem.
//!
//! - `init()`       — create idle task, start scheduling
//! - `spawn()`      — add a new task
//! - `yield_now()`  — cooperative context switch
//! - `tick()`       — called from PIT timer ISR
//! - `exit_current()` — terminate running task

pub mod task;
pub mod mlfq;
pub mod switch;

use task::*;
use mlfq::Scheduler;
use crate::kprintln;
use crate::sync::Spinlock;
use core::sync::atomic::{AtomicBool, Ordering};

/// Global scheduler instance (spinlock-protected).
static SCHEDULER: Spinlock<Scheduler> = Spinlock::new(Scheduler::new());

/// Flag: scheduler is active and context switches are allowed.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Flag: timer ISR sets this when a reschedule is needed.
/// Checked after returning from interrupt to avoid switching mid-ISR.
static NEED_RESCHEDULE: AtomicBool = AtomicBool::new(false);

// ── Public API ───────────────────────────────────────────────────────────

/// Initialize the scheduler and create the idle task.
///
/// Called from `kmain()` after memory subsystem is ready.
pub fn init() {
    let mut sched = SCHEDULER.lock();

    // Slot 0: idle task (lowest priority — always runnable)
    let slot = sched.spawn(idle_task, "idle", PRIORITY_LEVELS - 1);
    if let Some(s) = slot {
        sched.current = s;
        if let Some(ref mut t) = sched.tasks[s] {
            t.state = TaskState::Running;
        }
    }

    sched.started = true;
    kprintln!("  [SCHED] MLFQ scheduler initialized ({} levels, boost every {}s)",
        PRIORITY_LEVELS, 5);
}

/// Spawn a new task at the given MLFQ priority level.
pub fn spawn(entry: extern "C" fn(), name: &str, priority: usize) -> Option<TaskId> {
    SCHEDULER.lock().spawn(entry, name, priority)
}

/// Start the scheduler — switches into the first ready task.
///
/// This function never returns to the caller. The caller's context
/// is saved as the "bootstrap" context and only resumed if all
/// tasks die (which shouldn't happen — idle task is immortal).
pub fn run() -> ! {
    ACTIVE.store(true, Ordering::SeqCst);
    kprintln!("  [SCHED] Scheduler running");

    // The bootstrap context — we switch away from this and never return.
    let mut bootstrap_ctx = Context::empty();

    let first_rsp: *const u64;
    {
        let sched = SCHEDULER.lock();
        if sched.current == NO_TASK {
            panic!("[SCHED] No tasks to run!");
        }
        let task = sched.tasks[sched.current].as_ref().unwrap();
        first_rsp = &task.context.rsp as *const u64;
    }

    unsafe {
        switch::context_switch(
            &mut bootstrap_ctx.rsp as *mut u64,
            first_rsp,
        );
    }

    // Should never reach here
    unreachable!("[SCHED] Bootstrap context resumed — this is a bug");
}

/// Called from the PIT timer interrupt handler.
///
/// Increments tick counters, checks quantum expiry, and sets the
/// `NEED_RESCHEDULE` flag if the current task should be preempted.
pub fn tick() {
    if !ACTIVE.load(Ordering::Relaxed) { return; }

    let needs = SCHEDULER.lock().tick();
    if needs {
        NEED_RESCHEDULE.store(true, Ordering::SeqCst);
    }
}

/// Perform a context switch if one is pending.
///
/// Called after returning from an interrupt (in the timer handler
/// epilogue) or voluntarily via `yield_now()`.
pub fn reschedule() {
    if !ACTIVE.load(Ordering::Relaxed) { return; }
    NEED_RESCHEDULE.store(false, Ordering::SeqCst);

    // Disable interrupts during the switch to prevent re-entrancy
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }

    let (old_rsp_ptr, new_rsp_ptr) = {
        let mut sched = SCHEDULER.lock();

        // Put current task back in its queue
        sched.requeue_current();

        // Pick the next task
        let next = sched.pick_next();
        if next == NO_TASK || next == sched.current {
            // Nothing to switch to — re-enable interrupts and return
            unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
            return;
        }

        let old_slot = sched.current;
        sched.current = next;

        if let Some(ref mut t) = sched.tasks[next] {
            t.state = TaskState::Running;
            t.ticks_used = 0;
        }

        // Get raw pointers to rsp fields (safe: tasks array is static)
        let old_ptr = &mut sched.tasks[old_slot]
            .as_mut().unwrap().context.rsp as *mut u64;
        let new_ptr = &sched.tasks[next]
            .as_ref().unwrap().context.rsp as *const u64;

        (old_ptr, new_ptr)
        // Lock dropped here — new task can acquire it
    };

    unsafe {
        switch::context_switch(old_rsp_ptr, new_rsp_ptr);
        // We return here when switched BACK to this task
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Cooperative yield — voluntarily give up the CPU.
pub fn yield_now() {
    NEED_RESCHEDULE.store(true, Ordering::SeqCst);
    reschedule();
}

/// Terminate the currently running task.
pub fn exit_current() -> ! {
    {
        let mut sched = SCHEDULER.lock();
        let slot = sched.current;
        sched.kill(slot);
    }
    // Force a reschedule — we're dead, so pick another task
    NEED_RESCHEDULE.store(true, Ordering::SeqCst);
    reschedule();
    unreachable!("[SCHED] Dead task resumed");
}

/// Check and handle pending reschedule (called from ISR epilogue).
pub fn check_reschedule() {
    if NEED_RESCHEDULE.load(Ordering::Relaxed) {
        reschedule();
    }
}

// ── IPC Integration: Block / Wake ────────────────────────────────────────

/// Block the currently running task with a typed reason.
///
/// The task is marked `Blocked` and removed from its run queue.
/// A reschedule is forced so another task runs immediately.
///
/// The task will not be scheduled again until `wake_task()` is called.
///
/// # Interrupt Safety
/// Interrupts are disabled before acquiring the SCHEDULER lock to prevent
/// deadlock with the PIT timer ISR (which calls `tick()` → `SCHEDULER.lock()`).
/// `reschedule()` handles re-enabling interrupts via `sti` after the context switch.
pub fn block_current(reason: task::BlockReason) {
    // Disable interrupts — prevents timer ISR from deadlocking on SCHEDULER lock
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }

    {
        let mut sched = SCHEDULER.lock();
        let slot = sched.current;
        if slot == task::NO_TASK {
            unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
            return;
        }

        // Extract priority first to avoid double mutable borrow
        let (_priority, should_yield) = match sched.tasks[slot] {
            Some(ref mut t) => {
                if t.awake_pending {
                    // We received a wakeup while preparing to block.
                    // Clear the flag and return — do NOT block.
                    t.awake_pending = false;
                    (t.priority, false)
                } else {
                    t.state = task::TaskState::Blocked;
                    (t.priority, true)
                }
            }
            None => {
                drop(sched);
                unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
                return;
            }
        };

        if should_yield {
            kprintln!("    [SCHED] Task {} blocked: {:?}", slot, reason);

            // SPSC-aware cross-boost: if we (producer) are blocking because
            // the ring is full, boost the *consumer* to drain it faster.
            if let task::BlockReason::SpscRingFull { consumer_slot } = reason {
                if consumer_slot != task::NO_TASK {
                    sched.boost_for_reason(consumer_slot, reason);
                }
            }

            drop(sched);
            yield_now();
        } else {
            drop(sched);
            unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        }
    }
}

/// Wake a blocked task and apply SPSC-aware priority boosting.
///
/// Transitions the task from `Blocked` → `Ready`, applies
/// `boost_for_reason()`, and enqueues it at its (possibly boosted)
/// priority level.
///
/// Called from `ipc::channel` when:
/// - Producer pushes data → consumer was blocked on empty ring
/// - Consumer pops data → producer was blocked on full ring
///
/// # Interrupt Safety
/// Interrupts are disabled to prevent deadlock with the PIT timer ISR.
/// After the lock is released, `NEED_RESCHEDULE` is set so the boosted
/// task gets CPU at the next timer tick.
pub fn wake_task(slot: task::TaskId, reason: task::BlockReason) {
    // Disable interrupts — prevents timer ISR from deadlocking on SCHEDULER lock
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }

    {
        let mut sched = SCHEDULER.lock();

        if let Some(ref mut t) = sched.tasks[slot] {
            if t.state != task::TaskState::Blocked {
                // Task is already awake (Running or Ready).
                // Set awake_pending to handle the race where the task is ABOUT to block.
                t.awake_pending = true;
                
                // Boost it so it processes the new data faster.
                sched.boost_for_reason(slot, reason);
                
                drop(sched);
                unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
                return;
            }

            // Task was blocked. Clear any pending awake flag just in case.
            t.awake_pending = false;

            // SPSC-aware boost BEFORE setting Ready
            sched.boost_for_reason(slot, reason);

            // Now re-read priority (may have changed from boost)
            if let Some(ref mut t) = sched.tasks[slot] {
                t.state = task::TaskState::Ready;
                t.ticks_used = 0;
                let prio = t.priority;
                sched.queues[prio].push_back(slot);
                kprintln!("    [SCHED] Task {} woken → level {}", slot, prio);
            }
        }
    }

    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }

    // Signal that a higher-priority task may now be runnable.
    // The next timer tick will trigger the actual preemption.
    NEED_RESCHEDULE.store(true, Ordering::SeqCst);
}

/// Get the slot index of the currently running task.
///
/// # Interrupt Safety
/// Brief cli/sti to prevent deadlock with timer ISR.
pub fn current_slot() -> task::TaskId {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }
    let id = SCHEDULER.lock().current;
    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    id
}

// ── Trampoline & Idle Task ───────────────────────────────────────────────

/// Called if a task's entry function returns. Terminates the task.
pub extern "C" fn task_exit_trampoline() {
    exit_current();
}

/// The idle task — runs when nothing else is ready.
///
/// `sti; hlt` enables interrupts and immediately halts the CPU.
/// When an interrupt fires, execution resumes after `hlt`, we yield,
/// and the scheduler picks a real task if one became ready.
extern "C" fn idle_task() {
    loop {
        unsafe { core::arch::asm!("sti; hlt", options(nomem, nostack)); }
        yield_now();
    }
}
