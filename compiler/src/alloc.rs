//! The process allocator.
//!
//! On macOS, growing a large block with `realloc` — what a `Vec` does each
//! time it doubles — leaves the blocks it outgrew in the allocator, freed but
//! never reused and never given back: a program that builds a list of a
//! million values by `push` and drops it, forty times, holds the memory of
//! forty lists (measured on macOS 26.1, in plain Rust as in Soma: 3.2 GB
//! where 110 MB are live). A strategy's history replayed in one process went
//! to 4.5 GB; from outside it reads as memory that is never released.
//!
//! Growing a large block is therefore allocate, copy, free: the old block
//! goes back as a block of its own size, which the next list reuses. Small
//! blocks, and every other system, keep the system's `realloc` (on Linux it
//! moves a large block without copying it).

#[cfg(target_os = "macos")]
mod macos {
    use std::alloc::{GlobalAlloc, Layout, System};

    /// From this size on, a block is one of the allocator's "large" ones.
    const LARGE: usize = 1 << 20;

    pub struct NoLargeRealloc;

    unsafe impl GlobalAlloc for NoLargeRealloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 { System.alloc(layout) }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout) }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { System.alloc_zeroed(layout) }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            if layout.size() < LARGE && new_size < LARGE { return System.realloc(ptr, layout, new_size); }
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            let new_ptr = System.alloc(new_layout);
            // on failure the old block is left as it was, as `realloc` promises
            if !new_ptr.is_null() {
                std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                System.dealloc(ptr, layout);
            }
            new_ptr
        }
    }

    #[global_allocator]
    static ALLOCATOR: NoLargeRealloc = NoLargeRealloc;
}
