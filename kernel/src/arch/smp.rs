//! SMP (Symmetric Multiprocessing) support
//!
//! Boots Application Processors (APs) using the APIC INIT-SIPI sequence.

pub const MAX_CPUS: usize = 8;

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, AtomicU16, Ordering};
use core::arch::asm;

/// Per-CPU data structure.
/// Offset 0: self_ptr (points to this structure)
/// Offset 8: kernel_stack
/// Offset 16: user_stack_saved
#[repr(C)]
pub struct PerCpuData {
    pub self_ptr: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack_saved: AtomicU64,
    pub id: AtomicU32,
    pub pcid: AtomicU16,
    pub is_bsp: AtomicBool,
}

static CPU_DATA: [PerCpuData; MAX_CPUS] = {
    const INIT: PerCpuData = PerCpuData {
        self_ptr: AtomicU64::new(0),
        kernel_stack: AtomicU64::new(0),
        user_stack_saved: AtomicU64::new(0),
        id: AtomicU32::new(0),
        pcid: AtomicU16::new(0),
        is_bsp: AtomicBool::new(false),
    };
    [INIT; MAX_CPUS]
};

/// Initialize per-CPU state for the current core.
pub fn init_percpu(id: u32, is_bsp: bool, kernel_stack: u64) {
    let cpu_data = &CPU_DATA[id as usize];
    cpu_data.id.store(id, Ordering::SeqCst);
    cpu_data.is_bsp.store(is_bsp, Ordering::SeqCst);
    cpu_data.kernel_stack.store(kernel_stack, Ordering::SeqCst);

    let ptr = cpu_data as *const PerCpuData as u64;
    cpu_data.self_ptr.store(ptr, Ordering::SeqCst);

    unsafe {
        crate::arch::asm::wrmsr(crate::arch::asm::MSR_GS_BASE, ptr);
        crate::arch::asm::wrmsr(crate::arch::asm::MSR_KERNEL_GS_BASE, ptr);
    }
}

/// Get the per-CPU data for the current core.
pub fn get_cpu_data() -> &'static PerCpuData {
    let ptr: u64;
    unsafe {
        asm!("mov {}, gs:[0]", out(reg) ptr, options(nostack, preserves_flags, readonly));
        &*(ptr as *const PerCpuData)
    }
}

/// Get the current CPU ID.
pub fn get_id() -> u32 {
    get_cpu_data().id.load(Ordering::SeqCst)
}

/// Check if the current CPU is the Bootstrap Processor.
pub fn is_bsp() -> bool {
    get_cpu_data().is_bsp.load(Ordering::SeqCst)
}

/// Initialize SMP — detect cores from ACPI MADT, boot APs.
pub fn init() {
    // TODO:
    // 1. Parse MADT for LAPIC entries
    // 2. Allocate per-CPU stacks and GDTs
    // 3. Send INIT-SIPI-SIPI to each AP
    // 4. APs jump to ap_entry and park on scheduler
}
