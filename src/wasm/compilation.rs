//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module

use super::module::{Module, Export};
use super::{Relocation, Relocations, RelocationType, DataInitializer};
use cranelift_codegen::{self, isa::TargetIsa, binemit::{self, Reloc}, ir::{Signature, TrapCode, SourceLoc}};
use cranelift_wasm::FunctionIndex;
use super::RelocSink;
use super::abi::{ABI_MAP, INTRINSIC_MAP};

use memory::Region;
use object::Wasm;
use object::Dispatch;

use nabi::{Result, Error};
use alloc::vec::Vec;
use alloc::string::String;

pub fn get_abi_func(name: &str, sig: &Signature) -> Result<*const ()> {
    let abi_func = ABI_MAP.get(name).ok_or_else(|| internal_error!())?;

    if abi_func.same_sig(sig) {
        Ok(abi_func.ptr)
    } else {
        Err(internal_error!())
    }
}

fn get_abi_intrinsic(name: &str) -> Result<*const()> {
    let func = INTRINSIC_MAP.get(name)?;

    Ok(func.ptr)
}

#[derive(Debug, Clone)]
pub enum FunctionType {
    Local {
        offset: usize,
        size: usize,
    },
    External {
        module: String,
        name: String,
    }
}

pub struct Compilation {
    region: Region,

    /// Compiled machine code for the function bodies
    /// This is mapped onto `self.region`.
    functions: Vec<FunctionType>,

    first_local_function: usize,

    /// The computed relocations
    relocations: Relocations,

    /// List of traps and their offsets in the generated code
    traps: Vec<TrapData>,
}

impl Compilation {
    /// Allocates the runtime data structures with the given flags
    fn new(region: Region, functions: Vec<FunctionType>, relocations: Relocations, traps: Vec<TrapData>) -> Self {
        let first_local_function = functions
            .iter()
            .position(|f| match f {
                FunctionType::Local {..} => true,
                _ => false,
            }).unwrap();

        Compilation {
            region,
            functions,
            first_local_function,
            relocations,
            traps,
        }
    }

    fn relocate_function(&self, module: &Module, reloc_num: usize, r: &Relocation, target_func_addr: *const ()) -> Result<()> {
        let body_addr = self.get_function_addr(module, reloc_num + self.first_local_function)?;
        let reloc_addr = unsafe { (body_addr as *const u8).offset(r.offset as isize) };

        match r.reloc {
            Reloc::Abs8 => {
                unsafe {
                    (reloc_addr as *mut usize).write(target_func_addr as usize);
                }
            }
            _ => unimplemented!()
        }

        Ok(())
    }

    /// Relocate the compliation.
    fn relocate(&mut self, module: &Module) -> Result<()> {
        // The relocations are absolute addresses
        // TODO: Support architectures other than x86_64, and other reloc kinds.
        for (i, function_relocs) in self.relocations.iter().enumerate() {
            for (ref reloc, ref reloc_type) in function_relocs {
                let target_func = match reloc_type {
                    RelocationType::Normal(func_index) => {
                        self.get_function_addr(module, *func_index)?
                    },
                    RelocationType::Intrinsic(name) => {
                        get_abi_intrinsic(name)?
                    },
                };

                self.relocate_function(module, i, reloc, target_func)?;
            }
        }

        Ok(())
    }

    pub fn get_function_addr(&self, module_ref: &Module, func_index: FunctionIndex) -> Result<*const ()> {
        match self.functions[func_index] {
            FunctionType::Local {
                offset,
                size: _,
            } => {
                Ok((self.region.start().as_u64() as usize + offset) as _)
            },
            FunctionType::External {
                ref module,
                ref name,
            } => {
                match module.as_str() {
                    "abi" => {
                        let sig_index = module_ref.functions[func_index];
                        let imported_sig = &module_ref.signatures[sig_index];

                        get_abi_func(name, imported_sig)
                    },
                    _ => {
                        Err(internal_error!())
                    }
                }
            },
        }
    }

