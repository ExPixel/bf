use std::io::{self, Read, Write, StdinLock, StdoutLock};
use llvm;
use llvm::core::*;
use llvm::execution_engine::*;
use llvm::target::*;
use std::{mem, ptr};
use ::bf::{BFInstr, BF_MEMORY_SIZE, BFCellValue};

macro_rules! cstring {
	($s:expr) => (
		concat!($s, '\0').as_ptr() as *const _
	)
}

struct BFLLVMInfo {
	context: *mut llvm::LLVMContext,
	module: *mut llvm::LLVMModule,
	builder: *mut llvm::LLVMBuilder,
	execution_engine: LLVMExecutionEngineRef,
	llvm_bf_fn: *mut llvm::LLVMValue,
	compiled_bf_fn: extern "C" fn(*mut u8, *mut StdinLock, *mut StdoutLock) -> (),
	output: *mut i8,
	ready: bool,
	i32_type: *mut llvm::LLVMType,
	i8_type: *mut llvm::LLVMType,
	var_data_ptr: *mut llvm::LLVMValue,
	var_data_ptr_name: *const i8,

	bf_output_fn: *mut llvm::LLVMValue,
	bf_input_fn: *mut llvm::LLVMValue,
	var_ptr_stdin: *mut llvm::LLVMValue,
	var_ptr_stdout: *mut llvm::LLVMValue,
}

pub struct BFLLVMProgram {
	memory: Vec<BFCellValue>,

	/// Only used during compilation.
	pc: u32,
	compiled: bool,
	llvm_info: BFLLVMInfo,
}

impl BFLLVMProgram {
	pub fn new() -> BFLLVMProgram {
		let mut _mem = Vec::with_capacity(BF_MEMORY_SIZE);
		_mem.resize(BF_MEMORY_SIZE, 0);

		BFLLVMProgram {
			memory: _mem,
			pc: 0,
			compiled: false,
			llvm_info: unsafe { Self::create_llvm_info() }
		}
	}

	pub fn dump_llvm_ir(&self) {
		unsafe { LLVMDumpModule(self.llvm_info.module); }
	}

	unsafe fn clean_llvm_info(&mut self) {
		LLVMDisposeExecutionEngine(self.llvm_info.execution_engine);
		LLVMContextDispose(self.llvm_info.context);
		println!("Cleaned up LLVM");
	}

	unsafe fn finalize_llvm_info(&mut self) {
		println!("Finalizing...");

		let _bf_string = cstring!("bf");

		LLVMBuildRetVoid(self.llvm_info.builder);
		LLVMDisposeBuilder(self.llvm_info.builder);

		// #TODO make sure these are completed successfully.
		LLVMLinkInMCJIT();
		LLVM_InitializeNativeTarget();
		LLVM_InitializeNativeAsmPrinter();

		LLVMCreateExecutionEngineForModule(&mut self.llvm_info.execution_engine, self.llvm_info.module,
			&mut self.llvm_info.output);

		LLVMAddGlobalMapping(self.llvm_info.execution_engine, self.llvm_info.bf_output_fn, __bf_print_output as *mut _);
		LLVMAddGlobalMapping(self.llvm_info.execution_engine, self.llvm_info.bf_input_fn, __bf_get_input as *mut _);

		let addr = LLVMGetFunctionAddress(self.llvm_info.execution_engine, _bf_string);
		let f: extern "C" fn(*mut u8, *mut StdinLock, *mut StdoutLock) -> () = mem::transmute(addr);

		self.llvm_info.compiled_bf_fn = f;
		self.llvm_info.ready = true;

		println!("Finalized LLVM info.");
	}

