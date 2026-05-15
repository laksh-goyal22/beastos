//! x86_64 Architecture Module
//!
//! Platform-specific CPU management including:
//! - GDT (Global Descriptor Table)
//! - IDT (Interrupt Descriptor Table)
//! - Paging (PML4, PCID)
//! - Syscall (LSTAR MSR)
//! - SMP (multi-core boot)

pub mod gdt;
pub mod idt;
pub mod paging;
pub mod syscall_entry;
pub mod smp;
pub mod userspace;
pub mod asm;
pub mod interrupt_forwarding;
