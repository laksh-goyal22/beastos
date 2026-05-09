# Beast OS - Top-Level Build Orchestration
# ==========================================

KERNEL_BINARY := target/x86_64-beast_os/release/beast_os_kernel
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
	$(CARGO_CMD) --release -p ahci
	$(CARGO_CMD) --release -p usb
	$(CARGO_CMD) --release -p network
	$(CARGO_CMD) --release -p gpu
	$(CARGO_CMD) --release -p audio
	@echo "✅ Drivers built"

# Build userland
userland:
	@echo "🔨 Building userland..."
	$(CARGO_CMD) --release -p libc
	$(CARGO_CMD) --release -p shell
	$(CARGO_CMD) --release -p bde
	@echo "✅ Userland built"

# Create bootable ISO
iso: build
	@echo "📀 Creating bootable ISO..."
	@mkdir -p $(ISO_DIR)/boot
	@cp $(KERNEL_BINARY) $(ISO_DIR)/boot/beast.kernel
	@cp boot/limine/limine.cfg $(ISO_DIR)/boot/
	@cp boot/limine/limine.sys $(ISO_DIR)/boot/
	@cp boot/limine/limine.sys $(ISO_DIR)/boot/limine-bios.sys
	@cp boot/limine/limine-bios-cd.bin $(ISO_DIR)/boot/
	@cp boot/limine/limine-uefi-cd.bin $(ISO_DIR)/boot/
	xorriso -as mkisofs -b boot/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image \
		--protective-msdos-label \
		-o $(ISO_FILE) $(ISO_DIR)
	./boot/limine/limine bios-install $(ISO_FILE)
	@echo "✅ ISO created: $(ISO_FILE)"

# Run in QEMU
run: iso
	@echo "🚀 Launching Beast OS in QEMU..."
	$(QEMU) -m $(QEMU_MEMORY) $(QEMU_FLAGS) \
		-cdrom $(ISO_FILE) \
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
