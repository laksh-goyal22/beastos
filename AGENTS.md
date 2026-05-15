# Pod Coordination (AGENTS.md)

## Active Pod: [antigravity]
**Status:** 🟢 IN PROGRESS
**Global Lock:** None

---

## 📅 MILESTONES

### 2026-05-13 — Process Isolation & Fault Recovery
- **IMPLEMENTED**: `create_user_page_table()` in `memory/vmm.rs` — now allocates a fresh PML4, copies kernel-space entries (indices 256–511) from kernel CR3, and sets up recursive mapping. User processes now have isolated address spaces instead of sharing kernel CR3.
- **FIXED**: Exception handlers in `arch/idt.rs` — `division_error`, `invalid_opcode`, `page_fault`, and `general_protection` handlers now check CPL and call `exit_current()` for user-mode faults instead of halting the entire CPU. Kernel-mode faults still halt for debugging.
- **UPDATED**: `AGENTS.md` and `TECH_STACK.md` to reflect current progress.

### 2026-05-10 — Phase 10: VMM Complete
- **IMPLEMENTED**: `VirtualMemoryManager` struct with:
  - Recursive page table mapping (PML4[510])
  - `map_page` / `map_page_with_flags` with automatic page table creation and huge-page splitting (1GiB, 2MiB)
  - `map_user_page` with on-demand paging support (bit 9 marker, no physical page until fault)
  - `handle_fault` — resolves on-demand faults by allocating and mapping physical pages
  - `unmap_page` and `translate` (huge-page aware)
- **INTEGRATED**: Page fault handler in `idt.rs` calls `vmm.handle_fault()` before panicking
- **STUB**: `create_user_page_table()` returns kernel CR3 — no process isolation yet
- Fixed compilation errors in `main.rs` (`user` flag to `map_page`)
- Fixed unused parameter warning in `vmm.rs` (`error_code` -> `_error_code`)
- Cleaned up imports in `syscall_entry.rs`, `syscall/mod.rs`

### 2026-05-09 — Phase 9: IPC & Task Joining
- **IMPLEMENTED**: Task Joining (Wait/Join) mechanism
  - `waiting_on` metadata on `Task`, `wait_task()` blocks until target exits
  - `exit_current()` wakes all waiters in a single scan
  - `BlockReason::Waiting` in MLFQ no-boost list
- **FIXED**: SPSC IPC bugs
  - Lost-wakeup race via `awake_pending` flag
  - Priority queue corruption — check `t.state` before pushing to `Ready`
  - SPSC-aware priority boosting (10x latency reduction)
  - `BlockReason` enum: struct variants → fields
- Verified bootloader requests and section names in `kernel.ld`
- Implemented Radioactive Green Boot Screen (0xFF33FF33)
- **FIXED**: Duplicate `syscall_dispatch` symbol error
- **FIXED**: `asm_sub_register` warnings in `arch/userspace.rs`
- **CLEANUP**: Verified GDT/IDT consistency for IST usage

---

## 🛠️ HANDOVER NOTES

### Current State:
- Kernel boots, framebuffer console (1280x800 at `0xffff8000fd000000`)
- HHDM, GDT, IDT, PIC, PIT, Keyboard — all initialized
- **Syscall system stable**, Task Joining works
- **VMM complete**: recursive mapping, on-demand paging, huge page splitting, page fault integration
- `create_user_page_table()` is **still a stub** — all user processes share kernel CR3

### Next Objective: Syscall Expansion & Userland Foundation
- Expand the syscall set to support real user programs:
  - `sys_open` / `sys_read` / `sys_write` — file I/O
  - `sys_exec` — program loading with per-process page tables
  - `sys_exit` / `sys_brk` — process lifecycle
- Build a minimal libc / crt0 for userland programs

### 2026-05-13 — Syscall Expansion & CR3-Aware Access
- **IMPLEMENTED**: `sys_brk` (syscall 45) — program break management with on-demand page allocation for user heap
- **FIXED**: `sys_write` — now CR3-aware, switches to user page table to read the buffer
- **FIXED**: `sys_exec` — now CR3-aware, switches to user page table to read the path
- **FIXED**: `Task::new_user` — variable name bugs (used `arg1`, `arg2`, `page_table`, `phys`, `virt`, `rsp` from wrong scope)
- **ADDED**: `brk` field to Task struct, initialized to `0x0000_2000_0000_0000` for user tasks

