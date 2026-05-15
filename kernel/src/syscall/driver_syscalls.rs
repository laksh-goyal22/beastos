use crate::ipc::spsc_with_boost::BeastSPSCRing;
use crate::kprintln;
use alloc::vec::Vec;
use crate::sync::Spinlock;
use crate::scheduler;

// Syscall numbers for driver operations
pub const SCALL_MAP_MMIO: u64 = 200;
pub const SCALL_BIND_IRQ: u64 = 201;
pub const SCALL_UNBIND_IRQ: u64 = 202;
pub const SCALL_CREATE_IRQ_RING: u64 = 202;
pub const SCALL_PORT_IO_IN: u64 = 203;
pub const SCALL_PORT_IO_OUT: u64 = 204;

const PAGE_SIZE: u64 = 4096;

// Track bound interrupts per task
struct IrqBinding {
    irq: u8,
    ring_id: usize,
    _task_id: usize,
}

static IRQ_BINDINGS: Spinlock<Vec<IrqBinding>> = Spinlock::new(Vec::new());
static IRQ_RINGS: Spinlock<Vec<BeastSPSCRing<InterruptEvent, 256>>> = Spinlock::new(Vec::new());

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct InterruptEvent {
    pub irq: u8,
    pub timestamp: u64,
    pub counter: u32,
}

impl Default for InterruptEvent {
    fn default() -> Self {
        Self {
            irq: 0,
            timestamp: 0,
            counter: 0,
        }
    }
}

// Simple helper to read TSC
fn rdtsc() -> u64 {
    let mut low: u32;
    let mut high: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack));
    }
    ((high as u64) << 32) | (low as u64)
}

// Basic Port I/O helpers
unsafe fn inb(port: u16) -> u8 {
    let result: u8;
    core::arch::asm!("in al, dx", out("al") result, in("dx") port, options(nomem, nostack, preserves_flags));
    result
}

unsafe fn inw(port: u16) -> u16 {
    let result: u16;
    core::arch::asm!("in ax, dx", out("ax") result, in("dx") port, options(nomem, nostack, preserves_flags));
    result
}

unsafe fn inl(port: u16) -> u32 {
    let result: u32;
    core::arch::asm!("in eax, dx", out("eax") result, in("dx") port, options(nomem, nostack, preserves_flags));
    result
}

unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

unsafe fn outw(port: u16, value: u16) {
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
}

unsafe fn outl(port: u16, value: u32) {
    core::arch::asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
}

// Syscall: Map MMIO region into driver's address space
pub fn sys_map_mmio(phys_addr: u64, size: u64) -> Result<u64, i32> {
    let current_id = scheduler::current_slot();
    
    if size == 0 || size % PAGE_SIZE != 0 {
        return Err(-22); // EINVAL
    }
    
    // In a real OS, validate it's an MMIO region and the process has permissions.
    // For now, we trust the caller and just map it directly to a chosen virtual address.
    // We'll map it to a high virtual address in user space just to be safe.
    let virt_addr = 0x0000_7000_0000_0000 + phys_addr; 
    
    if let Some(vmm) = crate::memory::vmm::VMM.lock().as_mut() {
        for offset in (0..size).step_by(PAGE_SIZE as usize) {
            let _ = vmm.map_page(virt_addr + offset, phys_addr + offset, true);
        }
    }
    
    kprintln!("[DRIVER] Mapped MMIO {:#x} -> {:#x} for task {}", phys_addr, virt_addr, current_id);
    Ok(virt_addr)
}

// Syscall: Bind IRQ to current task via SPSC ring
pub fn sys_bind_irq(irq: u8, ring_id: usize) -> Result<(), i32> {
    let current_id = scheduler::current_slot();
    
    // Validate IRQ is not already bound
    {
        let bindings = IRQ_BINDINGS.lock();
        if bindings.iter().any(|b| b.irq == irq) {
            return Err(-16); // EBUSY
        }
    }
    
    // Validate ring exists
    let rings = IRQ_RINGS.lock();
    if ring_id >= rings.len() {
        return Err(-2); // ENOENT
    }
    
    // Register binding
    let binding = IrqBinding {
        irq,
        ring_id,
        _task_id: current_id,
    };
    IRQ_BINDINGS.lock().push(binding);
    
    kprintln!("[DRIVER] IRQ {} bound to task {} via ring {}", irq, current_id, ring_id);
    Ok(())
}

// Syscall: Create new SPSC ring for interrupts
pub fn sys_create_irq_ring(_capacity: usize) -> Result<usize, i32> {
    let mut rings = IRQ_RINGS.lock();
    let ring_id = rings.len();
    
    let ring = BeastSPSCRing::<InterruptEvent, 256>::new();
    let current_id = scheduler::current_slot();
    ring.set_consumer(current_id);
    rings.push(ring);
    
    let ring_ptr = &rings[ring_id] as *const _ as u64;
    if let Some(vmm) = crate::memory::vmm::VMM.lock().as_mut() {
        let phys = crate::memory::vmm::virt_to_phys(ring_ptr);
        let _ = vmm.map_page(ring_ptr, phys, true);
    }
    
    kprintln!("[DRIVER] Created IRQ ring {} at {:#x} for task {}", ring_id, ring_ptr, current_id);
    Ok(ring_ptr as usize) // returning pointer as ring_id for userspace to access directly
}

// Syscall: Legacy I/O port input
pub fn sys_port_in(port: u16) -> Result<u32, i32> {
    let value = unsafe {
        match port {
            0x60..=0x6F => inb(port) as u32,
            0x70..=0x77 => inw(port) as u32,
            _ => inl(port),
        }
    };
    Ok(value)
}

// Syscall: Legacy I/O port output
pub fn sys_port_out(port: u16, value: u32, width: u8) -> Result<(), i32> {
    unsafe {
        match width {
            1 => outb(port, value as u8),
            2 => outw(port, value as u16),
            4 => outl(port, value),
            _ => return Err(-22),
        }
    }
    Ok(())
}

// Helper: Forward interrupt to bound ring (called from IDT)
pub fn forward_interrupt_to_ring(irq: u8) -> bool {
    let bindings = IRQ_BINDINGS.lock();
    
    for binding in bindings.iter() {
        if binding.irq == irq {
            if let Some(ring) = IRQ_RINGS.lock().get(binding.ring_id) {
                let event = InterruptEvent {
                    irq,
                    timestamp: rdtsc(),
                    counter: 0,
                };
                
                let _ = ring.enqueue(event);
            }
            return true;
        }
    }
    false
}
