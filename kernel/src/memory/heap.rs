//! Kernel Heap Allocator — Slab-Based
//!
//! Provides `GlobalAlloc` implementation using a simple bump/linked-list
//! free list allocator for the kernel heap region.
//!
//! Heap region: 16 MiB starting at a fixed virtual address.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use spin::Mutex;
use crate::kprintln;

/// Kernel heap virtual base address (in higher-half).
#[allow(dead_code)]
const HEAP_START: usize = 0xFFFF_C000_0000_0000;
/// Kernel heap size: 16 MiB.
const HEAP_SIZE: usize = 16 * 1024 * 1024;

/// A free block in the linked list.
struct FreeBlock {
    size: usize,
    next: Option<&'static mut FreeBlock>,
}

impl FreeBlock {
    #[allow(dead_code)]
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }
}

/// Linked-list heap allocator.
pub struct HeapAllocator {
    head: Option<&'static mut FreeBlock>,
}

impl HeapAllocator {
    pub const fn new() -> Self {
        Self { head: None }
    }

    /// Initialize the heap with a contiguous memory region.
    ///
    /// # Safety
    /// `start` must point to a valid, unused memory region of at least `size` bytes.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        let block = &mut *(start as *mut FreeBlock);
        block.size = size;
        block.next = None;
        self.head = Some(block);
    }

    /// Allocate memory with the given layout.
    fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(core::mem::align_of::<FreeBlock>());
        let size = layout.size().max(core::mem::size_of::<FreeBlock>());

        // Align size up
        let size = (size + align - 1) & !(align - 1);

        // First-fit search
        let mut current = &mut self.head;
        while let Some(ref mut block) = *current {
            let block_addr = *block as *mut FreeBlock as usize;
            let aligned_addr = (block_addr + align - 1) & !(align - 1);
            let adjustment = aligned_addr - block_addr;

            if block.size >= size + adjustment {
                let remaining = block.size - size - adjustment;

                if remaining >= core::mem::size_of::<FreeBlock>() + 16 {
                    // Split: create a new free block after the allocation
                    let new_block_addr = aligned_addr + size;
                    let new_block = unsafe { &mut *(new_block_addr as *mut FreeBlock) };
                    new_block.size = remaining;
                    new_block.next = block.next.take();
                    *current = Some(new_block);
                } else {
                    // Use the whole block
                    let next = block.next.take();
                    *current = next;
                }

                return aligned_addr as *mut u8;
            }

            current = &mut current.as_mut().unwrap().next;
        }

        ptr::null_mut() // OOM
    }

    /// Deallocate memory — add it back to the free list.
    fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        let align = layout.align().max(core::mem::align_of::<FreeBlock>());
        let size = layout.size().max(core::mem::size_of::<FreeBlock>());
        let size = (size + align - 1) & !(align - 1);

        unsafe {
            let block = &mut *(ptr as *mut FreeBlock);
            block.size = size;
            block.next = self.head.take();
            self.head = Some(block);
        }
    }
}

/// Global kernel allocator (locked).
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap(Mutex::new(HeapAllocator::new()));

struct LockedHeap(Mutex<HeapAllocator>);

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().deallocate(ptr, layout);
    }
}

/// Initialize the kernel heap.
///
/// Maps `HEAP_SIZE` pages into the heap virtual region and initializes
/// the allocator.
pub fn init() {
    // For now, we use a simple approach: Limine's HHDM maps all physical
    // memory, so we allocate physical pages and use them at their HHDM
    // virtual addresses. In the future, we'll map into HEAP_START.

    // Allocate heap pages from PMM
    let heap_pages = HEAP_SIZE / super::pmm::PAGE_SIZE;
    let phys_base = super::pmm::alloc_contiguous(heap_pages);

    match phys_base {
        Some(phys) => {
            let virt = super::vmm::phys_to_virt(phys) as usize;
            unsafe {
                // Zero the heap region
                core::ptr::write_bytes(virt as *mut u8, 0, HEAP_SIZE);
                ALLOCATOR.0.lock().init(virt, HEAP_SIZE);
            }
            kprintln!("    Heap: {} MiB at virt {:#x} (phys {:#x})",
                HEAP_SIZE / (1024 * 1024), virt, phys);
        }
        None => {
            kprintln!("[WARN] Heap: Not enough contiguous memory, using smaller heap");
            // Fallback: try a smaller heap (1 MiB)
            let small_size = 1024 * 1024;
            if let Some(phys) = super::pmm::alloc_contiguous(small_size / super::pmm::PAGE_SIZE) {
                let virt = super::vmm::phys_to_virt(phys) as usize;
                unsafe {
                    core::ptr::write_bytes(virt as *mut u8, 0, small_size);
                    ALLOCATOR.0.lock().init(virt, small_size);
                }
                kprintln!("    Heap: {} KiB (fallback)", small_size / 1024);
            } else {
                kprintln!("[ERROR] Heap: No memory available!");
            }
        }
    }
}
