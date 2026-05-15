//! AHCI SATA Driver — Real Port-Based DMA I/O
//!
//! Implements read_sector / write_sector using the AHCI HBA command list
//! with PRDT entries. Targets QEMU's AHCI (ICH9) and real hardware.

use crate::memory::pmm;
use crate::memory::vmm::phys_to_virt;
use crate::kprintln;
use core::ptr;

// PCI config space
const PCI_BAR5: u8 = 0x24;

// ABAR offsets
const GHC_OFFSET: u64 = 0x04;
const PI_OFFSET: u64 = 0x0C;

// Port register stride & base
const PORT_OFFSET: u64 = 0x100;
const PORT_STRIDE: u64 = 0x80;

// Port register offsets
const P_CLB: u64 = 0x00;
const P_CLBU: u64 = 0x04;
const P_FB: u64 = 0x08;
const P_FBU: u64 = 0x0C;
const P_IS: u64 = 0x10;
const _P_IE: u64 = 0x14;
const P_CMD: u64 = 0x18;
const P_TFD: u64 = 0x20;
const P_SIG: u64 = 0x24;
const P_SSTS: u64 = 0x28;
const P_SERR: u64 = 0x30;
const P_CI: u64 = 0x38;

// PxCMD bits
const CMD_ST: u32 = 1 << 0;
const CMD_FRE: u32 = 1 << 4;
const CMD_CR: u32 = 1 << 15;
const CMD_FR: u32 = 1 << 14;
const CMD_POD: u32 = 1 << 2;
const CMD_SUD: u32 = 1 << 1;

// ATA commands
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;

// H2D FIS type
const FIS_TYPE_H2D: u8 = 0x27;

// Command table layout constants
const CT_CFIS_OFFSET: u64 = 0x00;
const CT_PRDT_OFFSET: u64 = 0x80;

pub struct AhciDriver {
    abar_virt: u64,
    port_index: u8,
    clb_phys: u64,
    fb_phys: u64,
}

impl AhciDriver {
    pub fn init() -> Option<Self> {
        kprintln!("  [AHCI] Scanning PCI for SATA controller...");

        let bar5 = match Self::find_ahci_bar() {
            Some(b) => b,
            None => {
                kprintln!("  [AHCI] No PCI device found");
                return None;
            }
        };
        let abar_phys = bar5 & !0x3FF;
        let abar_virt = phys_to_virt(abar_phys);

        // Enable AHCI (GHC.AE = bit 31)
        unsafe {
            let ghc = (abar_virt + GHC_OFFSET) as *mut u32;
            ghc.write_volatile(ghc.read_volatile() | (1 << 31));
        }

        let ports = unsafe { ((abar_virt + PI_OFFSET) as *const u32).read_volatile() };
        kprintln!("  [AHCI] HBA at phys={:#x} virt={:#x}, ports={:#b}", abar_phys, abar_virt, ports);

        for i in 0..32u32 {
            if (ports & (1 << i)) == 0 {
                continue;
            }
            let pbase = abar_virt + PORT_OFFSET + (i as u64) * PORT_STRIDE;

            let ssts = unsafe { ((pbase + P_SSTS) as *const u32).read_volatile() };
            if (ssts & 0x0F) != 0x03 {
                continue;
            }

            let sig = unsafe { ((pbase + P_SIG) as *const u32).read_volatile() };
            kprintln!("  [AHCI] Port {}: sig={:#010x}, ssts={:#x}", i, sig, ssts);

            // Accept ATA drives (SATA drive sig typically 0x00000101 or 0xEB140101)
            if sig == 0x0000_0101 || sig == 0xEB14_0101 || (sig & 0xFFFF) == 0x0101 {
                let mut driver = Self {
                    abar_virt,
                    port_index: i as u8,
                    clb_phys: 0,
                    fb_phys: 0,
                };
                if driver.init_port(pbase) {
                    return Some(driver);
                }
            }
        }

        kprintln!("  [AHCI] No active SATA drive found");
        None
    }