	unsafe fn create_llvm_info() -> BFLLVMInfo {
		let _bf_string = cstring!("bf");
		let context = LLVMContextCreate();
		let module = LLVMModuleCreateWithNameInContext(_bf_string, context);
		let builder = LLVMCreateBuilderInContext(context);

		let i8_type = LLVMInt8TypeInContext(context);
		let i8_ptr_type = LLVMPointerType(i8_type, 0);
		let void_type = LLVMVoidTypeInContext(context);
		let void_ptr_type = LLVMPointerType(void_type, 0);
		let i32_type = LLVMInt32TypeInContext(context);

		let mut bf_output_function_args_type = [void_ptr_type, i8_type];
		let bf_output_function_type = LLVMFunctionType(
			void_type,
			bf_output_function_args_type.as_mut_ptr(),
			bf_output_function_args_type.len() as u32,
			0
		);
		let bf_output_fn = LLVMAddFunction(module, cstring!("__bf_print_output"), bf_output_function_type);
		LLVMSetFunctionCallConv(bf_output_fn, llvm::LLVMCallConv::LLVMCCallConv as u32);

		let mut bf_input_function_args_type = [void_ptr_type];
		let bf_input_function_type = LLVMFunctionType(
			i8_type,
			bf_input_function_args_type.as_mut_ptr(),
			bf_input_function_args_type.len() as u32,
			0
		);
		let bf_input_fn = LLVMAddFunction(module, cstring!("__bf_get_input"), bf_input_function_type);; // #TODO oh shit
		LLVMSetFunctionCallConv(bf_input_fn, llvm::LLVMCallConv::LLVMCCallConv as u32);

		let mut bf_function_args_type = [i8_ptr_type, void_ptr_type, void_ptr_type];
		let bf_function_type = LLVMFunctionType(
			void_type,
			bf_function_args_type.as_mut_ptr(),
			bf_function_args_type.len() as u32,
			0
		);
		let bf_function = LLVMAddFunction( module, _bf_string, bf_function_type);

		let basic_block = LLVMAppendBasicBlockInContext(
			context, bf_function,
			cstring!("entry")
		);

		LLVMPositionBuilderAtEnd(builder, basic_block);

		let ptr_memory = LLVMGetParam(bf_function, 0);
		let ptr_stdin = LLVMGetParam(bf_function, 1);
		let ptr_stdout = LLVMGetParam(bf_function, 2);

		let mut var_ptr_stdin = LLVMBuildAlloca(builder, void_ptr_type, cstring!("stdin_lock_ptr"));
		LLVMBuildStore(builder, ptr_stdin, var_ptr_stdin);
		let mut var_ptr_stdout = LLVMBuildAlloca(builder, void_ptr_type, cstring!("stdout_lock_ptr"));
		LLVMBuildStore(builder, ptr_stdout, var_ptr_stdout);

		let mut var_data_ptr = LLVMBuildAlloca(builder, i8_ptr_type, cstring!("data_ptr"));
		LLVMBuildStore(
			builder,
			ptr_memory,
			var_data_ptr
		);

		let mut execution_engine = mem::uninitialized();
		let mut output = mem::zeroed();

		println!("Intialized LLVM");

		BFLLVMInfo {
			context: context,
			module: module,
			builder: builder,
			execution_engine: execution_engine,
			llvm_bf_fn: bf_function,
			compiled_bf_fn: mem::transmute(ptr::null() as *const i8),
			output: output,
			ready: false,
			i32_type: i32_type,
			i8_type: i8_type,
			var_data_ptr: var_data_ptr,
			var_data_ptr_name: cstring!("data_ptr"),
			bf_output_fn: bf_output_fn,
			bf_input_fn: bf_input_fn,
			var_ptr_stdin: var_ptr_stdin,
			var_ptr_stdout: var_ptr_stdout,
		}
	}

	pub fn compile<R>(&mut self, mut input: R) where R: Read+Sized {
		if self.compiled { panic!("Cannot compile the same BFLLVMProgram twice.") }
		self.compiled = true;
		let mut buffer = [0u8; 2048];
		let mut last_char = 0;
		let mut last_char_count = 0;

		// format:
		// (Loop Start PC, Loop Block, After Loop Block)
		let mut block_stack = Vec::new();

		loop {
			match input.read(&mut buffer) {
				Ok(read) => {
					if read == 0 {
						if last_char_count > 0 {
							self.push_instr(last_char, last_char_count, &mut block_stack);
						}
						break
					}
					for idx in 0..read {
						let ch = buffer[idx];
						if !Self::valid_bf_char(ch) { continue }
						if last_char_count > 0 {
							if ch != last_char {
								self.push_instr(last_char, last_char_count, &mut block_stack);
								last_char_count = 1;
								last_char = ch;
							} else if ch == last_char {
								last_char_count += 1;
							}
						} else {
							last_char = ch;
							last_char_count = 1;
						}
					}
				},

				Err(e) => {
					panic!("Error while reading input: {}", e);
				}
			}
		}

		if let Some((unmatched_loop_start, _, _)) = block_stack.pop() {
			panic!("No matching ']' for '[' at {}", unmatched_loop_start);
		}

		unsafe { self.finalize_llvm_info(); }
	}

