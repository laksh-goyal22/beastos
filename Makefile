# Beast OS - Top-Level Build Orchestration
# ==========================================

KERNEL_BINARY := target/x86_64-unknown-none/release/beast_os_kernel
ISO_DIR := iso
ISO_FILE := beast-os.iso
QEMU := qemu-system-x86_64
QEMU_MEMORY := 512M
QEMU_FLAGS := -nographic -no-reboot -no-shutdown
# Graphical window: use: make graphical
CARGO_CMD := cargo build -q
CARGO_QUIET := CARGO_TERM_COLOR=never RUSTFLAGS=-Awarning

.PHONY: all build kernel drivers userland run go debug test bench clean iso

# Default target
all: build iso

# Build kernel, drivers, and userland
build: kernel drivers userland
	@echo "✅ Beast OS build complete"

# Compile the kernel
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

# Build userland applications
userland:
	@mkdir -p target/initrd/bin
	@mkdir -p target/initrd/dev
	@mkdir -p target/initrd/home
	@mkdir -p target/initrd/usr
	@echo "🔨 Building userland..."
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p libc
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p usb_driver
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p hello
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p compositor
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p io_uring_test
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p test_keyboard
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p shell
	RUSTFLAGS="-C link-arg=-Ttext=0x400000 -C relocation-model=static" $(CARGO_CMD) --release -p be
	$(CARGO_CMD) --release -p bde
	@cp target/x86_64-unknown-none/release/usb_driver target/initrd/bin/usb_driver.beast
	@cp target/x86_64-unknown-none/release/hello target/initrd/bin/hello.beast
	@cp target/x86_64-unknown-none/release/compositor target/initrd/bin/compositor.beast
	@cp target/x86_64-unknown-none/release/io_uring_test target/initrd/bin/io_uring_test.beast
	@cp target/x86_64-unknown-none/release/shell target/initrd/bin/shell.beast
	@cp target/x86_64-unknown-none/release/be target/initrd/bin/be.beast
	@echo "📦 Creating initrd.tar..."
	@cd target/initrd && tar -cf ../initrd.tar bin dev home usr
	@echo "✅ Userland built"

# Create bootable ISO
iso: build
	@echo "📀 Creating bootable ISO..."
	@rm -f $(ISO_FILE)
	@rm -rf $(ISO_DIR)
	@mkdir -p $(ISO_DIR)/boot/limine
	@cp $(KERNEL_BINARY) $(ISO_DIR)/boot/beast.kernel
	@cp target/initrd.tar $(ISO_DIR)/boot/
	@cp boot/limine/limine.conf $(ISO_DIR)/boot/limine/
	@cp boot/limine/limine.sys $(ISO_DIR)/boot/limine/
	@cp boot/limine/limine-bios.sys $(ISO_DIR)/boot/limine/
	@cp boot/limine/limine-bios-cd.bin $(ISO_DIR)/boot/limine/
	@cp boot/limine/limine-uefi-cd.bin $(ISO_DIR)/boot/limine/
	xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image \
		--protective-msdos-label \
		-o $(ISO_FILE) $(ISO_DIR)
	./boot/limine/limine bios-install $(ISO_FILE)
	@echo "✅ ISO created: $(ISO_FILE)"

# Run in QEMU (headless, serial I/O — type commands after boot)
# First time:  make run   (builds everything)
# After that:  make go    (runs QEMU immediately)
# Quit QEMU:   Ctrl+A then X
run: iso
	$(QEMU) -m $(QEMU_MEMORY) $(QEMU_FLAGS) -cdrom $(ISO_FILE) -device isa-debug-exit,iobase=0xf4,iosize=0x04

go: beast-os.iso
	$(QEMU) -m $(QEMU_MEMORY) $(QEMU_FLAGS) -cdrom $(ISO_FILE) -device isa-debug-exit,iobase=0xf4,iosize=0x04

# Run with graphical QEMU window (requires WSLg or X server)
graphical: iso
	@echo "🚀 Launching Beast OS in QEMU (graphical)..."
	$(QEMU) -m $(QEMU_MEMORY) -serial stdio -no-reboot -no-shutdown -cdrom $(ISO_FILE) -device isa-debug-exit,iobase=0xf4,iosize=0x04
	
# Run with GDB debugging
debug: iso
	@echo "🐛 Launching Beast OS in QEMU with GDB server on port 1234..."
	$(QEMU) -m $(QEMU_MEMORY) $(QEMU_FLAGS) -cdrom $(ISO_FILE) -s -S -device isa-debug-exit,iobase=0xf4,iosize=0x04

# Run tests
test:
	@echo "🧪 Running tests..."
	cargo test --workspace
	@echo "✅ Tests passed"

# Run benchmarks
bench:
	@echo "📊 Running benchmarks..."
	cargo bench --workspace
	@echo "✅ Benchmarks complete"

# Clean build artifacts
clean:
	@echo "🧹 Cleaning..."
	cargo clean
	@rm -rf $(ISO_DIR)/boot/*
	@rm -f $(ISO_FILE)
	@rm -rf target/initrd
	@echo "✅ Clean"

# Deep clean (remove all generated files)
clean-all: clean
	@echo "🧹 Deep cleaning..."
	@rm -rf target
	@rm -rf $(ISO_DIR)
	@echo "✅ Deep clean complete"

# Help
help:
	@echo "Beast OS Build Commands:"
	@echo ""
	@echo "  make           - Build kernel, drivers, userland, and ISO"
	@echo "  make build     - Build kernel, drivers, and userland only"
	@echo "  make iso       - Create bootable ISO"
	@echo "  make run       - Run in QEMU"
	@echo "  make debug     - Run with GDB debugging (-s -S)"
	@echo "  make test      - Run tests"
	@echo "  make bench     - Run benchmarks"
	@echo "  make clean     - Remove build artifacts"
	@echo "  make clean-all - Deep clean (remove everything)"
	@echo "  make help      - Show this help message"
	@echo ""
	@echo "Requirements:"
	@echo "  - Rust nightly"
	@echo "  - cargo"
	@echo "  - qemu-system-x86_64"
	@echo "  - xorriso"
	@echo "  - limine bootloader"