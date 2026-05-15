#![no_std]
#![no_main]

extern crate beast_crt;

const SQE_SIZE: u64 = 40;
const CQE_SIZE: u64 = 24;
const HEADER_SIZE: u64 = 32;
const SQ_TAIL_OFF: u64 = 4;
const CQ_HEAD_OFF: u64 = 0;
const CQ_TAIL_OFF: u64 = 4;
const RING_ADDR: u64 = 0x6000_0000_0000;
const SQ_ADDR: u64 = RING_ADDR;
const CQ_ADDR: u64 = RING_ADDR + 0x1000;

const IORING_OP_WRITE: u8 = 2;
const IORING_OP_OPEN: u8 = 4;
const IORING_OP_CLOSE: u8 = 5;
const SQE_CHAIN: u8 = 1;

#[repr(C)]
struct Sqe { opcode: u8, flags: u8, _pad: [u8; 6], fd: u64, addr: u64, len: u64, user_data: u64 }
#[repr(C)]
struct Cqe { user_data: u64, result: i64, flags: u32 }

unsafe fn push_sqe(sqe: &Sqe) {
    let t = (SQ_ADDR + SQ_TAIL_OFF) as *mut u32;
    let tail = core::ptr::read_volatile(t);
    core::ptr::write((SQ_ADDR + HEADER_SIZE + (tail as u64 % 16) * SQE_SIZE) as *mut Sqe, *sqe);
    core::ptr::write_volatile(t, tail.wrapping_add(1));
}
unsafe fn pop_cqe() -> Option<Cqe> {
    let hp = (CQ_ADDR + CQ_HEAD_OFF) as *mut u32;
    let tp = (CQ_ADDR + CQ_TAIL_OFF) as *mut u32;
    let (h, t) = (core::ptr::read_volatile(hp), core::ptr::read_volatile(tp));
    if h == t { return None; }
    let cqe = core::ptr::read((CQ_ADDR + HEADER_SIZE + (h as u64 % 16) * CQE_SIZE) as *const Cqe);
    core::ptr::write_volatile(hp, h.wrapping_add(1));
    Some(cqe)
}

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    let ring_fd = beast_syscall::io_uring_setup(16);
    if ring_fd < 0 { return 1; }

    let msg = b"io_uring: write OK!\n";
    unsafe { push_sqe(&Sqe { opcode: IORING_OP_WRITE, flags: 0, _pad: [0; 6], fd: 1,
        addr: msg.as_ptr() as u64, len: msg.len() as u64, user_data: 10 }); }
    beast_syscall::io_uring_enter(ring_fd as u64, 1, 0);
    let cqe = unsafe { pop_cqe() };
    if cqe.is_none() || cqe.unwrap().result != msg.len() as i64 { return 2; }

    let path = b"/bin/hello.beast\0";
    unsafe { push_sqe(&Sqe { opcode: IORING_OP_OPEN, flags: 0, _pad: [0; 6], fd: 0,
        addr: path.as_ptr() as u64, len: path.len() as u64, user_data: 20 }); }
    beast_syscall::io_uring_enter(ring_fd as u64, 1, 0);
    let file_fd = unsafe { match pop_cqe() { Some(c) if c.result >= 0 => c.result as u64, _ => return 7 } };

    unsafe { push_sqe(&Sqe { opcode: IORING_OP_CLOSE, flags: 0, _pad: [0; 6], fd: file_fd,
        addr: 0, len: 0, user_data: 30 }); }
    beast_syscall::io_uring_enter(ring_fd as u64, 1, 0);
    unsafe { pop_cqe(); }

    // Chain: open + close in one submit
    unsafe { push_sqe(&Sqe { opcode: IORING_OP_OPEN, flags: SQE_CHAIN, _pad: [0; 6], fd: 0,
        addr: path.as_ptr() as u64, len: path.len() as u64, user_data: 40 });
        push_sqe(&Sqe { opcode: IORING_OP_CLOSE, flags: 0, _pad: [0; 6], fd: 0,
            addr: 0, len: 0, user_data: 50 }); }
    beast_syscall::io_uring_enter(ring_fd as u64, 2, 0);
    unsafe { pop_cqe(); }

    // Verify stats syscall works
    let mut stats = [0u64; 5];
    beast_syscall::io_uring_stats(ring_fd as u64, &mut stats);
    if stats[0] == 0 || stats[1] != stats[0] { return 11; }

    beast_syscall::io_uring_register(ring_fd as u64, 1, 0); // unregister, no-op test

    beast_syscall::write(1, b"io_uring: all tests passed!\n");
    0
}
