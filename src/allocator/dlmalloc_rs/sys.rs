use core::ptr;
use super::super::map_heap;
use arch::paging::PageMapper;
use sync::atomic::{Atomic, Ordering};

static OFFSET: Atomic<usize> = Atomic::new(0);

pub unsafe fn alloc(size: usize) -> (*mut u8, usize, u32) {
    let offset = OFFSET.load(Ordering::Relaxed);

    let (ptr, actual_size) = map_heap(&mut PageMapper::new(), ::KERNEL_HEAP_OFFSET + offset, size);
    
    OFFSET.fetch_add(actual_size, Ordering::Relaxed);

    (ptr, actual_size, 0)
}

pub unsafe fn remap(_ptr: *mut u8, _oldsize: usize, _newsize: usize, _can_move: bool)
    -> *mut u8
{
    // TODO: I think this can be implemented near the end?
    ptr::null_mut()
}

pub unsafe fn free_part(_ptr: *mut u8, _oldsize: usize, _newsize: usize) -> bool {
    false
}

pub unsafe fn free(_ptr: *mut u8, _size: usize) -> bool {
    false
}

pub fn can_release_part(_flags: u32) -> bool {
    false
}

pub fn allocates_zeros() -> bool {
    true
}

pub fn page_size() -> usize {
    4096
}
