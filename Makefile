# Beast OS - Top-Level Build Orchestration
# ==========================================

KERNEL_BINARY := target/x86_64-beast_os/release/beast_os
ISO_DIR := iso
ISO_FILE := beast-os.iso
QEMU := qemu-system-x86_64
QEMU_MEMORY := 512M
QEMU_FLAGS := -serial stdio -no-reboot -no-shutdown
CARGO_CMD := cargo build -Z build-std=core,compiler_builtins,alloc -Z json-target-spec

.PHONY: all build kernel drivers userland run debug test bench clean iso

# Default target
all: build

# Full build
build: kernel drivers userland
	@echo "✅ Beast OS build complete"

# Build kernel
kernel:
	@echo "🔨 Building kernel..."
	$(CARGO_CMD) --release -p beast_os_kernel
	@echo "✅ Kernel built"

# Build userspace drivers
drivers:
	@echo "🔨 Building drivers..."
	$(CARGO_CMD) --release -p ahci_driver
	$(CARGO_CMD) --release -p usb_driver
	$(CARGO_CMD) --release -p network_driver
	$(CARGO_CMD) --release -p gpu_driver
	$(CARGO_CMD) --release -p audio_driver
	@echo "✅ Drivers built"

# Build userland
userland:
	@echo "🔨 Building userland..."
	$(CARGO_CMD) --release -p beast_libc
	$(CARGO_CMD) --release -p beast_shell
	$(CARGO_CMD) --release -p beast_desktop
	@echo "✅ Userland built"

# Create bootable ISO
iso: build
	@echo "📀 Creating bootable ISO..."
	@mkdir -p $(ISO_DIR)/boot
	@cp $(KERNEL_BINARY) $(ISO_DIR)/boot/beast.kernel
	@cp boot/limine/limine.cfg $(ISO_DIR)/boot/
	@cp boot/limine/limine.sys $(ISO_DIR)/boot/
	xorriso -as mkisofs -b boot/limine.sys \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		-o $(ISO_FILE) $(ISO_DIR)
	@echo "✅ ISO created: $(ISO_FILE)"

# Run in QEMU
run: build
	@echo "🚀 Launching Beast OS in QEMU..."
	$(QEMU) -m $(QEMU_MEMORY) $(QEMU_FLAGS) \
		-drive format=raw,file=$(ISO_FILE) \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04

# Debug with GDB
debug: build
	@echo "🐛 Launching Beast OS with GDB..."
	$(QEMU) -m $(QEMU_MEMORY) $(QEMU_FLAGS) \
		-drive format=raw,file=$(ISO_FILE) \
		-s -S &
	gdb -ex "target remote :1234" -ex "symbol-file $(KERNEL_BINARY)"

# Run tests
test:
	@echo "🧪 Running tests..."
	cargo test --workspace
	@echo "✅ All tests passed"

# Run benchmarks
bench:
	@echo "📊 Running benchmarks..."
	cargo bench -p beast_ring
	@echo "✅ Benchmarks complete"

# Clean build artifacts
clean:
	@echo "🧹 Cleaning..."
	cargo clean
	@rm -rf $(ISO_DIR)/boot/beast.kernel
	@rm -f $(ISO_FILE)
	@echo "✅ Clean"
