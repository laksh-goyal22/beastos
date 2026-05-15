#![no_std]
#![no_main]

extern crate beast_crt;

const SQE_SIZE: u64 = 40;
const CQE_SIZE: u64 = 24;
const HEADER_SIZE: u64 = 32;

const IORING_OP_WRITE: u8 = 2;

const SQ_TAIL_OFF: u64 = 4;
const CQ_HEAD_OFF: u64 = 0;
const CQ_TAIL_OFF: u64 = 4;

const RING_ADDR: u64 = 0x6000_0000_0000;
const SQ_ADDR: u64 = RING_ADDR;
const CQ_ADDR: u64 = RING_ADDR + 0x1000;

#[repr(C)]
struct Sqe {
    opcode: u8,
    flags: u8,
    _pad: [u8; 6],
    fd: u64,
    addr: u64,
    len: u64,
    user_data: u64,
}

#[repr(C)]
struct Cqe {
    user_data: u64,
    result: i64,
    flags: u32,
}

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    let fd = beast_syscall::io_uring_setup(16);
    if fd < 0 {
        return 1;
    }

    let msg = b"Hello via io_uring!\n";

    let sq_tail_ptr = (SQ_ADDR + SQ_TAIL_OFF) as *mut u32;
    let cq_head_ptr = (CQ_ADDR + CQ_HEAD_OFF) as *mut u32;
    let cq_tail_ptr = (CQ_ADDR + CQ_TAIL_OFF) as *mut u32;

    unsafe {
        let tail = core::ptr::read_volatile(sq_tail_ptr);
        let sqe = &mut *((SQ_ADDR + HEADER_SIZE + tail as u64 * SQE_SIZE) as *mut Sqe);
        sqe.opcode = IORING_OP_WRITE;
        sqe.fd = 1;
        sqe.addr = msg.as_ptr() as u64;
        sqe.len = msg.len() as u64;
        sqe.user_data = 42;
        core::ptr::write_volatile(sq_tail_ptr, tail.wrapping_add(1));
    }

    let ret = beast_syscall::io_uring_enter(fd as u64, 1, 0);
    if ret < 0 {
        return 2;
    }

    unsafe {
        let cq_tail = core::ptr::read_volatile(cq_tail_ptr);
        let cq_head = core::ptr::read_volatile(cq_head_ptr);
        if cq_head != cq_tail {
            let cqe = &*((CQ_ADDR + HEADER_SIZE + cq_head as u64 * CQE_SIZE) as *const Cqe);
            if cqe.result == msg.len() as i64 {
                core::ptr::write_volatile(cq_head_ptr, cq_head.wrapping_add(1));
                return 0;
            }
        }
    }

    3
}
