//! Framebuffer Console Driver
//!
//! Provides a simple scrolling text console over the framebuffer.

use core::fmt;
use spin::Mutex;
use crate::drivers::framebuffer::{clear, fill_rect};

/// A basic 8x8 text console overlaying the framebuffer.
pub struct Console {
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub fg_color: u32,
    pub bg_color: u32,
    pub width: u32,
    pub height: u32,
    pub blink_state: bool,
    pub tick_count: u32,
    cursor_saved: [u32; 256], // char_w * char_h max (16*16 for scale=2)
    cursor_was_visible: bool,
}

pub static CONSOLE: Mutex<Console> = Mutex::new(Console {
    cursor_x: 0,
    cursor_y: 0,
    fg_color: 0xFF000000, // Deep Black
    bg_color: 0xFF33FF33, // Radioactive Green
    width: 0,            // Updated on init
    height: 0,           // Updated on init
    blink_state: false,
    tick_count: 0,
    cursor_saved: [0u32; 256],
    cursor_was_visible: false,
});

impl Console {
    /// Initialize the console dimensions
    pub fn init(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.blink_state = false;
        self.tick_count = 0;
        self.cursor_was_visible = false;
        clear(self.bg_color);
    }

    pub fn tick(&mut self) {
        if self.width == 0 { return; }
        self.tick_count += 1;
        // Blink every 50 ticks (0.5s at 100Hz)
        if self.tick_count % 50 == 0 {
            self.blink_state = !self.blink_state;
            if self.blink_state {
                // Cursor turning ON: save pixels and draw cursor
                self.save_cursor_area();
                self.draw_cursor();
            } else {
                // Cursor turning OFF: restore saved pixels
                self.restore_cursor_area();
            }
        }
    }

    fn save_cursor_area(&mut self) {
        let cw = self.char_w();
        let ch = self.char_h();
        let n = crate::drivers::framebuffer::read_pixels(
            self.cursor_x, self.cursor_y, cw, ch, &mut self.cursor_saved,
        );
        self.cursor_saved[n..].fill(self.bg_color);
    }

    fn restore_cursor_area(&self) {
        crate::drivers::framebuffer::blit_rect(
            self.cursor_x, self.cursor_y, self.char_w(), self.char_h(), &self.cursor_saved,
        );
    }

    fn draw_cursor(&self) {
        if self.width == 0 { return; }
        // Block cursor: green overlay at cursor position
        let cursor_color = 0x80_33FF33u32;
        fill_rect(self.cursor_x, self.cursor_y, self.char_w(), self.char_h(), cursor_color);
    }

    fn char_w(&self) -> u32 { 8 * 2 }
    fn char_h(&self) -> u32 { 8 * 2 }

    /// Write a single byte/character to the screen
    pub fn write_byte(&mut self, byte: u8) {
        let char_w = self.char_w();
        let char_h = self.char_h();

        match byte {
            b'\n' => self.newline(),
            b'\r' => self.cursor_x = 0,
            b'\t' => {
                let tab = char_w * 8;
                let next = ((self.cursor_x / tab) + 1) * tab;
                self.cursor_x = next.min(self.width.saturating_sub(char_w));
            }
            8 | 127 => {
                if self.cursor_x >= char_w {
                    self.cursor_x -= char_w;
                    self.restore_cursor_area();
                    fill_rect(self.cursor_x, self.cursor_y, char_w, char_h, self.bg_color);
                }
            }
            byte => {
                if self.cursor_x + char_w > self.width {
                    self.newline();
                }
                self.restore_cursor_area();
                fill_rect(self.cursor_x, self.cursor_y, char_w, char_h, self.bg_color);

                crate::drivers::framebuffer::draw_char_scaled(
                    self.cursor_x,
                    self.cursor_y,
                    byte as char,
                    self.fg_color,
                    self.bg_color,
                    2,
                );
                self.cursor_x += char_w;
            }
        }
    }

    /// Move to the next line; scroll when the screen is full.
    pub fn newline(&mut self) {
        self.restore_cursor_area();
        let char_h = self.char_h();
        self.cursor_x = 0;
        self.cursor_y += char_h;
        if self.cursor_y + char_h > self.height {
            crate::drivers::framebuffer::scroll_up(char_h, self.bg_color);
            self.cursor_y = self.height.saturating_sub(char_h);
        }
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    CONSOLE.lock().write_fmt(args).unwrap();
}

/// Print to framebuffer console.
#[macro_export]
macro_rules! fb_print {
    ($($arg:tt)*) => ($crate::drivers::console::_print(format_args!($($arg)*)));
}

/// Print line to framebuffer console.
#[macro_export]
macro_rules! fb_println {
    () => ($crate::fb_print!("\n"));
    ($($arg:tt)*) => ($crate::fb_print!("{}\n", format_args!($($arg)*)));
}

pub fn handle_serial_interrupt() {
    // Read all available bytes from the UART FIFO (PIC edge-triggered,
    // so drain in one shot to avoid losing bytes).
    unsafe {
        while (inb(0x3F8 + 5) & 1) != 0 {
            let data = inb(0x3F8);
            crate::syscall::push_keyboard_char(data);
        }
    }
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    val
}
