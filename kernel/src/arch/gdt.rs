//! Global Descriptor Table (GDT) for x86_64
//!
//! Beast OS uses a per-CPU GDT with TSS for Ring 3 → Ring 0 transitions.

use core::arch::asm;
use core::mem::size_of;
use core::ptr::addr_of;
use crate::kprintln;
use crate::sync::Spinlock;

/// GDT entry (8 bytes for standard segments).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    flags_limit_hi: u8,
    base_hi: u8,
}

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const USER_DATA_SELECTOR: u16 = 0x18 | 3;
pub const USER_CODE_SELECTOR: u16 = 0x20 | 3;

impl GdtEntry {
    pub const fn null() -> Self {
        Self { limit_low: 0, base_low: 0, base_mid: 0, access: 0, flags_limit_hi: 0, base_hi: 0 }
    }

    /// Kernel code segment (Ring 0, 64-bit long mode).
    pub const fn kernel_code() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0x9A,
            flags_limit_hi: 0xAF,
            base_hi: 0,
        }
    }

    /// Kernel data segment (Ring 0).
    pub const fn kernel_data() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0x92,
            flags_limit_hi: 0xCF,
            base_hi: 0,
        }
    }

    /// User data segment (Ring 3) — must come before user code for SYSCALL/SYSRET.
    pub const fn user_data() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0xF2,
            flags_limit_hi: 0xCF,
            base_hi: 0,
        }
    }

    /// User code segment (Ring 3, 64-bit).
    pub const fn user_code() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0xFA,
            flags_limit_hi: 0xAF,
            base_hi: 0,
        }
    }
}

/// Task State Segment — ring transitions + IST stacks.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Tss {
    _reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    _reserved1: u64,
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    pub iopb_offset: u16,
}

impl Tss {
    pub const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp0: 0, rsp1: 0, rsp2: 0,
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iopb_offset: size_of::<Tss>() as u16,
        }
    }
}

/// The actual GDT layout:
/// 0x00: Null
/// 0x08: Kernel Code (selector 0x08)
/// 0x10: Kernel Data (selector 0x10)
/// 0x18: User Data   (selector 0x1B with RPL=3)
/// 0x20: User Code   (selector 0x23 with RPL=3)
/// 0x28: TSS Low     (16-byte system descriptor)
/// 0x30: TSS High
#[repr(C, align(16))]
struct GdtTable {
    entries: [u64; 7],
}

static GDT: Spinlock<GdtTable> = Spinlock::new(GdtTable { entries: [0; 7] });
static TSS: Spinlock<Tss> = Spinlock::new(Tss::new());
static LDT_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Kernel-mode interrupt stack (16KB, page-aligned).
/// Force into .bss so the page table maps it writable — Rust places
/// zero-initialized statics in .rodata when there's no interior mutability.
#[repr(C, align(4096))]
struct KernelStack([u8; 16384]);
#[link_section = ".bss"]
static KERNEL_STACK: KernelStack = KernelStack([0; 16384]);

/// Double-fault IST stack (8KB).
#[repr(C, align(4096))]
struct IstStack([u8; 8192]);
#[link_section = ".bss"]
static IST_STACK: IstStack = IstStack([0; 8192]);

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Initialize the GDT with TSS for the BSP (Bootstrap Processor).
pub fn init() {
    let mut tss = TSS.lock();
    let mut gdt = GDT.lock();

    // Set up TSS with kernel stacks
    let kernel_stack_top = addr_of!(KERNEL_STACK) as u64 + 16384;
    tss.rsp0 = kernel_stack_top;
    tss.ist[0] = addr_of!(IST_STACK) as u64 + 8192;

    // Build GDT entries
    gdt.entries[0] = 0;
    gdt.entries[1] = gdt_entry_to_u64(GdtEntry::kernel_code());
    gdt.entries[2] = gdt_entry_to_u64(GdtEntry::kernel_data());
    gdt.entries[3] = gdt_entry_to_u64(GdtEntry::user_data());
    gdt.entries[4] = gdt_entry_to_u64(GdtEntry::user_code());

    // TSS descriptor (16 bytes = 2 entries)
    let tss_base = (&*tss as *const Tss) as u64;
    let tss_limit = (size_of::<Tss>() - 1) as u64;
    gdt.entries[5] = tss_descriptor_low(tss_base, tss_limit);
    gdt.entries[6] = tss_descriptor_high(tss_base);
    // LDT entry 7 is filled later in enable_ldt() (after memory init)

    unsafe {
        // Load GDT
        let gdt_ptr = GdtPointer {
            limit: (core::mem::size_of::<GdtTable>() - 1) as u16,
            base: (&*gdt as *const GdtTable) as u64,
        };
        asm!("lgdt [{}]", in(reg) &gdt_ptr, options(nostack));

        // Reload segment registers
        // CS: far return trick
        asm!(
            "pushq $0x08",           // kernel code selector
            "leaq 2f(%rip), {tmp}",
            "pushq {tmp}",
            "lretq",
            "2:",
            tmp = out(reg) _,
            options(att_syntax)
        );

        // DS, SS, ES, FS = kernel data (0x10)
        // GS is handled separately via MSR
        asm!(
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "xor ax, ax",
            "mov fs, ax",
            out("ax") _,
        );

        // Load TSS (selector = 0x28)
        asm!("ltr ax", in("ax") 0x28u16, options(nostack));

        // ============================================================
        // CRITICAL: Set up GS base for per-CPU data
        // ============================================================
        
        // Get pointer to CPU 0's per-CPU data structure
        let cpu0_data = crate::arch::smp::get_cpu_data_ptr(0) as u64;
        
        // IA32_GS_BASE (MSR 0xC0000100) - User GS base
        // Used by swapgs when entering user mode
        let msr_gs_base: u32 = 0xC0000100;
        asm!(
            "wrmsr",
            in("ecx") msr_gs_base,
            in("eax") cpu0_data as u32,
            in("edx") (cpu0_data >> 32) as u32,
            options(nostack)
        );
        
        // IA32_KERNEL_GS_BASE (MSR 0xC0000101) - Kernel GS base
        // Used by swapgs when returning from user mode
        let msr_kernel_gs: u32 = 0xC0000101;
        asm!(
            "wrmsr",
            in("ecx") msr_kernel_gs,
            in("eax") cpu0_data as u32,
            in("edx") (cpu0_data >> 32) as u32,
            options(nostack)
        );
        
        // Now load GS selector to 0x10 (kernel data segment)
        // The actual base comes from the MSR above
        asm!(
            "mov ax, 0x10",
            "mov gs, ax",
            out("ax") _,
        );

        // Verify GS base is set correctly (debug)
        let mut gs_base_low: u32;
        let mut gs_base_high: u32;
        asm!(
            "rdmsr",
            in("ecx") msr_gs_base,
            out("eax") gs_base_low,
            out("edx") gs_base_high,
            options(nostack)
        );
        let gs_base = ((gs_base_high as u64) << 32) | (gs_base_low as u64);
        kprintln!("  [GDT] GS base set to {:#x}", gs_base);
    }

    // Initialize per-CPU data for BSP (Bootstrap Processor)
    crate::arch::smp::init_percpu(0, true, kernel_stack_top);

    crate::arch::syscall_entry::init_kernel_stack_ptr(kernel_stack_top);
    
    kprintln!("  [GDT] Loaded with TSS and Per-CPU data initialized");
}