    fn init_port(&mut self, pbase: u64) -> bool {
        // Allocate command list (32 entries × 32 bytes = 1KB)
        let clb_phys = match pmm::alloc_contiguous(2) {
            Some(p) => p,
            None => { kprintln!("  [AHCI] OOM for command list"); return false; }
        };
        unsafe {
            ptr::write_bytes(phys_to_virt(clb_phys) as *mut u8, 0, 2048);
        }

        // Allocate FIS receive area (256 bytes, use a page)
        let fb_phys = match pmm::alloc_page() {
            Some(p) => p,
            None => {
                kprintln!("  [AHCI] OOM for FIS area");
                pmm::free_page(clb_phys);
                pmm::free_page(clb_phys + 4096);
                return false;
            }
        };
        unsafe {
            ptr::write_bytes(phys_to_virt(fb_phys) as *mut u8, 0, 4096);
        }

        // Stop port
        unsafe {
            let cmd = (pbase + P_CMD) as *mut u32;
            cmd.write_volatile(cmd.read_volatile() & !(CMD_ST | CMD_FRE));
            let mut t = 1000000;
            while (cmd.read_volatile() & (CMD_FR | CMD_CR)) != 0 && t > 0 { t -= 1; }
        }

        // Clear interrupts and errors
        unsafe {
            ((pbase + P_IS) as *mut u32).write_volatile(!0u32);
            ((pbase + P_SERR) as *mut u32).write_volatile(!0u32);
        }

        // Set base addresses
        unsafe {
            ((pbase + P_CLB) as *mut u32).write_volatile((clb_phys & 0xFFFF_FFFF) as u32);
            ((pbase + P_CLBU) as *mut u32).write_volatile((clb_phys >> 32) as u32);
            ((pbase + P_FB) as *mut u32).write_volatile((fb_phys & 0xFFFF_FFFF) as u32);
            ((pbase + P_FBU) as *mut u32).write_volatile((fb_phys >> 32) as u32);
        }

        // Enable FIS receive, power on, spin up
        unsafe {
            let cmd = (pbase + P_CMD) as *mut u32;
            cmd.write_volatile(cmd.read_volatile() | CMD_FRE | CMD_POD | CMD_SUD);
            let mut t = 1000000;
            while (cmd.read_volatile() & CMD_FR) == 0 && t > 0 { t -= 1; }
            cmd.write_volatile(cmd.read_volatile() | CMD_ST);
            t = 1000000;
            while (cmd.read_volatile() & CMD_CR) != 0 && t > 0 { t -= 1; }
        }

        self.clb_phys = clb_phys;
        self.fb_phys = fb_phys;

        kprintln!("  [AHCI] Port {} initialized (CLB={:#x}, FB={:#x})", self.port_index, clb_phys, fb_phys);
        true
    }

    fn find_ahci_bar() -> Option<u64> {
        for bus in 0..256u32 {
            for device in 0..32u32 {
                let class = Self::pci_config_read(bus as u8, device as u8, 0, 0x08);
                let class_code = (class >> 16) & 0xFF;
                let subclass = (class >> 8) & 0xFF;
                if class_code == 0x01 && subclass == 0x06 {
                    let bar5 = Self::pci_config_read(bus as u8, device as u8, 0, PCI_BAR5) as u64;
                    kprintln!("  [AHCI] Found SATA controller at bus={} dev={} bar5={:#x}", bus, device, bar5);
                    return Some(bar5);
                }
            }
        }
        kprintln!("  [AHCI] PCI scan complete, no SATA controller found");
        None
    }

    fn pci_config_read(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
        let addr = 0x80000000u32
            | ((bus as u32) << 16)
            | ((dev as u32) << 11)
            | ((func as u32) << 8)
            | (offset as u32 & 0xFC);
        unsafe {
            core::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") addr, options(nomem, nostack));
            let value: u32;
            core::arch::asm!("in eax, dx", in("dx") 0xCFCu16, out("eax") value, options(nomem, nostack));
            value
        }
    }

    fn pbase(&self) -> u64 {
        self.abar_virt + PORT_OFFSET + (self.port_index as u64) * PORT_STRIDE
    }

    /// Build a H2D register FIS in the command table CFIS area
    fn fill_fis(ct_virt: u64, lba: u64, count: u16, is_write: bool) {
        let fis = ct_virt + CT_CFIS_OFFSET;
        unsafe {
            let base = fis as *mut u8;
            base.write_volatile(FIS_TYPE_H2D);
            base.add(1).write_volatile(if is_write { 0x81 } else { 0x80 });
            base.add(2).write_volatile(if is_write { ATA_WRITE_DMA_EXT } else { ATA_READ_DMA_EXT });
            base.add(3).write_volatile(0);
            base.add(4).write_volatile((lba >> 0) as u8);
            base.add(5).write_volatile((lba >> 8) as u8);
            base.add(6).write_volatile((lba >> 16) as u8);
            base.add(7).write_volatile(0x40);
            base.add(8).write_volatile((lba >> 24) as u8);
            base.add(9).write_volatile((lba >> 32) as u8);
            base.add(10).write_volatile((lba >> 40) as u8);
            base.add(11).write_volatile(0);
            base.add(12).write_volatile((count & 0xFF) as u8);
            base.add(13).write_volatile((count >> 8) as u8);
            base.add(14).write_volatile(0);
            base.add(15).write_volatile(0);
        }
    }

