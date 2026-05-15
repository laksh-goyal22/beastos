use core::sync::atomic::{AtomicU32, Ordering};

pub const IOURING_MAX_ENTRIES: u32 = 128;
pub const SQE_SIZE: u64 = 40;
pub const CQE_SIZE: u64 = 24;
pub const RING_HEADER_SIZE: u64 = 32;

pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READ: u8 = 1;
pub const IORING_OP_WRITE: u8 = 2;

pub const SQ_HEAD_OFF: u64 = 0;
pub const SQ_TAIL_OFF: u64 = 4;
pub const SQ_ENTRIES_OFF: u64 = 8;
pub const CQ_HEAD_OFF: u64 = 12;
pub const CQ_TAIL_OFF: u64 = 16;
pub const CQ_ENTRIES_OFF: u64 = 20;

/// SQ page layout (mapped at user address 0):
///   [0..4):   sq_head (u32, written by kernel)
///   [4..8):   sq_tail (u32, written by user)
///   [8..12):  sq_entries (u32, readonly)
///   [12..16): cq_head (u32, written by user)
///   [16..20): cq_tail (u32, written by kernel)
///   [20..24): cq_entries (u32, readonly)
///   [24..32): reserved
///   [32..):   SQEs

/// CQ page layout (mapped at user address 0x1000):
///   [0..4):   cq_head (u32, written by user)
///   [4..8):   cq_tail (u32, written by kernel)
///   [8..12):  cq_entries (u32, readonly)
///   [12..16): flags (u32)
///   [16..):   CQEs

#[repr(C)]
pub struct IoUringSQEntry {
    pub opcode: u8,
    pub flags: u8,
    _pad: [u8; 6],
    pub fd: u64,
    pub addr: u64,
    pub len: u64,
    pub user_data: u64,
}

#[repr(C)]
pub struct IoUringCQEntry {
    pub user_data: u64,
    pub result: i64,
    pub flags: u32,
}

pub struct IoUring {
    pub sq_phys: u64,
    pub cq_phys: u64,
    pub sq_kvirt: u64,
    pub cq_kvirt: u64,
    pub entries: u32,
}

impl IoUring {
    pub fn new(entries: u32) -> Option<Self> {
        let entries = entries.min(IOURING_MAX_ENTRIES).max(1);
        let sq_phys = crate::memory::pmm::alloc_page()?;
        let cq_phys = crate::memory::pmm::alloc_page()?;
        let sq_kvirt = crate::memory::vmm::phys_to_virt(sq_phys);
        let cq_kvirt = crate::memory::vmm::phys_to_virt(cq_phys);
        unsafe {
            core::ptr::write_bytes(sq_kvirt as *mut u8, 0, 4096);
            core::ptr::write_bytes(cq_kvirt as *mut u8, 0, 4096);
            let sq = sq_kvirt as *mut u32;
            *sq.add(2) = entries;  // sq_entries
            *sq.add(5) = entries;  // cq_entries
        }
        Some(Self { sq_phys, cq_phys, sq_kvirt, cq_kvirt, entries })
    }

    unsafe fn sq_head(&self) -> u32 {
        let p = self.sq_kvirt as *const u32;
        core::ptr::read_volatile(p)
    }

    unsafe fn sq_tail(&self) -> u32 {
        let p = (self.sq_kvirt + 4) as *const u32;
        core::ptr::read_volatile(p)
    }

    unsafe fn set_sq_head(&self, val: u32) {
        let p = self.sq_kvirt as *mut u32;
        p.write_volatile(val);
    }

    unsafe fn cq_head(&self) -> u32 {
        let p = (self.cq_kvirt) as *const u32;
        core::ptr::read_volatile(p)
    }

    unsafe fn cq_tail(&self) -> u32 {
        let p = (self.cq_kvirt + 4) as *const u32;
        core::ptr::read_volatile(p)
    }

    unsafe fn set_cq_tail(&self, val: u32) {
        let p = (self.cq_kvirt + 4) as *mut u32;
        p.write_volatile(val);
    }

    pub fn submit_sqes(&mut self) -> u32 {
        unsafe {
            let head = self.sq_head();
            let tail = self.sq_tail();
            if head == tail { return 0; }
            let sqe_base = self.sq_kvirt + RING_HEADER_SIZE;
            let mut submitted = 0;
            let mut idx = head;
            while idx != tail {
                let sqe = &*((sqe_base + (idx as u64 % self.entries as u64) * SQE_SIZE) as *const IoUringSQEntry);
                let result = match sqe.opcode {
                    IORING_OP_READ => crate::syscall::sys_read(sqe.fd, sqe.addr, sqe.len),
                    IORING_OP_WRITE => crate::syscall::sys_write(sqe.fd, sqe.addr, sqe.len),
                    _ => -1,
                };
                self.push_cqe(sqe.user_data, result, 0);
                idx = idx.wrapping_add(1);
                submitted += 1;
            }
            self.set_sq_head(idx);
            submitted
        }
    }

    fn push_cqe(&mut self, user_data: u64, result: i64, flags: u32) {
        unsafe {
            let head = self.cq_head();
            let tail = self.cq_tail();
            let used = tail.wrapping_sub(head);
            if used >= self.entries { return; }
            let cqe_base = self.cq_kvirt + RING_HEADER_SIZE;
            let idx = tail % self.entries;
            let cqe = &mut *((cqe_base + idx as u64 * CQE_SIZE) as *mut IoUringCQEntry);
            cqe.user_data = user_data;
            cqe.result = result;
            cqe.flags = flags;
            self.set_cq_tail(tail.wrapping_add(1));
        }
    }

    pub fn map_to_user(&self, user_sq_addr: u64, page_table: u64) -> bool {
        use crate::arch::paging::flags;
        let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(page_table);
        vmm.map_page_with_flags(user_sq_addr, self.sq_phys, flags::PRESENT | flags::USER | flags::WRITABLE).is_ok()
            && vmm.map_page_with_flags(user_sq_addr + 0x1000, self.cq_phys, flags::PRESENT | flags::USER | flags::WRITABLE).is_ok()
    }
}
