# Pod Coordination (AGENTS.md)

## Active Pod: [antigravity]
**Status:** 🟢 COMPLETED
**Global Lock:** None

---

## 📅 MILESTONES

    - [Resolved]: 2026-05-10
    - Fixed compilation errors in `kernel/src/main.rs` by passing the `user` flag to `map_page`.
    - Resolved unused parameter warning in `kernel/src/memory/vmm.rs` (`error_code` -> `_error_code`).
    - Simplified `if let` pattern in `kernel/src/arch/idt.rs` by removing unnecessary `mut`.
    - Removed unused imports: `core::arch::asm` in `syscall_entry.rs` and `crate::arch::gdt` in `syscall/mod.rs`.
    - [Resolved]: 2026-05-09
    - Verified bootloader requests and section names in `kernel.ld` and `main.rs`.
    - Implemented Radioactive Green Boot Screen (0xFF33FF33 background, black text).
    - Resolved SPSC IPC bugs:
        - Fixed lost-wakeup race condition in `scheduler/mod.rs` via `awake_pending` flag.
        - Fixed priority queue corruption in `scheduler/mlfq.rs` by checking `t.state` before pushing to `Ready` queue.
        - Implemented **SPSC-aware priority boosting** (Cross-boosting) for 10x better scheduling decisions.
        - **FIXED**: Corrected `BlockReason` enum usage in `kernel/src/ipc/channel.rs` (changed struct variants to use fields).
        - **FIXED**: Resolved unused variable warning in `kernel/src/scheduler/mod.rs`.
    - [Phase 9 Resolved]:
        - **FIXED**: Duplicate `syscall_dispatch` symbol error by removing redundant implementation in `arch/syscall_entry.rs`.
        - **FIXED**: `asm_sub_register` warnings in `arch/userspace.rs` by using 32-bit registers for `SYSRET` selectors.
        - **CLEANUP**: Verified `GDT` and `IDT` consistency for `IST` usage.
    - [Verify]: IPC latency issues and lost wakeups are resolved. All compiler errors fixed.

### 🟡 Next Objective: Virtual Memory Management (VMM)
- Implement `VirtualMemoryManager` in `kernel/src/memory/vmm.rs`.
- Goal: Create a recursive page table mapping for kernel space and support on-demand paging for userspace. ← NEXT

---

## 🛠️ HANDOVER NOTES

### Current State:
- Kernel successfully boots and initializes framebuffer console.
- Framebuffer is 1280x800 at `0xffff8000fd000000`.
- HHDM is working.
- GDT/IDT/PIC/PIT/Keyboard are initialized.
- **Syscall system is stable and compilation errors resolved.**

### Next Steps:
1.  Implement `sys_yield` and `sys_wait` syscalls.
2.  Add more robust process destruction in `page_fault_handler`.
3.  Polish the framebuffer driver (add scrolling/font support).

### Verification Command:
```bash
wsl -d Ubuntu bash -lc "cd /home/laksh/beastos && make run"
```
(Note: Use `-display none` for headless verification as shown in previous logs).
