# Beast OS - Justfile (Alternative Task Runner)
# ================================================

# Default recipe
default: build

# Build everything
build:
    cargo build --release --workspace

# Build kernel only
kernel:
    cargo build --release -p beast_os_kernel

# Run in QEMU
run: build
    ./scripts/run_qemu.sh

# Debug with GDB
debug: build
    ./scripts/debug.sh

# Run tests
test:
    cargo test --workspace

# Run benchmarks
bench:
    cargo bench -p beast_ring

# Create ISO
iso: build
    ./scripts/package.sh

# Clean
clean:
    cargo clean

# Format code
fmt:
    cargo fmt --all

# Lint
lint:
    cargo clippy --workspace -- -D warnings

# Run on real Mac hardware via USB
mac: iso
    ./scripts/run_mac.sh