	fn valid_bf_char(ch: u8) -> bool {
		ch == b'>' || ch == b'<' || 
		ch == b'+' || ch == b'-' || 
		ch == b'.' || ch == b',' || 
		ch == b'[' || ch == b']'
	}

	#[inline(always)]
	fn push_instr(&mut self, ch: u8, arg: u32, block_stack: &mut Vec<(u32, *mut llvm::LLVMBasicBlock, *mut llvm::LLVMBasicBlock)>) {
		match ch {
			b'>' => {
				unsafe {
					let mut tmp = LLVMBuildLoad(self.llvm_info.builder,
						self.llvm_info.var_data_ptr,
						cstring!("temp"));
					tmp = LLVMBuildAdd(self.llvm_info.builder, 
						tmp, LLVMConstInt(self.llvm_info.i32_type, arg as u64, 0),
						cstring!("temp"));
					LLVMBuildStore(self.llvm_info.builder, tmp, self.llvm_info.var_data_ptr);
				}
				self.pc += arg;
			},
			b'<' => {
				unsafe {
					let mut tmp = LLVMBuildLoad(self.llvm_info.builder,
						self.llvm_info.var_data_ptr,
						cstring!("temp"));
					tmp = LLVMBuildSub(self.llvm_info.builder, 
						tmp, LLVMConstInt(self.llvm_info.i32_type, arg as u64, 0),
						cstring!("temp"));
					 LLVMBuildStore(self.llvm_info.builder, tmp, self.llvm_info.var_data_ptr);
				}
				self.pc += arg;
			},
			b'+' => {
				unsafe {
					let cell_ptr = LLVMBuildLoad(self.llvm_info.builder,
						self.llvm_info.var_data_ptr,
						cstring!("cell_ptr"));
					let mut cell_val = LLVMBuildLoad(self.llvm_info.builder,
						cell_ptr,
						cstring!("cell_val"));
					cell_val = LLVMBuildAdd(self.llvm_info.builder, 
						cell_val, LLVMConstInt(self.llvm_info.i8_type, arg as u64, 0),
						cstring!("cell_val"));
					LLVMBuildStore(self.llvm_info.builder, cell_val, cell_ptr);
				}
				self.pc += arg;
			},
			b'-' => {
				unsafe {
					let cell_ptr = LLVMBuildLoad(self.llvm_info.builder,
						self.llvm_info.var_data_ptr,
						cstring!("cell_ptr"));
					let mut cell_val = LLVMBuildLoad(self.llvm_info.builder,
						cell_ptr,
						cstring!("cell_val"));
					cell_val = LLVMBuildSub(self.llvm_info.builder, 
						cell_val, LLVMConstInt(self.llvm_info.i8_type, arg as u64, 0),
						cstring!("cell_val"));
					LLVMBuildStore(self.llvm_info.builder, cell_val, cell_ptr);
				}
				self.pc += arg;
			},
			b'.' => {
				unsafe {
					let cell_ptr = LLVMBuildLoad(self.llvm_info.builder,
						self.llvm_info.var_data_ptr,
						cstring!("cell_ptr"));
					let cell_val = LLVMBuildLoad(self.llvm_info.builder,
						cell_ptr,
						cstring!("cell_val"));
					let ptr_stdout = LLVMBuildLoad(self.llvm_info.builder, self.llvm_info.var_ptr_stdout, cstring!("sout"));
					let mut output_args = [ptr_stdout, cell_val];
					LLVMBuildCall(self.llvm_info.builder,
						self.llvm_info.bf_output_fn,
						output_args.as_mut_ptr(),
						output_args.len() as u32,
						cstring!("unused"));
				}
				self.pc += arg;
			},
			b',' => {
				unsafe {
					let ptr_stdin = LLVMBuildLoad(self.llvm_info.builder, self.llvm_info.var_ptr_stdin, cstring!("sin"));
					let mut input_args = [ptr_stdin];
					let input_val = LLVMBuildCall(self.llvm_info.builder,
						self.llvm_info.bf_input_fn,
						input_args.as_mut_ptr(),
						input_args.len() as u32,
						cstring!("input"));
					let cell_ptr = LLVMBuildLoad(self.llvm_info.builder,
						self.llvm_info.var_data_ptr,
						cstring!("cell_ptr"));
					LLVMBuildStore(self.llvm_info.builder, input_val, cell_ptr);
				}
				self.pc += arg;
			},

			b'[' => {
				unsafe {
					for _ in 0..arg {
						let loop_block = LLVMAppendBasicBlockInContext(
							self.llvm_info.context, self.llvm_info.llvm_bf_fn,
							cstring!("begin_loop")
						);

						let after_loop_block = LLVMAppendBasicBlockInContext(
							self.llvm_info.context, self.llvm_info.llvm_bf_fn,
							cstring!("after_loop")
						);

						let _pc = self.pc;
						block_stack.push((_pc, loop_block, after_loop_block));

						
						let cell_ptr = LLVMBuildLoad(self.llvm_info.builder,
							self.llvm_info.var_data_ptr,
							cstring!("cell_ptr"));
						let cell_val = LLVMBuildLoad(self.llvm_info.builder,
							cell_ptr,
							cstring!("cell_val"));

						let jump_out_of_loop = LLVMBuildICmp(self.llvm_info.builder,
							llvm::LLVMIntPredicate::LLVMIntEQ,
							cell_val,
							LLVMConstInt(self.llvm_info.i8_type, 0, 0),
							cstring!("loop_start_cmp")
						);

						LLVMBuildCondBr(self.llvm_info.builder,
							jump_out_of_loop,
							after_loop_block, loop_block);
						LLVMPositionBuilderAtEnd(self.llvm_info.builder, loop_block);

						self.pc += 1;
					}
				}
			},

			b']' => {
				unsafe {
					for _ in 0..arg {
						if let Some((loop_start_pc, loop_block, after_loop_block)) = block_stack.pop() {
							let cell_ptr = LLVMBuildLoad(self.llvm_info.builder,
								self.llvm_info.var_data_ptr,
								cstring!("cell_ptr"));
							let cell_val = LLVMBuildLoad(self.llvm_info.builder,
								cell_ptr,
								cstring!("cell_val"));
							let jump_restart_loop = LLVMBuildICmp(self.llvm_info.builder,
								llvm::LLVMIntPredicate::LLVMIntNE,
								cell_val,
								LLVMConstInt(self.llvm_info.i8_type, 0, 0),
								cstring!("loop_end_cmp")
							);
							LLVMBuildCondBr(self.llvm_info.builder,
								jump_restart_loop,
								loop_block, after_loop_block);
							LLVMPositionBuilderAtEnd(self.llvm_info.builder, after_loop_block);
						} else {
							panic!("No matching '[' for ']' at {}", self.pc);
						}
						self.pc += 1;
					}
				}
			},

			_ => { /* Non comman characters are just ignored. */ },
		}
	}

