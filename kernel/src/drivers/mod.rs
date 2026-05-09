//! Hardware Drivers for Beast OS
//!
//! Ring 0 driver shims — minimal hardware access layer.

pub mod pic;
pub mod pit;
pub mod keyboard;
pub mod framebuffer;
pub mod console;

use crate::kprintln;

/// Initialize all hardware drivers.
pub fn init() {
    kprintln!("  [DRV] Initializing PIC...");
    pic::init();

    kprintln!("  [DRV] Initializing PIT timer...");
    pit::init(100); // 100 Hz tick

    kprintln!("  [DRV] Enabling keyboard...");
    keyboard::init();

    kprintln!("  [DRV] Initializing console...");
    let width = framebuffer::width();
    let height = framebuffer::height();
    if width > 0 && height > 0 {
        console::CONSOLE.lock().init(width, height);
    }

    kprintln!("  [DRV] Drivers initialized");
}
