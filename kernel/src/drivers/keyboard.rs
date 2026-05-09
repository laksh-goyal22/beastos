//! PS/2 Keyboard Driver
//!
//! Handles scancode set 1 from IRQ1 (vector 33).

use core::arch::asm;
use crate::kprintln;

const KEYBOARD_DATA_PORT: u16 = 0x60;
const KEYBOARD_STATUS_PORT: u16 = 0x64;

/// Scancode-to-ASCII lookup table (US QWERTY, scancode set 1, lowercase).
static SCANCODE_MAP: [u8; 128] = {
    let mut map = [0u8; 128];
    map[0x02] = b'1'; map[0x03] = b'2'; map[0x04] = b'3'; map[0x05] = b'4';
    map[0x06] = b'5'; map[0x07] = b'6'; map[0x08] = b'7'; map[0x09] = b'8';
    map[0x0A] = b'9'; map[0x0B] = b'0'; map[0x0C] = b'-'; map[0x0D] = b'=';
    map[0x0E] = 0x08; // Backspace
    map[0x0F] = b'\t';
    map[0x10] = b'q'; map[0x11] = b'w'; map[0x12] = b'e'; map[0x13] = b'r';
    map[0x14] = b't'; map[0x15] = b'y'; map[0x16] = b'u'; map[0x17] = b'i';
    map[0x18] = b'o'; map[0x19] = b'p'; map[0x1A] = b'['; map[0x1B] = b']';
    map[0x1C] = b'\n'; // Enter
    map[0x1E] = b'a'; map[0x1F] = b's'; map[0x20] = b'd'; map[0x21] = b'f';
    map[0x22] = b'g'; map[0x23] = b'h'; map[0x24] = b'j'; map[0x25] = b'k';
    map[0x26] = b'l'; map[0x27] = b';'; map[0x28] = b'\'';
    map[0x29] = b'`';
    map[0x2B] = b'\\';
    map[0x2C] = b'z'; map[0x2D] = b'x'; map[0x2E] = b'c'; map[0x2F] = b'v';
    map[0x30] = b'b'; map[0x31] = b'n'; map[0x32] = b'm'; map[0x33] = b',';
    map[0x34] = b'.'; map[0x35] = b'/';
    map[0x39] = b' '; // Space
    map
};

/// Initialize the PS/2 keyboard controller.
pub fn init() {
    // Flush any pending scancodes
    unsafe {
        while (inb(KEYBOARD_STATUS_PORT) & 1) != 0 {
            let _ = inb(KEYBOARD_DATA_PORT);
        }
    }
    kprintln!("    PS/2 keyboard ready");
}

/// Called from the keyboard IRQ handler (vector 33).
/// Returns the ASCII character if it's a key-press (not release).
pub fn handle_scancode() -> Option<u8> {
    let scancode = unsafe { inb(KEYBOARD_DATA_PORT) };

    // Ignore key releases (bit 7 set)
    if scancode & 0x80 != 0 {
        return None;
    }

    let ascii = SCANCODE_MAP.get(scancode as usize).copied().unwrap_or(0);
    if ascii != 0 {
        Some(ascii)
    } else {
        None
    }
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    val
}
