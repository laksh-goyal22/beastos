//! Hardware Drivers for Beast OS
//!
//! Ring 0 driver shims — minimal hardware access layer.

pub mod pic;
pub mod pit;
pub mod keyboard;
pub mod mouse;
pub mod framebuffer;
pub mod console;
pub mod ahci;

use crate::kprintln;

/// Global AHCI driver instance, initialised after PCI scan.
pub static AHCI: spin::Once<ahci::AhciDriver> = spin::Once::new();

/// Initialize all hardware drivers.
pub fn init() {
    kprintln!("  [DRV] Initializing PIC...");
    pic::init();

    kprintln!("  [DRV] Initializing PIT timer...");
    pit::init(100); // 100 Hz tick

    kprintln!("  [DRV] Enabling keyboard...");
    keyboard::init();

    kprintln!("  [DRV] Enabling mouse...");
    mouse::init();

    kprintln!("  [DRV] Initializing serial port...");
    crate::logger::init();

    kprintln!("  [DRV] Initializing console...");
    let width = framebuffer::width();
    let height = framebuffer::height();
    if width > 0 && height > 0 {
        console::CONSOLE.lock().init(width, height);
        // Update mouse constraints
        let mut mouse = mouse::MOUSE_STATE.lock();
        mouse.x = (width / 2) as i32;
        mouse.y = (height / 2) as i32;
        mouse.bounds_width = width as i32;
        mouse.bounds_height = height as i32;
    }

    kprintln!("  [DRV] Initializing AHCI...");
    if let Some(ahci) = ahci::AhciDriver::init() {
        AHCI.call_once(|| ahci);
        kprintln!("  [DRV] AHCI ready");
    }

    kprintln!("  [DRV] Drivers initialized");
}