/// Load the LDT (called after memory and IDT are initialized).
pub fn enable_ldt() {
    crate::kprintln!("  [GDT] LDT disabled (needs debug)");
}



/// Update the kernel stack for the current CPU (used on context switch).
pub fn set_kernel_stack(stack_top: u64) {
    TSS.lock().rsp0 = stack_top;
    crate::arch::syscall_entry::set_kernel_stack_ptr(stack_top);
    unsafe {
        let cpu_data_ptr = crate::arch::smp::get_cpu_data_ptr(0);
        (*cpu_data_ptr).kernel_stack.store(stack_top, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Convert a GdtEntry struct to a raw u64.
fn gdt_entry_to_u64(entry: GdtEntry) -> u64 {
    let bytes: [u8; 8] = unsafe { core::mem::transmute(entry) };
    u64::from_le_bytes(bytes)
}

/// Build the low 8 bytes of a 16-byte TSS descriptor.
fn tss_descriptor_low(base: u64, limit: u64) -> u64 {
    let mut desc: u64 = 0;
    desc |= limit & 0xFFFF;
    desc |= (base & 0xFFFF) << 16;
    desc |= ((base >> 16) & 0xFF) << 32;
    desc |= 0x89u64 << 40;
    desc |= ((limit >> 16) & 0xF) << 48;
    desc |= ((base >> 24) & 0xFF) << 56;
    desc
}

/// Build the high 8 bytes of a 16-byte TSS descriptor.
fn tss_descriptor_high(base: u64) -> u64 {
    (base >> 32) & 0xFFFF_FFFF
}

fn ldt_descriptor(base: u64, limit: u64) -> u64 {
    let mut desc: u64 = 0;
    desc |= limit & 0xFFFF;
    desc |= (base & 0xFFFF) << 16;
    desc |= ((base >> 16) & 0xFF) << 32;
    desc |= 0x82u64 << 40;  // Present, System, LDT, DPL=0
    desc |= ((limit >> 16) & 0xF) << 48;
    desc |= ((base >> 24) & 0xFF) << 56;
    desc
}

/// Set Kernel GS Base (for per-CPU data)
pub unsafe fn set_kernel_gs_base(base: u64) {
    let msr: u32 = 0xC0000101;  // IA32_KERNEL_GS_BASE
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") base as u32,
        in("edx") (base >> 32) as u32,
        options(nostack)
    );
}

/// Get Kernel GS Base
pub unsafe fn get_kernel_gs_base() -> u64 {
    let mut low: u32 = 0;
    let mut high: u32 = 0;
    let msr: u32 = 0xC0000101;
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nostack)
    );
    ((high as u64) << 32) | (low as u64)
}

/// Set User GS Base
pub unsafe fn set_user_gs_base(base: u64) {
    let msr: u32 = 0xC0000100;  // IA32_GS_BASE
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") base as u32,
        in("edx") (base >> 32) as u32,
        options(nostack)
    );
}

/// Get User GS Base
pub unsafe fn get_user_gs_base() -> u64 {
    let mut low: u32 = 0;
    let mut high: u32 = 0;
    let msr: u32 = 0xC0000100;
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nostack)
    );
    ((high as u64) << 32) | (low as u64)
}