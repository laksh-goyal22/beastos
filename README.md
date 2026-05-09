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

```
beast-os/
├── boot/          # Bootloader (Limine + assembly stages)
├── kernel/        # Microkernel (Ring 0)
├── drivers/       # Ring 3 userspace drivers
├── compositor/    # Glass Engine graphics compositor
├── userland/      # User applications & libraries
├── libs/          # Shared libraries (SPSC, allocator, fonts)
├── tools/         # Build utilities
├── scripts/       # Build & run scripts
├── docs/          # Documentation
└── tests/         # Integration tests
```

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
