pub const IOURING_MAX_ENTRIES: u32 = 128;
pub const MAX_REGISTERED_BUFS: usize = 16;
pub const MAX_REGISTERED_FDS: usize = 32;
pub const SQE_SIZE: u64 = 40;
pub const CQE_SIZE: u64 = 24;
pub const RING_HEADER_SIZE: u64 = 32;

pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READ: u8 = 1;
pub const IORING_OP_WRITE: u8 = 2;
pub const IORING_OP_READ_ZC: u8 = 3;
pub const IORING_OP_OPEN: u8 = 4;
pub const IORING_OP_CLOSE: u8 = 5;

pub const IORING_REGISTER_BUFFER: u64 = 0;
pub const IORING_UNREGISTER_BUFFER: u64 = 1;
pub const IORING_REGISTER_FILES: u64 = 2;
pub const IORING_UNREGISTER_FILES: u64 = 3;

pub const SQE_CHAIN: u8 = 1;
pub const SQE_FIXED_FILE: u8 = 4;
pub const SQE_PRIORITY_SHIFT: u8 = 4;

pub const SQ_HEAD_OFF: u64 = 0;
pub const SQ_TAIL_OFF: u64 = 4;
pub const CQ_HEAD_OFF: u64 = 12;
pub const CQ_TAIL_OFF: u64 = 16;
pub const CQ_ENTRIES_OFF: u64 = 20;

/// Adaptive wait thresholds (microseconds)
pub const SPIN_PAUSE_MAX: u64 = 10;    // pause() spin up to 10µs
pub const SPIN_MWAIT_MIN: u64 = 10;    // mwait from 10µs
pub const SPIN_YIELD_MIN: u64 = 50;    // yield from 50µs
pub const MWAIT_HINT_C1: u32 = 0x00;   // C1 state (light sleep)

pub const IORING_SETUP_SQPOLL: u64 = 1;

pub struct RingCounters {
    pub submitted: u64, pub completed: u64, pub zero_copy: u64,
    pub chains: u64, pub fixed_file_hits: u64,
}

#[repr(C)]
pub struct IoUringSQEntry {
    pub opcode: u8, pub flags: u8, _pad: [u8; 6],
    pub fd: u64, pub addr: u64, pub len: u64, pub user_data: u64,
}
#[repr(C)]
pub struct IoUringCQEntry {
    pub user_data: u64, pub result: i64, pub flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RegisteredBuffer {
    pub user_addr: u64, pub size: u64, pub buf_id: u16, pub active: bool,
}

pub struct IoUring {
    pub sq_phys: u64, pub cq_phys: u64, pub sq_kvirt: u64, pub cq_kvirt: u64,
    pub entries: u32, pub page_table: u64,
    pub bufs: [RegisteredBuffer; MAX_REGISTERED_BUFS],
    pub fixed_files: alloc::vec::Vec<Option<u64>>,
    pub sq_poll: bool,
    pub counters: RingCounters,
    chain_result: i64,
}

impl IoUring {
    pub fn new(entries: u32, page_table: u64, flags: u64) -> Option<Self> {
        let entries = entries.min(IOURING_MAX_ENTRIES).max(1);
        let sq_phys = crate::memory::pmm::alloc_page()?;
        let cq_phys = crate::memory::pmm::alloc_page()?;
        let sq_kvirt = crate::memory::vmm::phys_to_virt(sq_phys);
        let cq_kvirt = crate::memory::vmm::phys_to_virt(cq_phys);
        unsafe {
            core::ptr::write_bytes(sq_kvirt as *mut u8, 0, 4096);
            core::ptr::write_bytes(cq_kvirt as *mut u8, 0, 4096);
            let sq = sq_kvirt as *mut u32;
            *sq.add(2) = entries;
            *sq.add(5) = entries;
        }
        Some(Self {
            sq_phys, cq_phys, sq_kvirt, cq_kvirt, entries, page_table,
            bufs: [RegisteredBuffer { user_addr: 0, size: 0, buf_id: 0, active: false }; MAX_REGISTERED_BUFS],
            fixed_files: alloc::vec::Vec::new(),
            sq_poll: (flags & IORING_SETUP_SQPOLL) != 0,
            counters: RingCounters { submitted: 0, completed: 0, zero_copy: 0, chains: 0, fixed_file_hits: 0 },
            chain_result: 0,
        })
    }

