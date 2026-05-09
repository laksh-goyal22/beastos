//! SMP (Symmetric Multiprocessing) support
//!
//! Boots Application Processors (APs) using the APIC INIT-SIPI sequence.

pub const MAX_CPUS: usize = 8;

/// Per-CPU data structure.
pub struct PerCpuData {
    pub id: u32,
    pub kernel_stack: u64,
    pub user_stack_saved: u64,
    pub pcid: u16,
    pub is_bsp: bool,
}

static mut CPU_DATA: [PerCpuData; MAX_CPUS] = {
    const INIT: PerCpuData = PerCpuData {
        id: 0, kernel_stack: 0, user_stack_saved: 0, pcid: 0, is_bsp: false,
    };
    [INIT; MAX_CPUS]
};

/// Initialize SMP — detect cores from ACPI MADT, boot APs.
pub fn init() {
    // TODO:
    // 1. Parse MADT for LAPIC entries
    // 2. Allocate per-CPU stacks and GDTs
    // 3. Send INIT-SIPI-SIPI to each AP
    // 4. APs jump to ap_entry and park on scheduler
}