	pub fn run(&mut self) {
		if !self.llvm_info.ready { panic!("LLVM is not ready!"); }

		let memory_ptr = self.memory.as_mut_ptr();

		let stdin = io::stdin();
		let stdout = io::stdout();
		let mut stdin_locked = stdin.lock();
		let mut stdout_locked = stdout.lock();

		unsafe {
			(self.llvm_info.compiled_bf_fn)(memory_ptr, &mut stdin_locked, &mut stdout_locked)
		};
	}
}

impl Drop for BFLLVMProgram {
	fn drop(&mut self) {
		unsafe { self.clean_llvm_info(); }
	}
}


#[no_mangle]
pub unsafe extern "C" fn __bf_print_output(stdout: *mut StdoutLock, ch: u8) {
	// print!("({}),", ch);
	if let Some(stdout) = stdout.as_mut() {
		let buf = [ch];
		match stdout.write(&buf) {
			Err(err) => { panic!("Error while outputting char: {}", err) },
			_ => {}
		}
	}
}

#[no_mangle]
pub unsafe extern "C" fn __bf_get_input(stdin: *mut StdinLock) -> u8 {
	let mut buf = [0];
	if let Some(stdin) = stdin.as_mut() {
		match stdin.read(&mut buf) {
			Err(err) => {println!("Error while reading char: {}", err)},
			_ => {}
		}
	}
	return buf[0];
}