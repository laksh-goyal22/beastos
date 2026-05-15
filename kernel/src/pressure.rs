use core::sync::atomic::{AtomicU32, Ordering};

const EWMA_ALPHA_NUM: u64 = 1;
const EWMA_ALPHA_DEN: u64 = 8;

static PRESSURE: AtomicU32 = AtomicU32::new(0);

// Oscillation detection: track direction changes
const OSC_HISTORY_SIZE: usize = 4;
static OSC_HISTORY: [AtomicU32; OSC_HISTORY_SIZE] = [
    AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0),
];
static OSC_IDX: AtomicU32 = AtomicU32::new(0);
static OSC_DIR_CHANGES: AtomicU32 = AtomicU32::new(0);
static BATCHING_DISABLED: AtomicU32 = AtomicU32::new(0); // 1 = disabled temporarily

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Phase {
    Idle = 0,
    Light = 1,
    Normal = 2,
    Heavy = 3,
    Storm = 4,
}

static CURRENT_PHASE: AtomicU32 = AtomicU32::new(0);

pub fn update() {
    let task_p = crate::scheduler::task_pressure() as u64;
    let mem_p = memory_pressure() as u64;
    let raw = ((task_p + mem_p) / 2) as u32;
    let prev = PRESSURE.load(Ordering::Relaxed) as u64;
    let smoothed = (prev * (EWMA_ALPHA_DEN - EWMA_ALPHA_NUM) + raw as u64 * EWMA_ALPHA_NUM) / EWMA_ALPHA_DEN;
    let new_val = smoothed.min(u32::MAX as u64) as u32;
    PRESSURE.store(new_val, Ordering::Relaxed);

    // Oscillation detection
    update_oscillation(new_val);
    update_phase(new_val);
}

fn update_oscillation(pressure: u32) {
    let idx = OSC_IDX.fetch_add(1, Ordering::Relaxed) as usize % OSC_HISTORY_SIZE;
    let prev = OSC_HISTORY[idx].swap(pressure, Ordering::Relaxed);
    if prev == 0 { return; } // Skip first iteration
    let dir = if pressure > prev { 1u32 } else if pressure < prev { 2u32 } else { 0u32 };
    if dir != 0 {
        let last_dir = OSC_HISTORY[(idx + OSC_HISTORY_SIZE - 1) % OSC_HISTORY_SIZE]
            .load(Ordering::Relaxed);
        if last_dir != 0 && last_dir != dir {
            // Direction changed — potential oscillation
            let changes = OSC_DIR_CHANGES.fetch_add(1, Ordering::Relaxed) + 1;
            if changes >= 4 {
                BATCHING_DISABLED.store(1, Ordering::Relaxed);
            }
        }
    }
}

/// Check if batching is disabled due to oscillation.
pub fn batching_disabled() -> bool {
    BATCHING_DISABLED.load(Ordering::Relaxed) != 0
}

/// Re-enable batching after oscillation settles.
pub fn reset_oscillation() {
    BATCHING_DISABLED.store(0, Ordering::Relaxed);
    OSC_DIR_CHANGES.store(0, Ordering::Relaxed);
}

fn update_phase(pressure: u32) {
    let p = pressure as f64 / u32::MAX as f64;
    let cur = CURRENT_PHASE.load(Ordering::Relaxed);
    let new = match cur {
        0 if p > 0.25 => 1,
        1 if p > 0.45 => 2,
        1 if p < 0.15 => 0,
        2 if p > 0.65 => 3,
        2 if p < 0.35 => 1,
        3 if p > 0.85 => 4,
        3 if p < 0.55 => 2,
        4 if p < 0.75 => 3,
        _ => cur,
    };
    CURRENT_PHASE.store(new, Ordering::Relaxed);
}

pub fn current_phase() -> Phase {
    match CURRENT_PHASE.load(Ordering::Relaxed) {
        0 => Phase::Idle,
        1 => Phase::Light,
        2 => Phase::Normal,
        3 => Phase::Heavy,
        4 => Phase::Storm,
        _ => Phase::Normal,
    }
}

pub fn recommended_batch_size() -> u32 {
    match current_phase() {
        Phase::Idle => 32,
        Phase::Light => 64,
        Phase::Normal => 128,
        Phase::Heavy => 256,
        Phase::Storm => 256,
    }
}

pub fn get() -> f64 {
    PRESSURE.load(Ordering::Relaxed) as f64 / u32::MAX as f64
}

pub fn get_raw() -> u32 {
    PRESSURE.load(Ordering::Relaxed)
}

fn memory_pressure() -> u32 {
    let total = crate::memory::pmm::total_count() as u64;
    let free = crate::memory::pmm::free_count() as u64;
    let used = total.saturating_sub(free);
    if total == 0 { return 0; }
    (used.saturating_mul(u32::MAX as u64) / total) as u32
}
