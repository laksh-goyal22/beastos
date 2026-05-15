//! PS/2 Mouse Driver
//!
//! Handles relative movement and button states from IRQ12 (vector 44).

use core::arch::asm;
use crate::kprintln;
use crate::sync::Spinlock;

const MOUSE_DATA_PORT: u16 = 0x60;
const MOUSE_STATUS_PORT: u16 = 0x64;

#[derive(Debug, Clone, Copy, Default)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub left_button: bool,
    pub right_button: bool,
    pub middle_button: bool,
    pub bounds_width: i32,
    pub bounds_height: i32,
}

pub static MOUSE_STATE: Spinlock<MouseState> = Spinlock::new(MouseState {
    x: 400,
    y: 300,
    left_button: false,
    right_button: false,
    middle_button: false,
    bounds_width: 800,
    bounds_height: 600,
});

static mut MOUSE_CYCLE: u8 = 0;
static mut MOUSE_PACKET: [u8; 3] = [0; 3];

/// Initialize the PS/2 mouse controller.
pub fn init() {
    unsafe {
        // Enable the auxiliary mouse device
        mouse_wait(1);
        outb(MOUSE_STATUS_PORT, 0xA8);

        // Enable interrupts
        mouse_wait(1);
        outb(MOUSE_STATUS_PORT, 0x20);
        mouse_wait(0);
        let status = inb(MOUSE_DATA_PORT) | 2;
        mouse_wait(1);
        outb(MOUSE_STATUS_PORT, 0x60);
        mouse_wait(1);
        outb(MOUSE_DATA_PORT, status);

        // Use default settings
        mouse_write(0xF6);
        mouse_read();

        // Enable data reporting
        mouse_write(0xF4);
        mouse_read();
    }
    kprintln!("    PS/2 mouse ready");
}

pub fn handle_mouse_interrupt() {
    let status = unsafe { inb(MOUSE_STATUS_PORT) };
    if status & 0x01 == 0 || status & 0x20 == 0 {
        return; // No data or not from mouse
    }

    let data = unsafe { inb(MOUSE_DATA_PORT) };
    
    unsafe {
        match MOUSE_CYCLE {
            0 => {
                MOUSE_PACKET[0] = data;
                // Bit 3 should always be 1 for the first byte of a packet
                if data & 0x08 != 0 {
                    MOUSE_CYCLE = 1;
                }
            }
            1 => {
                MOUSE_PACKET[1] = data;
                MOUSE_CYCLE = 2;
            }
            2 => {
                MOUSE_PACKET[2] = data;
                MOUSE_CYCLE = 0;
                process_packet();
            }
            _ => MOUSE_CYCLE = 0,
        }
    }
}

fn process_packet() {
    let packet = unsafe { MOUSE_PACKET };
    let mut state = MOUSE_STATE.lock();

    state.left_button = (packet[0] & 0x01) != 0;
    state.right_button = (packet[0] & 0x02) != 0;
    state.middle_button = (packet[0] & 0x04) != 0;

    let x_sign = (packet[0] & 0x10) != 0;
    let y_sign = (packet[0] & 0x20) != 0;

    let mut dx = packet[1] as i32;
    if x_sign { dx |= !0xFF; }
    
    let mut dy = packet[2] as i32;
    if y_sign { dy |= !0xFF; }

    let bw = state.bounds_width.max(1);
    let bh = state.bounds_height.max(1);
    state.x = (state.x + dx).clamp(0, bw - 1);
    state.y = (state.y - dy).clamp(0, bh - 1);
}

unsafe fn mouse_wait(a_type: u8) {
    let mut timeout = 100000;
    if a_type == 0 {
        while timeout > 0 {
            if (inb(MOUSE_STATUS_PORT) & 1) == 1 { return; }
            timeout -= 1;
        }
    } else {
        while timeout > 0 {
            if (inb(MOUSE_STATUS_PORT) & 2) == 0 { return; }
            timeout -= 1;
        }
    }
}

unsafe fn mouse_write(a_write: u8) {
    mouse_wait(1);
    outb(MOUSE_STATUS_PORT, 0xD4);
    mouse_wait(1);
    outb(MOUSE_DATA_PORT, a_write);
}

unsafe fn mouse_read() -> u8 {
    mouse_wait(0);
    inb(MOUSE_DATA_PORT)
}

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    val
}