### 2026-05-13 — libc / crt0 for Userland ELFs
- **CREATED**: `libs/beast_syscall/` — shared no_std syscall library with 30+ wrappers (read, write, open, close, exec, exit, brk, yield, getpid, chdir, getcwd, ls, mkdir, ps, uptime, fb_info, get_mouse, clipboard, wait, lseek, create, delete, clear_screen) and raw `syscall0`–`syscall5` inline asm primitives
- **CREATED**: `libs/beast_crt/` — standard C runtime: `_start(argc, argv)` → calls user's `main(argc, argv)` → `sys_exit(code)`. Includes a default `#[panic_handler]`
- **MIGRATED**: `userland/hello` — now uses `beast_syscall` instead of its own `syscalls.rs` (removed 78 lines of duplicate code)
- **UPDATED**: workspace `Cargo.toml` includes `beast_syscall` and `beast_crt`

### 2026-05-14 — Userland Cleanup & Cursor Fix
- **CLEANUP**: Removed stale `userland/shell/syscalls.rs` (dead file — shell already used `beast_syscall` everywhere)
- **CLEANUP**: Removed `beast_os_kernel` dep from `userland/editor/Cargo.toml` (userland shouldn't link kernel crate)
- **FIXED**: Cursor rendering in `console.rs` — cursor now properly saves/restores pixels under the cursor block, eliminating the persistent semi-transparent overlay when cursor blinks off
- **ADDED**: `read_pixels()` function to `kernel/src/drivers/framebuffer.rs` for cursor area save/restore

### 2026-05-14 — FAT32 Filesystem Driver
- **IMPLEMENTED**: Full FAT32 read-only driver in `kernel/src/fs/fat32.rs`:
  - MBR partition table parsing (supports partition types 0x0B and 0x0C)
  - FAT32 BPB (BIOS Parameter Block) parsing with validation
  - Cluster chain traversal via FAT entry reading
  - Short (8.3) filename directory entry parsing
  - VFAT long filename (LFN) entry parsing with UTF-16LE to String conversion
  - File open/read/seek via VFS `File` trait
  - Directory listing via `read_dir`
  - Mounted at `/fat` in the VFS on init

### 2026-05-14 — Quick Fixes Round
- **FIXED**: Mouse bounds hardcoded to 800x600 in `drivers/mouse.rs` — now uses real framebuffer dimensions
- **FIXED**: Token expiry in `fs/token.rs` — `is_expired()` now compares against PIT tick count
- **ENABLED**: `ring_transfer` module in `fs/mod.rs` — fixed imports, enabled compilation
- **CLEANUP**: Removed unused `String` import from `fs/token.rs`, added public `pit::get_hz()` getter

### 2026-05-14 — DevFS with Device Nodes
- **IMPLEMENTED**: Complete device filesystem at `kernel/src/fs/devfs.rs`:
  - `/dev/stdin` — reads from kernel keyboard buffer
  - `/dev/mouse` — returns binary mouse state (x, y, buttons)
  - `/dev/fb` — returns framebuffer info (width, height, pitch, base, size)
  - `/dev/serial` — raw COM1 read/write via port I/O
  - Mounted at `/dev` during filesystem init

### 2026-05-14 — USB Driver Migration & Syscall Fix
- **FIXED**: Incorrect syscall constants in `driver_syscalls.rs` (10/11/13 → 200/201/202)
- **ADDED**: Driver syscall wrappers to `beast_syscall` — `map_mmio()`, `bind_irq()`, `create_irq_ring()`, `port_in()`, `port_out()`
- **MIGRATED**: `userland/usb_driver` to use `beast_syscall` + `beast_crt`

### 2026-05-15 — Warning Cleanup & Boot Fix
- **FIXED**: All kernel library warnings eliminated (17+ → 0) — unused variables, imports, constants, and dead code across fat32, ring_transfer, gc, token
- **FIXED**: Pre-existing triple fault at AHCI PCI scan — `pci_config_read()` used memory-mapped writes to address `0xCF8` (causing #PF) instead of proper `in`/`out` I/O port instructions
- **FIXED**: Timer IRQ (vector 32) masked during early boot to prevent spurious interrupts before IDT is initialized; unmasked after IDT init via `pic::enable_timer()`
- **BOOT CONFIRMED**: Kernel boots through all phases, loads userland ELF, reaches scheduler — full boot-to-userland verified

### Next Steps:
1. Minor warning cleanup (unused structs in FAT32, unused vars)
2. Editor dead_code warnings cleanup
3. Boot test: verify kernel boots with new FAT32/devfs changes

### Verification Command:
```bash
wsl -d Ubuntu bash -lc "cd /home/laksh/beastos && cargo build --target x86_64-unknown-none"
```
