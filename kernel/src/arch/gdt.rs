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
            access: 0x9A,        // P=1, DPL=0, S=1, E=1, R=1
            flags_limit_hi: 0xAF, // G=1, L=1 (64-bit), Limit[19:16]=F
            base_hi: 0,
        }
    }

    /// Kernel data segment (Ring 0).
    pub const fn kernel_data() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0x92,        // P=1, DPL=0, S=1, W=1
            flags_limit_hi: 0xCF,
            base_hi: 0,
        }
    }

    /// User data segment (Ring 3) — must come before user code for SYSCALL/SYSRET.
    pub const fn user_data() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0xF2,        // P=1, DPL=3, S=1, W=1
            flags_limit_hi: 0xCF,
            base_hi: 0,
        }
    }

    /// User code segment (Ring 3, 64-bit).
    pub const fn user_code() -> Self {
        Self {
            limit_low: 0xFFFF, base_low: 0, base_mid: 0,
            access: 0xFA,        // P=1, DPL=3, S=1, E=1, R=1
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
///
/// Note: For SYSRET on AMD64, user data MUST be at STAR_SEL+0
///       and user code at STAR_SEL+8. We use 0x18/0x20.
#[repr(C, align(16))]
struct GdtTable {
    entries: [u64; 7], // 5 standard + 2 for TSS (16-byte descriptor)
}

static GDT: Spinlock<GdtTable> = Spinlock::new(GdtTable { entries: [0; 7] });
static TSS: Spinlock<Tss> = Spinlock::new(Tss::new());

/// Kernel-mode interrupt stack (16KB, page-aligned).
#[repr(C, align(4096))]
struct KernelStack([u8; 16384]);
static KERNEL_STACK: KernelStack = KernelStack([0; 16384]);

/// Double-fault IST stack (8KB).
#[repr(C, align(4096))]
struct IstStack([u8; 8192]);
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
    let kernel_stack_top = addr_of!(KERNEL_STACK) as u64 + 16384; // stack grows down
    tss.rsp0 = kernel_stack_top;
    tss.ist[0] = addr_of!(IST_STACK) as u64 + 8192;   // IST1 for double fault

    // Build GDT entries
    gdt.entries[0] = 0; // Null
    gdt.entries[1] = gdt_entry_to_u64(GdtEntry::kernel_code()); // 0x08
    gdt.entries[2] = gdt_entry_to_u64(GdtEntry::kernel_data()); // 0x10
    gdt.entries[3] = gdt_entry_to_u64(GdtEntry::user_data());   // 0x18
    gdt.entries[4] = gdt_entry_to_u64(GdtEntry::user_code());   // 0x20

    // TSS descriptor (16 bytes = 2 entries)
    let tss_base = (&*tss as *const Tss) as u64;
    let tss_limit = (size_of::<Tss>() - 1) as u64;
    gdt.entries[5] = tss_descriptor_low(tss_base, tss_limit);
    gdt.entries[6] = tss_descriptor_high(tss_base);

    unsafe {
        // Load GDT
        let gdt_ptr = GdtPointer {
            limit: (core::mem::size_of::<GdtTable>() - 1) as u16,
            base: (&*gdt as *const GdtTable) as u64,
        };
        asm!("lgdt [{}]", in(reg) &gdt_ptr, options(nostack));

        // Reload segment registers
        // CS: far return trick (AT&T syntax)
        asm!(
            "pushq $0x08",           // kernel code selector
            "leaq 2f(%rip), {tmp}",
            "pushq {tmp}",
            "lretq",
            "2:",
            tmp = out(reg) _,
            options(att_syntax)
        );

        // DS, SS, ES, FS, GS = kernel data (0x10)
        asm!(
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "xor ax, ax",
            "mov fs, ax",
            "mov gs, ax",
            out("ax") _,
        );

        // Load TSS (selector = 0x28)
        asm!("ltr ax", in("ax") 0x28u16, options(nostack));
    }

    // Initialize per-CPU data for BSP (Bootstrap Processor)
    crate::arch::smp::init_percpu(0, true, kernel_stack_top);

    kprintln!("  [GDT] Loaded with TSS and Per-CPU data initialized");
}

/// Convert a GdtEntry struct to a raw u64.
fn gdt_entry_to_u64(entry: GdtEntry) -> u64 {
    let bytes: [u8; 8] = unsafe { core::mem::transmute(entry) };
    u64::from_le_bytes(bytes)
}

/// Build the low 8 bytes of a 16-byte TSS descriptor.
fn tss_descriptor_low(base: u64, limit: u64) -> u64 {
    let mut desc: u64 = 0;
    desc |= limit & 0xFFFF;                         // Limit[15:0]
    desc |= (base & 0xFFFF) << 16;                  // Base[15:0]
    desc |= ((base >> 16) & 0xFF) << 32;            // Base[23:16]
    desc |= 0x89u64 << 40;                          // Type=9 (64-bit TSS), P=1
    desc |= ((limit >> 16) & 0xF) << 48;            // Limit[19:16]
    desc |= ((base >> 24) & 0xFF) << 56;            // Base[31:24]
    desc
}

/// Build the high 8 bytes of a 16-byte TSS descriptor.
fn tss_descriptor_high(base: u64) -> u64 {
    (base >> 32) & 0xFFFF_FFFF // Base[63:32]
}
