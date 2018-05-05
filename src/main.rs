
#![no_std]
#![feature(lang_items)]
#![feature(abi_x86_interrupt)]
#![feature(asm)]
#![feature(const_fn)]
#![feature(decl_macro)]
#![feature(pointer_methods)]
#![feature(thread_local)]
#![feature(alloc)]
#![feature(allocator_api)]
#![feature(global_allocator)]
#![feature(global_asm)]
#![feature(core_intrinsics)]
#![feature(naked_functions)]
#![feature(compiler_builtins_lib)]
#![feature(nonnull_cast)]

#![no_main]
// #![deny(warnings)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate bitflags;
extern crate os_bootinfo;
extern crate x86_64;
extern crate spin;
extern crate rlibc;
extern crate bit_field;
#[macro_use]
extern crate alloc;
extern crate hashmap_core;
extern crate nabi;

extern crate cretonne_wasm;
extern crate cretonne_native;
extern crate cretonne_codegen;
extern crate wasmparser;

#[macro_use]
pub mod arch;
pub mod panic;
pub mod memory;
pub mod time;
pub mod common;
pub mod allocator;
pub mod consts;
pub mod abi;
pub mod object;
pub mod task;
pub mod wasm;
pub mod tests;

pub use consts::*;

#[global_allocator]
pub static ALLOCATOR: allocator::Allocator = allocator::Allocator;

pub fn kmain() -> ! {
    println!("Nebulet v{}", VERSION);
    
    // tests::test_all();

    use task::thread::Thread;

    let thread0 = Thread::new(1024 * 16, test_thread0)
        .unwrap();
    let thread1 = Thread::new(1024 * 16, test_thread1)
        .unwrap();

    println!("Adding thread 0");
    thread0.resume().unwrap();
    println!("Adding thread 1");
    thread1.resume().unwrap();
    println!("Done");

    // task::GlobalScheduler::switch();

    unsafe {
        ::arch::interrupt::enable();
    }

    loop {
        unsafe { ::arch::interrupt::halt(); }
    }
}

extern fn test_thread0() {
    println!("Test Thread 0");

    loop {}
}

extern fn test_thread1() {
    println!("Test Thread 1");

    loop {}
}