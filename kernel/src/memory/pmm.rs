//! Physical Memory Manager — Bitmap Allocator
//!
//! Tracks physical page availability using a bitmap.
//! Each bit represents one 4KiB page.

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::kprintln;
use crate::sync::Spinlock;

/// Page size: 4 KiB.
pub const PAGE_SIZE: usize = 4096;

/// Maximum supported physical memory: 4 GiB (for now).
const MAX_PAGES: usize = 1024 * 1024;
const BITMAP_WORDS: usize = MAX_PAGES / 64;

/// Bitmap: 1 = free, 0 = used/reserved.
static BITMAP: Spinlock<[u64; BITMAP_WORDS]> = Spinlock::new([0u64; BITMAP_WORDS]);

/// Total number of usable pages.
static TOTAL_PAGES: AtomicUsize = AtomicUsize::new(0);
/// Number of currently free pages.
static FREE_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Initialize the PMM from a memory map and explicit kernel range.
pub fn init(entries: &[&limine::memmap::Entry], kernel_start: u64, kernel_end: u64) {
    let mut bitmap = BITMAP.lock();
    // Start with everything marked as used (0)
    for i in 0..BITMAP_WORDS {
        bitmap[i] = 0;
    }

    let mut free_count = 0;
    // Mark usable regions as free
    for entry in entries {
        if entry.type_ == limine::memmap::MEMMAP_USABLE {
            let base = entry.base;
            let length = entry.length;
            let start_page = (base as usize + PAGE_SIZE - 1) / PAGE_SIZE;
            let end_page = ((base + length) as usize) / PAGE_SIZE;

            for page in start_page..end_page {
                if page < MAX_PAGES {
                    set_bit(&mut *bitmap, page);
                    free_count += 1;
                }
            }
        }
    }

    TOTAL_PAGES.store(free_count, Ordering::SeqCst);
    FREE_PAGES.store(free_count, Ordering::SeqCst);

    // CRITICAL: Reserve the kernel and modules range explicitly.
    let k_start_page = kernel_start as usize / PAGE_SIZE;
    let k_end_page = (kernel_end as usize + PAGE_SIZE - 1) / PAGE_SIZE;
    for page in k_start_page..k_end_page {
        if page < MAX_PAGES && test_bit(&*bitmap, page) {
            clear_bit(&mut *bitmap, page);
            FREE_PAGES.fetch_sub(1, Ordering::SeqCst);
        }
    }

    // Reserve page 0 (null pointer guard) and first 1 MiB (legacy hardware)
    let reserved_end = (1024 * 1024) / PAGE_SIZE;
    for page in 0..reserved_end {
        if page < MAX_PAGES && test_bit(&*bitmap, page) {
            clear_bit(&mut *bitmap, page);
            FREE_PAGES.fetch_sub(1, Ordering::SeqCst);
        }
    }

    let free = FREE_PAGES.load(Ordering::SeqCst);
    let total = TOTAL_PAGES.load(Ordering::SeqCst);
    kprintln!("    PMM: {} usable pages ({} MiB), {} reserved",
        free,
        free * PAGE_SIZE / (1024 * 1024),
        total - free);
}

/// Allocate a single physical page.
pub fn alloc_page() -> Option<u64> {
    let mut bitmap = BITMAP.lock();
    for i in 0..BITMAP_WORDS {
        if bitmap[i] != 0 {
            let bit = bitmap[i].trailing_zeros() as usize;
            let page = i * 64 + bit;
            clear_bit(&mut *bitmap, page);
            FREE_PAGES.fetch_sub(1, Ordering::SeqCst);
            return Some((page * PAGE_SIZE) as u64);
        }
    }
    None
}

/// Free a physical page by its address.
pub fn free_page(phys_addr: u64) {
    let page = (phys_addr as usize) / PAGE_SIZE;
    if page >= MAX_PAGES { return; }
    let mut bitmap = BITMAP.lock();
    if !test_bit(&*bitmap, page) {
        set_bit(&mut *bitmap, page);
        FREE_PAGES.fetch_add(1, Ordering::SeqCst);
    }
}

/// Allocate `count` contiguous physical pages.
pub fn alloc_contiguous(count: usize) -> Option<u64> {
    if count == 0 { return None; }
    let mut bitmap = BITMAP.lock();
    let mut run_start = 0;
    let mut run_len = 0;

    for page in 0..MAX_PAGES {
        if test_bit(&*bitmap, page) {
            if run_len == 0 { run_start = page; }
            run_len += 1;
            if run_len == count {
                for p in run_start..run_start + count {
                    clear_bit(&mut *bitmap, p);
                    FREE_PAGES.fetch_sub(1, Ordering::SeqCst);
                }
                return Some((run_start * PAGE_SIZE) as u64);
            }
        } else {
            run_len = 0;
        }
    }
    None
}

pub fn free_count() -> usize { FREE_PAGES.load(Ordering::SeqCst) }
pub fn total_count() -> usize { TOTAL_PAGES.load(Ordering::SeqCst) }

#[inline(always)]
fn set_bit(bitmap: &mut [u64], page: usize) { bitmap[page / 64] |= 1u64 << (page % 64); }
#[inline(always)]
fn clear_bit(bitmap: &mut [u64], page: usize) { bitmap[page / 64] &= !(1u64 << (page % 64)); }
#[inline(always)]
fn test_bit(bitmap: &[u64], page: usize) -> bool { (bitmap[page / 64] & (1u64 << (page % 64))) != 0 }