    pub fn register_buffer(&mut self, buf_id: u16, user_addr: u64, size: u64) -> bool {
        if (buf_id as usize) >= MAX_REGISTERED_BUFS { return false; }
        self.bufs[buf_id as usize] = RegisteredBuffer { user_addr, size, buf_id, active: true };
        true
    }
    pub fn unregister_buffer(&mut self, buf_id: u16) {
        if (buf_id as usize) < MAX_REGISTERED_BUFS { self.bufs[buf_id as usize].active = false; }
    }

    pub fn register_fds(&mut self, fds: alloc::vec::Vec<Option<u64>>) { self.fixed_files = fds; }
    pub fn unregister_fds(&mut self) { self.fixed_files.clear(); }

    unsafe fn sq_head(&self) -> u32 { core::ptr::read_volatile(self.sq_kvirt as *const u32) }
    unsafe fn sq_tail(&self) -> u32 { core::ptr::read_volatile((self.sq_kvirt + 4) as *const u32) }
    unsafe fn set_sq_head(&self, val: u32) { (self.sq_kvirt as *mut u32).write_volatile(val); }
    unsafe fn cq_head(&self) -> u32 { core::ptr::read_volatile(self.cq_kvirt as *const u32) }
    unsafe fn cq_tail(&self) -> u32 { core::ptr::read_volatile((self.cq_kvirt + 4) as *const u32) }
    unsafe fn set_cq_tail(&self, val: u32) { ((self.cq_kvirt + 4) as *mut u32).write_volatile(val); }

    fn exec_sqe(&mut self, sqe: &IoUringSQEntry) -> i64 {
        self.counters.submitted += 1;
        let fd = if (sqe.flags & SQE_FIXED_FILE) != 0 && (sqe.fd as usize) < self.fixed_files.len() {
            self.counters.fixed_file_hits += 1;
            self.fixed_files[sqe.fd as usize].unwrap_or(sqe.fd)
        } else { sqe.fd };
        let result = match sqe.opcode {
            IORING_OP_READ => crate::syscall::sys_read(fd, sqe.addr, sqe.len),
            IORING_OP_WRITE => crate::syscall::sys_write(fd, sqe.addr, sqe.len),
            IORING_OP_READ_ZC => { self.counters.zero_copy += 1; unsafe { self.read_zc(sqe) } }
            IORING_OP_OPEN => crate::syscall::sys_open(sqe.addr, sqe.len),
            IORING_OP_CLOSE => crate::syscall::sys_close(fd),
            _ => -1,
        };
        self.counters.completed += 1;
        result
    }

    pub fn submit_sqes(&mut self) -> u32 {
        unsafe {
            let head = self.sq_head();
            let tail = self.sq_tail();
            if head == tail { return 0; }
            let sqe_base = self.sq_kvirt + RING_HEADER_SIZE;
            let mut submitted = 0;
            // Process by priority: higher priority SQEs first (5 passes max)
            for prio in (0..5u8).rev() {
                let mut idx = head;
                while idx != tail && submitted < self.entries {
                    let sqe = &*((sqe_base + (idx as u64 % self.entries as u64) * SQE_SIZE) as *const IoUringSQEntry);
                    let sqe_prio = (sqe.flags >> SQE_PRIORITY_SHIFT) & 0x7;
                    if sqe_prio != prio { idx = idx.wrapping_add(1); continue; }
                    let chain = (sqe.flags & SQE_CHAIN) != 0;
                    let local_sqe = IoUringSQEntry {
                        opcode: sqe.opcode, flags: sqe.flags, _pad: [0; 6],
                        fd: if sqe.flags & 0x80 != 0 { self.chain_result.max(0) as u64 } else { sqe.fd },
                        addr: sqe.addr, len: sqe.len, user_data: sqe.user_data,
                    };
                    let result = self.exec_sqe(&local_sqe);
                    if !chain {
                        self.chain_result = 0;
                        self.push_cqe(sqe.user_data, result, 0);
                    } else {
                        self.counters.chains += 1;
                        self.chain_result = result;
                    }
                    submitted += 1;
                    idx = idx.wrapping_add(1);
                    if !chain { break; }
                }
            }
            self.set_sq_head(tail);
            submitted
        }
    }

