#![no_std]
#![feature(
    lang_items,
    abi_x86_interrupt,
    asm,
    const_fn,
    decl_macro,
    thread_local,
    alloc,
    allocator_api,
    global_asm,
    core_intrinsics,
    naked_functions,
    compiler_builtins_lib,
    box_into_raw_non_null,
    box_syntax,
    unsize,
    coerce_unsized,
    dropck_eyepatch,
    arbitrary_self_types,
    nll,
    fnbox,
    integer_atomics,
    platform_intrinsics,
    range_contains,
    stmt_expr_attributes,
    alloc_error_handler,
    const_fn_union,
)]

#![no_main]
#![deny(warnings)]

#[macro_use]
extern crate bootloader;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate bitflags;
extern crate spin;
extern crate bit_field;
#[macro_use]
extern crate alloc;
extern crate hashmap_core;
#[macro_use]
extern crate nabi;
extern crate raw_cpuid;
extern crate rand_core;
extern crate rand;
extern crate acpi;

extern crate cranelift_wasm;
extern crate cranelift_native;
extern crate cranelift_codegen;
extern crate target_lexicon;
extern crate wasmparser;
extern crate nebulet_derive;

pub use bootloader::x86_64;

pub mod nil;
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
pub mod externs;
pub mod sync;
pub mod signals;
pub mod event;

pub use consts::*;

use object::{Thread, Process, Wasm, Channel, HandleRights, Dispatcher};
use object::channel;
use event::{Event, EventVariant};
use object::dispatcher::LocalObserver;
use object::wait_observer::WaitObserver;
use signals::Signal;
use common::tar::Tar;
use alloc::vec::Vec;
use nabi::Error;

#[global_allocator]
pub static ALLOCATOR: allocator::Allocator = allocator::Allocator;

pub fn kmain(init_fs: &[u8]) -> ! {
    // println!("------------");
    // println!("Nebulet v{}", VERSION);

    let mut thread = Thread::new(1024 * 1024, move || {
        first_thread(init_fs);
    }).unwrap();

    thread.start();

    unsafe {
        arch::cpu::Local::context_switch();
    }

    unreachable!();
}

fn first_thread(init_fs: &[u8]) {
    let tar = Tar::load(init_fs);

    let wasm = tar.iter().find(|file| {
        file.path == "sipinit.wasm"
    }).unwrap();

    let code = Wasm::compile(wasm.data)
        .unwrap();

    let process = Process::create(code.copy_ref())
        .unwrap();

    let (tx, rx) = Channel::new_pair();

    {
        let mut handle_table = process.handle_table().write();
        let handle = handle_table.allocate(rx, HandleRights::READ | HandleRights::TRANSFER).unwrap();
        assert!(handle.inner() == 0);
    }

    process.start().unwrap();
    
    let event = Event::new(EventVariant::AutoUnsignal);
    let mut waiter = WaitObserver::new(event, Signal::WRITABLE);

    for chunk in init_fs.chunks(channel::MAX_MSG_SIZE) {
        loop {
            let msg = channel::Message::new(chunk, Vec::new()).unwrap(); // not efficient, but it doesn't matter here
            match tx.send(msg) {
                Ok(_) => break,
                Err(Error::SHOULD_WAIT) => {
                    if let Some(observer) = LocalObserver::new(&mut waiter, &mut tx.copy_ref().upcast()) {
                        observer.wait();
                        drop(observer);
                    }
                },
                Err(e) => panic!("initfs channel err: {:?}", e),
            }
        }
    }
    tx.on_zero_handles();
}
