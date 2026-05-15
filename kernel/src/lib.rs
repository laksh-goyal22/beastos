//! Beast OS Kernel Library
//!
//! The core kernel crate for Beast OS — a microkernel operating system
//! targeting x86_64 with zero-copy IPC, lock-free SPSC rings, and
//! O(1) algorithmic complexity throughout.
//!
//! # Architecture
//!
//! - `arch/`     — x86_64-specific: GDT, IDT, paging, syscalls
//! - `memory/`   — Physical/virtual memory management, heap, CoW
//! - `scheduler/`— Task scheduling, context switching
//! - `ipc/`      — SPSC rings, channels, shared memory, capabilities
//! - `sync/`     — Spinlocks, mutexes, semaphores
//! - `drivers/`  — Ring 0 hardware shims (framebuffer, keyboard, serial)
//! - `fs/`       — VFS, FAT32, tmpfs
//! - `syscall/`  — System call interface, V-DSO
//! - `time/`     — PIT, HPET, APIC timer, TSC
//! - `acpi/`     — ACPI table parsing, power management

#![no_std]
#![no_main]
// #![feature(const_mut_refs)]

extern crate alloc;

// Include the trampolines assembly file
core::arch::global_asm!(include_str!("arch/asm/trampolines.S"));

// pub mod acpi;
pub mod arch;
pub mod drivers;
pub mod fs;
pub mod ipc;
pub mod logger;
pub mod memory;
pub mod scheduler;
pub mod sync;
pub mod syscall;
pub mod storage;
// pub mod time;