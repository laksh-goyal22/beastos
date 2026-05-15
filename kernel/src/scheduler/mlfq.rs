//! Multilevel Feedback Queue (MLFQ) Scheduler
//!
//! 6 priority levels with exponentially increasing time quanta.
//! Tasks are demoted when they exhaust their quantum, promoted when
//! they block on I/O.  A periodic priority boost prevents starvation.

use super::task::*;
use crate::kprintln;

/// Time quanta per MLFQ level (in PIT ticks, 10 ms each at 100 Hz).
const QUANTA: [u64; PRIORITY_LEVELS] = [
    1,  // Level 0:  10 ms  (interactive / real-time)
    2,  // Level 1:  20 ms
    4,  // Level 2:  40 ms
    8,  // Level 3:  80 ms
    16, // Level 4: 160 ms
    32, // Level 5: 320 ms  (background / batch)
];

/// Anti-starvation: boost all tasks to level 0 every N ticks.
const BOOST_INTERVAL: u64 = 500; // 5 seconds at 100 Hz

// ── Fixed-Size Circular Queue ────────────────────────────────────────────

/// Ring buffer of task IDs for one priority level.
pub struct TaskQueue {
    buf: [TaskId; MAX_TASKS],
    head: usize,
    tail: usize,
    len: usize,
}

impl TaskQueue {
    pub const fn new() -> Self {
        Self { buf: [NO_TASK; MAX_TASKS], head: 0, tail: 0, len: 0 }
    }

    pub fn push_back(&mut self, id: TaskId) {
        if self.len >= MAX_TASKS { return; }
        self.buf[self.tail] = id;
        self.tail = (self.tail + 1) % MAX_TASKS;
        self.len += 1;
    }

    pub fn push_front(&mut self, id: TaskId) {
        if self.len >= MAX_TASKS { return; }
        self.head = if self.head == 0 { MAX_TASKS - 1 } else { self.head - 1 };
        self.buf[self.head] = id;
        self.len += 1;
    }

    pub fn pop_front(&mut self) -> Option<TaskId> {
        if self.len == 0 { return None; }
        let id = self.buf[self.head];
        self.head = (self.head + 1) % MAX_TASKS;
        self.len -= 1;
        Some(id)
    }

    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// Remove a specific task ID (linear scan — fine for 64 tasks).
    pub fn remove(&mut self, id: TaskId) {
        let mut new = TaskQueue::new();
        while let Some(tid) = self.pop_front() {
            if tid != id { new.push_back(tid); }
        }
        *self = new;
    }
}

// ── MLFQ Scheduler ──────────────────────────────────────────────────────

/// Core MLFQ scheduler state.
pub struct Scheduler {
    /// Task storage (Option to allow empty slots).
    pub tasks: [Option<Task>; MAX_TASKS],
    /// One run queue per priority level.
    pub queues: [TaskQueue; PRIORITY_LEVELS],
    /// Index of the currently running task (NO_TASK if idle).
    pub current: TaskId,
    /// Global tick counter.
    pub tick_count: u64,
    /// Ticks until next priority boost.
    boost_countdown: u64,
    /// Next free task ID.
    pub next_id: usize,
    /// Whether the scheduler has been started.
    pub started: bool,
}

impl Scheduler {
    pub const fn new() -> Self {
        const NONE: Option<Task> = None;
        const EMPTY_Q: TaskQueue = TaskQueue::new();
        Self {
            tasks: [NONE; MAX_TASKS],
            queues: [EMPTY_Q; PRIORITY_LEVELS],
            current: NO_TASK,
            tick_count: 0,
            boost_countdown: BOOST_INTERVAL,
            next_id: 0,
            started: false,
        }
    }

    // ── Task Management ──────────────────────────────────────────────

    /// Spawn a new task. Returns its ID, or None if out of slots.
    pub fn spawn(
        &mut self,
        entry: extern "C" fn(),
        name: &str,
        priority: usize,
    ) -> Option<TaskId> {
        let slot = self.find_free_slot()?;
        let id = self.next_id;
        self.next_id += 1;

        let prio = priority.min(PRIORITY_LEVELS - 1);
        
        // Kernel tasks share the kernel page table
        let page_table = crate::arch::paging::read_cr3() & !0xFFF;
        
        let task = Task::new(id, entry, name, prio, page_table)?;
        self.tasks[slot] = Some(task);
        self.queues[prio].push_back(slot);

        kprintln!("    [SCHED] Spawned task {} '{}' at level {}", id, name, prio);
        Some(slot)
    }

    pub fn find_free_slot(&self) -> Option<usize> {
        self.tasks.iter().position(|t| t.is_none())
    }

    /// Mark a task as dead and remove it from queues.
    pub fn kill(&mut self, slot: usize) {
        if let Some(ref mut task) = self.tasks[slot] {
            task.state = TaskState::Dead;
            self.queues[task.priority].remove(slot);
        }
        // Don't free the slot yet — let cleanup handle it later.
    }

    // ── Scheduling ───────────────────────────────────────────────────

