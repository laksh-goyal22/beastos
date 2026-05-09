//! Global Descriptor Table (GDT) for x86_64
//!
//! Beast OS uses a per-CPU GDT to avoid SMP race conditions.
//! Each core gets its own TSS (Task State Segment) for Ring 3 → Ring 0
//! transitions via `swapgs`.

use core::mem::size_of;

/// GDT entry (8 bytes for standard, 16 bytes for TSS).
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

impl GdtEntry {
    /// Create a null GDT entry.
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_mid: 0,
            access: 0,
            flags_limit_hi: 0,
            base_hi: 0,
        }
    }

    /// Create a kernel code segment (Ring 0, 64-bit).
    pub const fn kernel_code() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_mid: 0,
            access: 0x9A,        // Present, Ring 0, Code, Execute/Read
            flags_limit_hi: 0xAF, // 64-bit, 4KB granularity
            base_hi: 0,
        }
    }

    /// Create a kernel data segment (Ring 0).
    pub const fn kernel_data() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_mid: 0,
            access: 0x92,        // Present, Ring 0, Data, Read/Write
            flags_limit_hi: 0xCF,
            base_hi: 0,
        }
    }

    /// Create a user code segment (Ring 3, 64-bit).
    pub const fn user_code() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_mid: 0,
            access: 0xFA,        // Present, Ring 3, Code, Execute/Read
            flags_limit_hi: 0xAF,
            base_hi: 0,
        }
    }

    /// Create a user data segment (Ring 3).
    pub const fn user_data() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_mid: 0,
            access: 0xF2,        // Present, Ring 3, Data, Read/Write
            flags_limit_hi: 0xCF,
            base_hi: 0,
        }
    }
}

/// Task State Segment — used for Ring 3 → Ring 0 stack switching.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Tss {
    _reserved0: u32,
    pub rsp0: u64,           // Ring 0 stack pointer
    pub rsp1: u64,
    pub rsp2: u64,
    _reserved1: u64,
    pub ist: [u64; 7],       // Interrupt Stack Table
    _reserved2: u64,
    _reserved3: u16,
    pub iopb_offset: u16,
}

impl Tss {
    pub const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iopb_offset: size_of::<Tss>() as u16,
        }
    }
}

/// Per-CPU GDT (6 entries + 1 TSS descriptor)
#[repr(C, align(16))]
pub struct Gdt {
    entries: [GdtEntry; 7],
}

impl Gdt {
    pub const fn new() -> Self {
        Self {
            entries: [
                GdtEntry::null(),       // 0x00: Null
                GdtEntry::kernel_code(), // 0x08: Kernel Code
                GdtEntry::kernel_data(), // 0x10: Kernel Data
                GdtEntry::user_code(),   // 0x18: User Code
                GdtEntry::user_data(),   // 0x20: User Data
                GdtEntry::null(),        // 0x28: TSS Low (filled at runtime)
                GdtEntry::null(),        // 0x30: TSS High
            ],
        }
    }
}

/// GDT Pointer (used by `lgdt` instruction)
#[repr(C, packed)]
pub struct GdtPointer {
    pub limit: u16,
    pub base: u64,
}

/// Initialize the GDT for the current CPU core.
pub fn init() {
    // TODO: Set up per-CPU GDT with TSS
    // 1. Allocate kernel stack for Ring 0
    // 2. Fill TSS.rsp0 with kernel stack pointer
    // 3. Create TSS descriptor in GDT slots 5-6
    // 4. Load GDT via lgdt
    // 5. Reload segment registers
    // 6. Load TSS via ltr
}