    /// Emit a `Code` instance
    pub fn emit(mut self, module: Module, data_initializers: Vec<DataInitializer>) -> Result<Dispatch<Wasm>> {
        self.relocate(&module)?;

        let start_index;
        if let Some(index) = module.start_func {
            start_index = index;
        }
        else if let Some(&Export::Function(index)) = module.exports.get("main") {
            start_index = index;
        }
        else {
            // TODO: We really need to handle this error nicely
            return Err(internal_error!());
        }

        // TODO: Check start func abi
        let start_ptr = self.get_function_addr(&module, start_index)?;

        let local_func_list = self.functions[module.imported_funcs.len()..]
            .iter()
            .map(|func_type| {
                match func_type {
                    FunctionType::Local {
                        offset,
                        size: _,
                    } => *offset,
                    _ => unreachable!()
                }
            })
            .collect();

        Wasm::new(module, data_initializers, self.region, start_ptr, local_func_list, self.traps)
    }
}

/// Define functions, etc and then "compile"
/// it all into a `Compliation`.
pub struct Compiler<'isa> {
    isa: &'isa dyn TargetIsa,

    contexts: Vec<(cranelift_codegen::Context, usize)>,

    total_size: usize,
}

impl<'isa> Compiler<'isa> {
    pub fn new(isa: &'isa dyn TargetIsa) -> Self {
        Self::with_capacity(isa, 0)
    }

    pub fn with_capacity(isa: &'isa dyn TargetIsa, capacity: usize) -> Self {
        Compiler {
            isa,
            contexts: Vec::with_capacity(capacity),
            total_size: 0,
        }
    }

    /// Define a function. This also compiles the function.
    pub fn define_function(&mut self, mut ctx: cranelift_codegen::Context) -> Result<()> {
        let code_size = ctx.compile(self.isa)
            .map_err(|e| {
                println!("Compile error: {:?}", e);
                internal_error!()
            })? as usize;

        self.contexts.push((ctx, code_size));

        self.total_size += code_size;

        Ok(())
    }

    /// This allocates a region from the Sip memory allocator
    /// and emits all the functions into that.
    ///
    /// This assumes that the functions don't need a specific
    /// alignment, which is true on x86_64, but may not
    /// be true on other architectures.
    pub fn compile(self, module: &Module) -> Result<Compilation> {
        let region = Region::allocate(self.total_size)
            .ok_or(Error::NO_MEMORY)?;

        let mut functions = Vec::with_capacity(module.functions.len());
        let mut relocs = Vec::with_capacity(self.contexts.len());
        let mut traps = Vec::new();

        let mut offset = 0;
        let region_start = region.start().as_u64() as usize;

        for (module, name) in module.imported_funcs.iter().cloned() {
            functions.push(FunctionType::External {
                module,
                name,
            });
        }

        // emit functions to memory
        for (ref ctx, size) in self.contexts.iter() {
            let mut trap_sink = TrapSink::new(offset);
            let mut reloc_sink = RelocSink::new();

            // println!("{}", ctx.func.display(Some(self.isa)));

            unsafe {
                ctx.emit_to_memory(self.isa, (region_start + offset) as *mut u8, &mut reloc_sink, &mut trap_sink);
            }

            functions.push(FunctionType::Local {
                offset,
                size: *size,
            });
            relocs.push(reloc_sink.func_relocs);
            traps.append(&mut trap_sink.trap_datas);

            offset += size;
        }

        Ok(Compilation::new(region, functions, relocs, traps))
    }
}

pub struct TrapData {
    pub offset: usize,
    pub code: TrapCode,
}

/// Simple implementation of a TrapSink
/// that saves the info for later.
pub struct TrapSink {
    current_func_offset: usize,
    trap_datas: Vec<TrapData>,
}

impl TrapSink {
    pub fn new(current_func_offset: usize) -> TrapSink {
        TrapSink {
            current_func_offset,
            trap_datas: Vec::new(),
        }
    }
}

impl binemit::TrapSink for TrapSink {
    fn trap(&mut self, offset: u32, _: SourceLoc, code: TrapCode) {
        self.trap_datas.push(TrapData {
            offset: self.current_func_offset + offset as usize,
            code,
        });
    }
}