    /// Called on every PIT timer tick.  Returns true if a reschedule
    /// is needed (current task's quantum expired).
    pub fn tick(&mut self) -> bool {
        self.tick_count += 1;

        // Anti-starvation boost
        self.boost_countdown -= 1;
        if self.boost_countdown == 0 {
            self.boost_all();
            self.boost_countdown = BOOST_INTERVAL;
        }

        // Check current task's quantum
        if self.current == NO_TASK { return false; }
        if let Some(ref mut task) = self.tasks[self.current] {
            task.ticks_used += 1;
            let quantum = QUANTA[task.priority];
            if task.ticks_used >= quantum {
                // Quantum expired → demote and reschedule
                self.demote_current();
                return true;
            }
        }
        false
    }

    /// Pick the highest-priority ready task.
    /// Returns the slot index, or NO_TASK if nothing runnable.
    ///
    /// Uses a `while let` loop per level to skip stale (Blocked/Dead)
    /// entries without permanently losing them from the scheduler.
    pub fn pick_next(&mut self) -> TaskId {
        for level in 0..PRIORITY_LEVELS {
            while let Some(slot) = self.queues[level].pop_front() {
                if let Some(ref task) = self.tasks[slot] {
                    if task.state == TaskState::Ready {
                        return slot;
                    }
                }
                // Dead/blocked entry — already popped, skip it.
            }
        }
        NO_TASK
    }

    /// Prepare to switch away from current task (put it back in queue).
    pub fn requeue_current(&mut self) {
        if self.current == NO_TASK { return; }
        if let Some(ref mut task) = self.tasks[self.current] {
            if task.state == TaskState::Running {
                task.state = TaskState::Ready;
                self.queues[task.priority].push_back(self.current);
            }
        }
    }

    /// Demote current task one priority level (quantum exhausted).
    fn demote_current(&mut self) {
        if let Some(ref mut task) = self.tasks[self.current] {
            task.ticks_used = 0;
            if task.priority < PRIORITY_LEVELS - 1 {
                task.priority += 1;
            }
        }
    }

    /// SPSC-aware priority boosting — the key Beast OS optimisation.
    ///
    /// Instead of guessing "did it block on I/O?", we know the *exact*
    /// reason and can make perfect scheduling decisions at zero cost.
    pub fn boost_for_reason(&mut self, slot: TaskId, reason: BlockReason) {
        match reason {
            // Consumer starving → boost the *producer* immediately to fill the ring.
            // When the consumer is woken later, it will also be boosted via wake_task().
            BlockReason::SpscRingEmpty { producer_slot } => {
                if let Some(ref mut t) = self.tasks[producer_slot] {
                    let old_prio = t.priority;
                    t.priority = 0;
                    t.ticks_used = 0;

                    if t.state == TaskState::Ready && old_prio != 0 {
                        self.queues[old_prio].remove(producer_slot);
                        self.queues[0].push_front(producer_slot);
                    }
                }
                
                // Also boost the consumer (the 'slot' argument) so it's RT when woken.
                if let Some(ref mut t) = self.tasks[slot] {
                    t.priority = 0;
                    t.ticks_used = 0;
                    // No queue move here because it's currently Blocked (transitioning).
                }
            }
            // Producer blocked on full ring → boost the *consumer*, not producer.
            BlockReason::SpscRingFull { consumer_slot } => {
                if let Some(ref mut t) = self.tasks[consumer_slot] {
                    let old_prio = t.priority;
                    t.priority = 0;
                    t.ticks_used = 0;

                    // Only move in queues if it's currently Ready.
                    // If it's Running or Blocked, it's not in any queue.
                    if t.state == TaskState::Ready && old_prio != 0 {
                        self.queues[old_prio].remove(consumer_slot);
                        self.queues[0].push_front(consumer_slot); // push_front to prioritize drainage
                    }
                }
            }
            // Standard I/O block — gentle one-level boost
            BlockReason::FileRead | BlockReason::MutexContention => {
                if let Some(ref mut t) = self.tasks[slot] {
                    let old_prio = t.priority;
                    t.ticks_used = 0;
                    if t.priority > 0 { t.priority -= 1; }
                    let new_prio = t.priority;

                    if t.state == TaskState::Ready && old_prio != new_prio {
                        self.queues[old_prio].remove(slot);
                        self.queues[new_prio].push_back(slot);
                    }
                }
            }
            // Sleep / voluntary yield / waiting — no boost (intentional)
            BlockReason::Sleep | BlockReason::Yield | BlockReason::Waiting => {}
        }
    }

    /// Simple one-level promotion (convenience wrapper).
    pub fn promote(&mut self, slot: TaskId) {
        if let Some(ref mut task) = self.tasks[slot] {
            task.ticks_used = 0;
            if task.priority > 0 {
                task.priority -= 1;
            }
        }
    }

    /// Anti-starvation: move ALL ready tasks to level 0.
    fn boost_all(&mut self) {
        // Drain all queues
        for level in 1..PRIORITY_LEVELS {
            while let Some(slot) = self.queues[level].pop_front() {
                if let Some(ref mut task) = self.tasks[slot] {
                    task.priority = 0;
                    task.ticks_used = 0;
                }
                self.queues[0].push_back(slot);
            }
        }
    }
}