    /// Fill a PRDT entry at offset 0x80 in command table
    fn fill_prdt(ct_virt: u64, buf_phys: u64, count: u16) {
        let prdt = (ct_virt + CT_PRDT_OFFSET) as *mut u32;
        let byte_count = (count as u32) * 512;
        unsafe {
            prdt.write_volatile((buf_phys & 0xFFFF_FFFF) as u32);
            prdt.add(1).write_volatile((buf_phys >> 32) as u32);
            prdt.add(2).write_volatile(0);
            prdt.add(3).write_volatile((1u32 << 31) | (byte_count - 1));
        }
    }

    /// Fill command header slot 0 in the command list
    fn fill_cmd_header(clb_virt: u64, ct_phys: u64, is_write: bool) {
        unsafe {
            // DW0: bits 31:16 = PRDTL(1), bit 5 = W, bit 0 = CIB
            let dw0: u32 = (1u32 << 16) | (if is_write { 1u32 << 5 } else { 0u32 });
            (clb_virt as *mut u32).write_volatile(dw0);
            // DW1: PRDBC (written to by HBA)
            ((clb_virt + 4) as *mut u32).write_volatile(0);
            // DW2: CTBA (low 32 of command table phys)
            ((clb_virt + 8) as *mut u32).write_volatile((ct_phys & 0xFFFF_FFFF) as u32);
            // DW3: CTBAU (high 32)
            ((clb_virt + 12) as *mut u32).write_volatile((ct_phys >> 32) as u32);
            // DW4-DW7: reserved
            ((clb_virt + 16) as *mut u32).write_volatile(0);
            ((clb_virt + 20) as *mut u32).write_volatile(0);
            ((clb_virt + 24) as *mut u32).write_volatile(0);
            ((clb_virt + 28) as *mut u32).write_volatile(0);
        }
    }

    fn issue_command(&self, lba: u64, count: u16, buf_phys: u64, is_write: bool) -> bool {
        // Allocate command table (128-byte aligned; a page suffices)
        let ct_phys = match pmm::alloc_page() {
            Some(p) => p,
            None => return false,
        };
        let ct_virt = phys_to_virt(ct_phys);
        unsafe { ptr::write_bytes(ct_virt as *mut u8, 0, 4096); }

        Self::fill_fis(ct_virt, lba, count, is_write);
        Self::fill_prdt(ct_virt, buf_phys, count);

        let clb_virt = phys_to_virt(self.clb_phys);
        Self::fill_cmd_header(clb_virt, ct_phys, is_write);

        // Ensure all writes visible before issuing
        unsafe { core::arch::asm!("mfence", options(nomem, nostack)); }

        // Issue command via PxCI (bit 0 = slot 0)
        let pbase = self.pbase();
        unsafe {
            let ci = (pbase + P_CI) as *mut u32;
            ci.write_volatile(1);
        }

        // Wait for completion (PxCI bit 0 clears)
        let ci_ptr = (pbase + P_CI) as *const u32;
        let mut timeout = 50_000_000u64;
        while timeout > 0 {
            if (unsafe { ci_ptr.read_volatile() } & 1) == 0 {
                break;
            }
            timeout -= 1;
        }

        // Check error
        let tfd = unsafe { ((pbase + P_TFD) as *const u32).read_volatile() };
        let err = (tfd >> 8) & 0xFF;

        pmm::free_page(ct_phys);

        if timeout == 0 {
            kprintln!("  [AHCI] Timeout lba={} write={}", lba, is_write);
            unsafe { ((pbase + P_CI) as *mut u32).write_volatile(1) };
            return false;
        }
        if err != 0 {
            kprintln!("  [AHCI] Error {:#x} lba={} write={}", err, lba, is_write);
            return false;
        }
        true
    }

    pub fn read_sector(&self, lba: u64, buffer: &mut [u8; 512]) -> bool {
        let buf_phys = match pmm::alloc_page() {
            Some(p) => p,
            None => return false,
        };
        let ok = self.issue_command(lba, 1, buf_phys, false);
        if ok {
            unsafe {
                ptr::copy_nonoverlapping(phys_to_virt(buf_phys) as *const u8, buffer.as_mut_ptr(), 512);
            }
        }
        pmm::free_page(buf_phys);
        ok
    }

    pub fn write_sector(&self, lba: u64, buffer: &[u8; 512]) -> bool {
        let buf_phys = match pmm::alloc_page() {
            Some(p) => p,
            None => return false,
        };
        unsafe {
            ptr::copy_nonoverlapping(buffer.as_ptr(), phys_to_virt(buf_phys) as *mut u8, 512);
        }
        let ok = self.issue_command(lba, 1, buf_phys, true);
        pmm::free_page(buf_phys);
        ok
    }
}

impl Drop for AhciDriver {
    fn drop(&mut self) {
        if self.clb_phys != 0 {
            pmm::free_page(self.clb_phys);
            pmm::free_page(self.clb_phys + 4096);
        }
        if self.fb_phys != 0 {
            pmm::free_page(self.fb_phys);
        }
    }
}
