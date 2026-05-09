# Beast OS 🦁

> A hyper-optimized microkernel operating system written in Rust, built on the "never move data twice" principle.

## Overview

Beast OS is a from-scratch operating system targeting x86_64 (specifically optimized for Late 2009 MacBook with Core 2 Duo). It implements a microkernel architecture with Ring 3 userspace drivers, zero-copy IPC via lock-free SPSC rings, and a premium desktop environment.

## Key Innovations

- **Zero-Copy Architecture** — Physical Page Aliasing eliminates data copies between kernel and userspace
- **SPSC Lock-Free Rings** — All IPC uses Single-Producer Single-Consumer rings with atomic head/tail pointers (~50ns enqueue)
- **O(1) Everything** — Handle-based arrays, V-DSO acceleration, hash-mapped paths
- **Ring 3 Microkernel Drivers** — Crashed drivers restart without kernel panic
- **Glass Engine** — SSE4.1-accelerated Porter-Duff alpha blending compositor
- **Capability-Based Security** — 128-bit opaque tokens, O(1) lookup

## Architecture

```
┌─────────────────────────────────────────┐
│           User Applications             │
│  (Shell, File Manager, Terminal, BDE)   │
├─────────────────────────────────────────┤
│         Ring 3 Drivers & Services       │
│  (AHCI, USB, Network, GPU, Audio)       │
├─────────────────────────────────────────┤
│         Glass Engine Compositor         │
│  (SSE4.1 blending, damage tracking)     │
├─────────────────────────────────────────┤
│              libc / libBeast            │
│  (Syscall wrappers, SPSC user API)      │
├─────────────────────────────────────────┤
│       ═══════ SYSCALL BOUNDARY ═══════  │
├─────────────────────────────────────────┤
│            Beast Microkernel            │
│  (Memory, Scheduler, IPC, VFS, Caps)    │
├─────────────────────────────────────────┤
│         Hardware Abstraction Layer      │
│  (x86_64, APIC, ACPI, PCI, DMA)        │
└─────────────────────────────────────────┘
```

## Building

### Prerequisites

- Rust nightly toolchain (`rustup toolchain install nightly`)
- NASM assembler
- QEMU for testing (`qemu-system-x86_64`)
- `xorriso` for ISO creation
- GNU Make

### Quick Start

```bash
# Clone and build
git clone https://github.com/yourusername/beast-os.git
cd beast-os

# Build the kernel
make build

# Run in QEMU
make run

# Run tests
make test
```

### Build Targets

| Target | Description |
|--------|-------------|
| `make build` | Build kernel and userland |
| `make run` | Launch in QEMU |
| `make debug` | Launch with GDB attached |
| `make iso` | Create bootable ISO |
| `make test` | Run all tests |
| `make bench` | Run benchmarks |
| `make clean` | Clean build artifacts |

## Project Structure

```text
beast-os/
├── boot/                   # Bootloader configuration
│   └── linker/
│       └── kernel.ld       # Kernel linker script (Limine protocol compliant)
├── kernel/                 # Microkernel (Ring 0) — active development
│   ├── build.rs            # Build script: linker args & asm file watch
│   ├── Cargo.toml          # Kernel crate dependencies (limine, spin, uart_16550)
│   └── src/
│       ├── main.rs         # Kernel entry point (kmain, Limine request statics)
│       ├── lib.rs          # Kernel crate root — re-exports all modules
│       ├── logger.rs       # Serial-port debug logger (kprint!/kprintln!)
│       ├── sync.rs         # Spinlock<T> synchronization primitive
│       ├── arch/           # x86_64 CPU-specific code
│       │   ├── mod.rs      # arch module root
│       │   ├── gdt.rs      # Global Descriptor Table + TSS (Spinlock-protected)
│       │   ├── idt.rs      # Interrupt Descriptor Table + exception handlers
│       │   ├── paging.rs   # 4-level page table management
│       │   ├── smp.rs      # SMP / LAPIC initialization (stub)
│       │   ├── syscall_entry.rs  # SYSCALL/SYSRET entry point
│       │   └── asm/
│       │       └── mod.rs  # MSR read/write helpers (rdmsr, wrmsr)
│       ├── drivers/        # Ring 0 hardware drivers
│       │   ├── mod.rs      # drivers module root (init() dispatcher)
│       │   ├── console.rs  # Text console and basic VGA/framebuffer text rendering
│       │   ├── framebuffer.rs   # Limine framebuffer init + pixel draw
│       │   ├── keyboard.rs # PS/2 keyboard initialization and interrupt handling
│       │   ├── pic.rs      # 8259 PIC remapping
│       │   └── pit.rs      # PIT timer configuration
│       └── memory/         # Memory management
│           ├── mod.rs      # memory module root
│           ├── heap.rs     # Kernel heap allocator (linked-list/slab-based)
│           ├── pmm.rs      # Physical Memory Manager (bitmap allocator)
│           └── vmm.rs      # Virtual Memory Manager + HHDM offset
├── drivers/                # Ring 3 userspace drivers (planned)
├── compositor/             # Glass Engine compositor (planned)
├── userland/               # User applications & libraries (planned)
├── libs/                   # Shared Rust libraries (planned)
├── tools/                  # Build utilities (planned)
├── scripts/                # Build & run automation (planned)
├── docs/                   # Architecture documentation (planned)
├── tests/                  # Integration tests (planned)
├── AGENTS.md               # AI agent handover / session continuity log
└── Makefile                # Build orchestration
```

## Core Modules

### 1. Kernel (Ring 0)
The heart of Beast OS. It handles only the most essential tasks:
- **SMP Management**: Multi-core initialization and per-CPU data structures.
- **Memory Management**: A custom Buddy Allocator for physical pages and a 4-level paging system.
- **Scheduler**: A high-performance, O(1) scheduler designed for low latency.
- **IPC**: The primary communication mechanism using lock-free SPSC rings.

### 2. Glass Engine (Compositor)
A premium graphics compositor that implements:
- **SSE4.1 Acceleration**: Fast alpha blending and image processing.
- **Damage Tracking**: Only redraws modified screen regions to save CPU cycles.
- **Glassmorphism**: Native support for frosted glass effects and smooth gradients.

### 3. Drivers (Ring 3)
Following the microkernel philosophy, most drivers run in userspace:
- **Isolation**: Crashed drivers are restarted by the kernel without affecting system stability.
- **Performance**: High-speed communication with the kernel via shared memory and rings.


## Target Hardware

- **CPU**: Intel Core 2 Duo (x86_64, SSE4.1)
- **GPU**: NVIDIA GeForce 9400M
- **Ethernet**: Broadcom BCM57780
- **WiFi**: Broadcom BCM4322
- **Audio**: Intel HDA

## Performance Targets

| Metric | Target | Status |
|--------|--------|--------|
| Syscall latency | <80ns | 🎯 |
| IPC enqueue | ~50ns | 🎯 |
| Pipe throughput | 20GB/s | 🎯 |
| Fork latency | <1ms | 🎯 |
| App startup | <1ms | 🎯 |
| Compositor FPS | 60+ | 🎯 |

## License

This project is licensed under the GPL-3.0 License - see [LICENSE](LICENSE) for details.
