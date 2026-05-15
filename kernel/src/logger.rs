//! Kernel logger — serial console output
//!
//! Provides `kprintln!` macro for kernel debug output via serial port.

use core::fmt::{self, Write};
use spin::Mutex;

/// Global serial writer, protected by spinlock.
static SERIAL_WRITER: Mutex<SerialWriter> = Mutex::new(SerialWriter { port: 0x3F8 });

/// Minimal serial port writer (8250 UART).
struct SerialWriter {
    port: u16,
}

impl SerialWriter {
    /// Write a byte to the serial port.
    fn write_byte(&self, byte: u8) {
        unsafe {
            // Wait for transmit buffer empty (bit 5 of LSR)
            while (port_read(self.port + 5) & 0x20) == 0 {}
            port_write(self.port, byte);
        }
    }

    /// Initialize the serial port (115200 baud, 8N1).
    pub fn init(&self) {
        unsafe {
            port_write(self.port + 1, 0x00); // Disable interrupts
            port_write(self.port + 3, 0x80); // Enable DLAB
            port_write(self.port + 0, 0x01); // Divisor lo: 115200 baud
            port_write(self.port + 1, 0x00); // Divisor hi
            port_write(self.port + 3, 0x03); // 8 bits, no parity, 1 stop
            port_write(self.port + 2, 0xC7); // Enable FIFO, clear, 14-byte
            port_write(self.port + 4, 0x0B); // IRQs enabled, RTS/DSR set
            port_write(self.port + 1, 0x01); // Enable "Data Ready" interrupt
        }
    }
}

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

/// Initialize the kernel logger (serial port).
pub fn init() {
    SERIAL_WRITER.lock().init();
}

/// Internal print function — outputs to serial AND framebuffer console.
#[doc(hidden)]
pub fn _kprint(args: fmt::Arguments) {
    // Use try_lock() to avoid deadlocks when called from page fault handler
    // or other interrupt contexts that might already hold the lock.
    if let Some(mut serial) = SERIAL_WRITER.try_lock() {
        serial.write_fmt(args).ok();
    }
    // Also write to framebuffer console if available.
    if let Some(mut console) = crate::drivers::console::CONSOLE.try_lock() {
        if console.width > 0 {
            console.write_fmt(args).ok();
        }
    }
}

/// Print to serial console.
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => ($crate::logger::_kprint(format_args!($($arg)*)));
}

/// Print line to serial console.
#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => ($crate::kprint!("{}\n", format_args!($($arg)*)));
}

// ---------------------------------------------------------------------------
// Port I/O helpers
// ---------------------------------------------------------------------------

#[inline(always)]
unsafe fn port_write(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}

#[inline(always)]
unsafe fn port_read(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack));
    value
}
