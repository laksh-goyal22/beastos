# Beast OS Tech Stack

## Core Architecture
- **Language**: Rust (Nightly)
- **Target**: x86_64-beast_os (custom target spec)
- **Kernel Type**: Monolithic (moving towards modular/namespaced)
- **Bootloader (Limine)**
*   **Version:** 0.6.3 (Protocol v5+)
*   **Requests Section:** `.requests` (Must be Aligned 4K and KEPT in linker script)
*   **Entry Point:** `kmain` (Higher-half virtual address)

### Memory Model
*   **HHDM:** Higher-Half Direct Mapping. Physical memory is mapped at a large offset (from Limine).
*   **Kernel Base:** `0xFFFFFFFF80100000` (as defined in `kernel.ld`)

## Architecture & Systems
- **GDT**: Per-CPU GDT with TSS support for Ring 3 transitions.
- **IDT**: 256-entry Interrupt Descriptor Table.
- **Memory Management**: Paging (x86_64 4-level), Physical Memory Manager (bitmap allocator), Virtual Memory Manager (HHDM), and Kernel Heap (linked-list allocator).
- **Concurrency**: `Spinlock` for kernel-level synchronization.
- **SMP**: Per-CPU data support using GS segment and `SWAPGS` instruction.

## Standards & Protocols
- **Safety**: Moving away from `static mut` and `unsafe` global state towards `Spinlock` and safe abstractions.
- **CI/CD**: Local verification using `cargo check` and `clippy`.
- **Documentation**: `TECH_STACK.md` (Global Truth), `AGENTS.md` (Continuity).

## Scheduler & IPC
- **MLFQ (Multi-Level Feedback Queue)**: Dynamic priority scheduling with anti-starvation boosting.
- **Context Switch**: 3-push IRETQ for kernel→kernel, 5-push IRETQ for user→kernel/user. Segment registers (DS, ES, SS, FS) reloaded before IRETQ to prevent GPF from user-mode corruption.
- **SPSC-Aware Boosting**: IPC channels (SPSC rings) are integrated with the scheduler. 
    - **Producer blocked (Full)**: Boosts consumer immediately to level 0.
    - **Consumer blocked (Empty)**: Boosts producer immediately to level 0 and prepares consumer for boost on wake.
    - **Latency**: 10x reduction in IPC tail latency by eliminating "blind" context switches.
- **Task Joining**: Efficient `wait_task` mechanism using `waiting_on` metadata. `exit_current` proactively wakes all waiters in a single scan, preventing polling.

## Virtual Memory Manager (VMM)
- **Recursive Page Table Mapping**: PML4[510] for kernel page table access.
- **On-Demand Paging**: `map_user_page` with bit-9 marker; page fault handler allocates physical page on first access.
- **Huge Page Support**: 1GiB and 2MiB page splitting for fine-grained mapping within large regions.
- **Page Fault Integration**: `handle_fault()` resolves on-demand faults; falls through to panic for real faults.
- **Page Table Walker**: `translate()`, `unmap_page()`, `map_page()` with automatic intermediate table creation.