    pub fn sq_poll_once(&mut self) -> u32 { self.submit_sqes() }

    unsafe fn read_zc(&mut self, sqe: &IoUringSQEntry) -> i64 {
        let buf_id = sqe.flags as u16 & 0x0F;
        if (buf_id as usize) >= MAX_REGISTERED_BUFS || !self.bufs[buf_id as usize].active { return -1; }
        let buf = &self.bufs[buf_id as usize];
        let phys = match crate::memory::pmm::alloc_page() { Some(p) => p, None => return -1 };
        let result = crate::syscall::read_file_into_phys(sqe.fd, phys, sqe.len.min(buf.size).min(4096));
        if result > 0 {
            use crate::arch::paging::flags;
            let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(self.page_table);
            let aligned_addr = buf.user_addr & !0xFFF;
            let _ = vmm.unmap_page(aligned_addr);
            let _ = vmm.map_page_with_flags(aligned_addr, phys, flags::PRESENT | flags::USER | flags::WRITABLE);
            result
        } else { crate::memory::pmm::free_page(phys); result }
    }

    fn push_cqe(&mut self, user_data: u64, result: i64, flags: u32) {
        unsafe {
            let head = self.cq_head(); let tail = self.cq_tail();
            if tail.wrapping_sub(head) >= self.entries { return; }
            let cqe = &mut *((self.cq_kvirt + RING_HEADER_SIZE + (tail % self.entries) as u64 * CQE_SIZE) as *mut IoUringCQEntry);
            cqe.user_data = user_data; cqe.result = result; cqe.flags = flags;
            self.set_cq_tail(tail.wrapping_add(1));
        }
    }

    pub fn map_to_user(&self, user_sq_addr: u64, pt: u64) -> bool {
        use crate::arch::paging::flags;
        let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(pt);
        vmm.map_page_with_flags(user_sq_addr, self.sq_phys, flags::PRESENT | flags::USER | flags::WRITABLE).is_ok()
            && vmm.map_page_with_flags(user_sq_addr + 0x1000, self.cq_phys, flags::PRESENT | flags::USER | flags::WRITABLE).is_ok()
    }

    /// Adaptive wait: spin/pause/mwait/yield based on allowed wait time and system pressure.
    /// Returns true if new SQEs arrived during the wait.
    pub fn adaptive_wait(&mut self, max_wait_us: u64) -> bool {
        if max_wait_us == 0 { return false; }
        // Reduce wait under high pressure
        let pressure = crate::pressure::get();
        let effective_wait = if pressure > 0.8 { 0 }
            else if pressure > 0.6 { max_wait_us / 2 }
            else { max_wait_us };
        if effective_wait == 0 { return false; }
        let start = crate::drivers::pit::get_ticks() as u64;
        let max_ticks = effective_wait / 10000;
        loop {
            unsafe { if self.sq_head() != self.sq_tail() { return true; } }
            let elapsed = (crate::drivers::pit::get_ticks() as u64).wrapping_sub(start);
            if elapsed >= max_ticks { return false; }
            if effective_wait <= SPIN_PAUSE_MAX {
                unsafe { core::arch::asm!("pause", options(nomem, nostack)); }
            } else {
                crate::scheduler::yield_now();
                return false;
            }
        }
    }
}
