//! Physical Memory Manager — Bitmap Allocator
//!
//! Tracks physical page availability using a bitmap.
//! Each bit represents one 4KiB page.
//!
//! Performance: O(n/64) allocation via 64-bit word scanning.

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::kprintln;
use crate::sync::Spinlock;

/// Page size: 4 KiB.
pub const PAGE_SIZE: usize = 4096;

/// Maximum supported physical memory: 4 GiB (for now).
/// This gives us 1M pages → 128 KiB bitmap.
const MAX_PAGES: usize = 1024 * 1024; // 4 GiB / 4 KiB
const BITMAP_WORDS: usize = MAX_PAGES / 64;

/// Bitmap: 1 = free, 0 = used/reserved.
static BITMAP: Spinlock<[u64; BITMAP_WORDS]> = Spinlock::new([0u64; BITMAP_WORDS]);

/// Total number of usable pages.
static TOTAL_PAGES: AtomicUsize = AtomicUsize::new(0);
/// Number of currently free pages.
static FREE_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Initialize the PMM from a memory map.
///
/// `regions` is an iterator of (base_addr, length, is_usable) tuples.
/// Called early in boot before any allocation.
pub fn init(entries: &[&limine::memmap::Entry]) {
    let mut bitmap = BITMAP.lock();
    // Start with everything marked as used (0)
    for i in 0..BITMAP_WORDS {
        bitmap[i] = 0;
    }

    let mut free_count = 0;
    // Mark usable regions as free
    for entry in entries {
        if entry.type_ != limine::memmap::MEMMAP_USABLE { continue; }

        let base = entry.base;
        let length = entry.length;
        let start_page = (base as usize + PAGE_SIZE - 1) / PAGE_SIZE; // round up
        let end_page = ((base + length) as usize) / PAGE_SIZE;        // round down

        for page in start_page..end_page {
            if page < MAX_PAGES {
                set_bit(&mut *bitmap, page);
                free_count += 1;
            }
        }
    }

    TOTAL_PAGES.store(free_count, Ordering::SeqCst);
    FREE_PAGES.store(free_count, Ordering::SeqCst);

    // Reserve page 0 (null pointer guard) and first 1 MiB (legacy hardware)
    let reserved_end = (1024 * 1024) / PAGE_SIZE; // 256 pages
    for page in 0..reserved_end {
        if test_bit(&*bitmap, page) {
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
/// Returns the physical address, or None if OOM.
pub fn alloc_page() -> Option<u64> {
    let mut bitmap = BITMAP.lock();
    for i in 0..BITMAP_WORDS {
        if bitmap[i] != 0 {
            // Find first set bit (free page)
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
/// Returns the base physical address, or None if unavailable.
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
                // Found a run — mark as used
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

/// Get number of free pages.
pub fn free_count() -> usize {
    FREE_PAGES.load(Ordering::SeqCst)
}

/// Get total usable pages.
pub fn total_count() -> usize {
    TOTAL_PAGES.load(Ordering::SeqCst)
}

// Bitmap helpers (word-level bit manipulation)

#[inline(always)]
fn set_bit(bitmap: &mut [u64], page: usize) {
    bitmap[page / 64] |= 1u64 << (page % 64);
}

#[inline(always)]
fn clear_bit(bitmap: &mut [u64], page: usize) {
    bitmap[page / 64] &= !(1u64 << (page % 64));
}

#[inline(always)]
fn test_bit(bitmap: &[u64], page: usize) -> bool {
    (bitmap[page / 64] & (1u64 << (page % 64))) != 0
}
