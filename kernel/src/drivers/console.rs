//! Framebuffer Console Driver
//!
//! Provides a simple scrolling text console over the framebuffer.

use core::fmt;
use spin::Mutex;
use crate::drivers::framebuffer::{clear, draw_char, fill_rect};

/// A basic 8x8 text console overlaying the framebuffer.
pub struct Console {
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub fg_color: u32,
    pub bg_color: u32,
    pub width: u32,
    pub height: u32,
}

pub static CONSOLE: Mutex<Console> = Mutex::new(Console {
    cursor_x: 0,
    cursor_y: 0,
    fg_color: 0xFF000000, // Deep Black
    bg_color: 0xFF33FF33, // Radioactive Green
    width: 0,            // Updated on init
    height: 0,           // Updated on init
});

impl Console {
    /// Initialize the console dimensions
    pub fn init(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.cursor_x = 0;
        self.cursor_y = 0;
        clear(self.bg_color);
    }

    /// Write a single byte/character to the screen
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.cursor_x = 0,
            8 => {
                // Backspace
                if self.cursor_x >= 8 {
                    self.cursor_x -= 8;
                    fill_rect(self.cursor_x, self.cursor_y, 8, 8, self.bg_color);
                }
            }
            byte => {
                if self.cursor_x + 8 > self.width {
                    self.newline();
                }
                draw_char(
                    self.cursor_x,
                    self.cursor_y,
                    byte as char,
                    self.fg_color,
                    self.bg_color,
                );
                self.cursor_x += 8;
            }
        }
    }

    /// Move to the next line
    pub fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 8;
        if self.cursor_y + 8 > self.height {
            // For now, clear the screen when we reach the bottom instead of scrolling
            self.cursor_y = 0;
            clear(self.bg_color);
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
