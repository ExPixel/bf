use std::io::{self, Read, Write, StdinLock, StdoutLock};

pub type BFCellValue = u8;
pub const BF_MEMORY_SIZE: usize = 3000;
pub const GROUP_REPEAT_PRINTS: bool = false;

const OPTIMIZATIONS: [fn(&[BFInstr], &mut Vec<BFInstr>) -> bool; 3] = [
	BFProgram::optimize_zero,
	BFProgram::optimize_move_data,
	BFProgram::optimize_find_zero,
];

#[derive(Debug, Copy, Clone)]
pub enum BFInstr {
	IncPC(usize),
	DecPC(usize),
	IncVal(usize),
	DecVal(usize),
	Output(usize),
	Input(usize),

	LoopStart(usize),
	LoopEnd(usize),

	// Optimized Instructions:
	ZeroCurrentCell,

	AddCellValueRight(usize), // (dist, rhs)
	AddCellValueLeft(usize), // (dist, rhs)
	
	SubCellValueRight(usize), // (dist, rhs)
	SubCellValueLeft(usize), // (dist, rhs)

	FindZeroCellLeft(usize),
	FindZeroCellRight(usize),
}

#[derive(Default)]
pub struct BFProgramStats {
	/// Number of loops that were optimized.
	pub optimized_loop_count: usize,

	/// Number of loops found in the program.
	pub loop_count: usize,
}

/// Brainfuck program.
pub struct BFProgram {
	memory: Vec<BFCellValue>,
	instructions: Vec<BFInstr>,

	/// Number of BF commands that actually make up this program (# read).
	instr_count: usize,
	
	data_ptr: usize,
	pc: usize,

	pub stats: BFProgramStats,
}

impl BFProgram {
	pub fn new() -> BFProgram {
		let mut _mem = Vec::with_capacity(BF_MEMORY_SIZE);
		_mem.resize(BF_MEMORY_SIZE, 0);

		BFProgram {
			memory: _mem,
			instructions: Vec::new(),
			instr_count: 0,

			data_ptr: 0,
			pc: 0,

			stats: BFProgramStats::default()
		}
	}

	pub fn compile<R>(&mut self, mut input: R) where R: Read+Sized {
		let mut buffer = [0u8; 2048];
		let mut last_char = 0;
		let mut last_char_count = 0;
		let mut loop_stack = Vec::new();
		let mut optim_workspace = Vec::new();
		loop {
			match input.read(&mut buffer) {
				Ok(read) => {
					if read == 0 {
						if last_char_count > 0 {
							self.push_instr(last_char, last_char_count, &mut loop_stack, &mut optim_workspace);
						}
						break
					}
					for idx in 0..read {
						let ch = buffer[idx];
						if !Self::valid_bf_char(ch) { continue }
						if last_char_count > 0 {
							if ch != last_char {
								self.push_instr(last_char, last_char_count, &mut loop_stack, &mut optim_workspace);
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

		if let Some(unmatched_loop_start) = loop_stack.pop() {
			panic!("No matching ']' for '[' at {}", unmatched_loop_start);
		}
	}

	fn valid_bf_char(ch: u8) -> bool {
		ch == b'>' || ch == b'<' || 
		ch == b'+' || ch == b'-' || 
		ch == b'.' || ch == b',' || 
		ch == b'[' || ch == b']'
	}

	#[inline(always)]
	fn push_instr(&mut self, ch: u8, arg: usize, loop_stack: &mut Vec<usize>, optim_workspace: &mut Vec<BFInstr>) {
		match ch {
			b'>' => { self.instructions.push(BFInstr::IncPC(arg)); self.instr_count += arg; },
			b'<' => { self.instructions.push(BFInstr::DecPC(arg)); self.instr_count += arg; },
			b'+' => { self.instructions.push(BFInstr::IncVal(arg)); self.instr_count += arg; },
			b'-' => { self.instructions.push(BFInstr::DecVal(arg)); self.instr_count += arg; },
			b'.' => { self.instructions.push(BFInstr::Output(arg)); self.instr_count += arg; },
			b',' => { self.instructions.push(BFInstr::Input(arg)); self.instr_count += arg; },

			b'[' => {
				for _ in 0..arg {
					loop_stack.push(self.instructions.len());
					self.instructions.push(BFInstr::LoopStart(0)); // will be back patched.
					self.instr_count += 1;
				}
			},

			b']' => {
				for _ in 0..arg {
					if let Some(loop_start) = loop_stack.pop() {
						let loop_end = self.instructions.len();
						unsafe {*self.instructions.get_unchecked_mut(loop_start) =
							BFInstr::LoopStart(loop_end); }
						self.instructions.push(BFInstr::LoopEnd(loop_start));
						self.instr_count += 1;

						self.stats.loop_count += 1;

						if cfg!(not(feature = "nooptim")) {
							if self.optimize_loop(loop_start, optim_workspace) {
								self.stats.optimized_loop_count += 1;
							}
						}
					} else {
						panic!("No matching '[' for ']' at {}", self.instructions.len());
					}
				}
			},

			_ => { /* Non comman characters are just ignored. */ },
		}
	}

	#[cfg(not(feature = "stats"))]
	pub fn run(&mut self) {
		let stdin = io::stdin();
		let stdout = io::stdout();
		let mut stdin_locked = stdin.lock();
		let mut stdout_locked = stdout.lock();

		while self.pc < self.instructions.len() {
			self._step(&mut stdin_locked, &mut stdout_locked);
			self.pc += 1;
		}
	}

	#[cfg(feature = "stats")]
	pub fn run(&mut self) {
		use std::collections::HashMap;

		let mut loop_map: HashMap<String, usize> = HashMap::new();

		let stdin = io::stdin();
		let stdout = io::stdout();
		let mut stdin_locked = stdin.lock();
		let mut stdout_locked = stdout.lock();

		let mut window_buffer = String::new();

		while self.pc < self.instructions.len() {

			if let BFInstr::LoopEnd(loop_start) = self.instructions[self.pc] {
				window_buffer.clear();
				format_bf_window_into(&self.instructions[loop_start..(self.pc + 1)], &mut window_buffer);
				if !loop_map.contains_key(&window_buffer) {
					loop_map.insert(window_buffer.clone(), 1);
				} else {
					if let Some(c) = loop_map.get_mut(&window_buffer) {
						*c += 1;
					}
				}
			}

			self._step(&mut stdin_locked, &mut stdout_locked);
			self.pc += 1;
		}

		let mut loop_stats = Vec::new();

		for item in loop_map.drain() {
			loop_stats.push(item);
		}

		loop_stats.sort_by(|a, b| b.1.cmp(&a.1));

		for &(ref loopstr, ref exec_count) in loop_stats.iter().take(10) {
			println!("{}\t\t\t\t ...\t{} times", loopstr, exec_count);
		}
	}

	fn _step(&mut self, stdin: &mut StdinLock, stdout: &mut StdoutLock) {
		match unsafe { *self.instructions.get_unchecked(self.pc) } {
			BFInstr::IncPC(inc) => self.data_ptr += inc,
			BFInstr::DecPC(dec) => self.data_ptr -= dec,
			BFInstr::IncVal(inc) => { let cur_cell = self.data_ptr; self.cell_add_imm(cur_cell, inc) },
			BFInstr::DecVal(dec) => { let cur_cell = self.data_ptr; self.cell_sub_imm(cur_cell, dec) },
			BFInstr::Output(times) => for _ in 0..times {
				let buf = &self.memory[self.data_ptr..(self.data_ptr + 1)];
				match stdout.write(buf) {
					Err(err) => {println!("Error while outputting char: {}", err)},
					_ => {}
				}
			},

			BFInstr::Input(times) => for _ in 0..times {
				let mut buf = &mut self.memory[self.data_ptr..(self.data_ptr + 1)];
				match stdin.read(buf) {
					Err(err) => {println!("Error while reading char: {}", err)},
					_ => {}
				}
			},

			BFInstr::LoopStart(jump_to) => {
				if self.memory[self.data_ptr] == 0 {
					self.pc = jump_to;
				}
			},

			BFInstr::LoopEnd(jump_to) => {
				if self.memory[self.data_ptr] != 0 {
					self.pc = jump_to;
				}
			},

			BFInstr::ZeroCurrentCell => {
				// no zero check necessary
				self.memory[self.data_ptr] = 0;
			},

			BFInstr::AddCellValueRight(dist) => {
				if self.memory[self.data_ptr] != 0 {
					debug_assert!(self.data_ptr + dist < BF_MEMORY_SIZE,
						"Expected self.data_ptr ({}) + dist ({}) to be less than BF_MEMORY_SIZE ({})",
						self.data_ptr, dist, BF_MEMORY_SIZE);
					let (lhs_cell, rhs_cell) = (self.data_ptr + dist, self.data_ptr);
					self.cell_add_cell(lhs_cell, rhs_cell);
					self.memory[self.data_ptr] = 0;
				}
			},

			BFInstr::AddCellValueLeft(dist) => {
				if self.memory[self.data_ptr] != 0 {
					debug_assert!(self.data_ptr >= dist,
						"Expected self.data_ptr ({}) to be greater than or equal to dist ({})",
						self.data_ptr, dist);
					let (lhs_cell, rhs_cell) = (self.data_ptr - dist, self.data_ptr);
					self.cell_add_cell(lhs_cell, rhs_cell);
					self.memory[self.data_ptr] = 0;
				}
			},

			BFInstr::SubCellValueRight(dist) => {
				if self.memory[self.data_ptr] != 0 {
					debug_assert!(self.data_ptr + dist < BF_MEMORY_SIZE,
						"Expected self.data_ptr ({}) + dist ({}) to be less than BF_MEMORY_SIZE ({})",
						self.data_ptr, dist, BF_MEMORY_SIZE);
					let (lhs_cell, rhs_cell) = (self.data_ptr + dist, self.data_ptr);
					self.cell_sub_cell(lhs_cell, rhs_cell);
					self.memory[self.data_ptr] = 0;
				}
			},

			BFInstr::SubCellValueLeft(dist) => {
				if self.memory[self.data_ptr] != 0 {
					debug_assert!(self.data_ptr >= dist,
						"Expected self.data_ptr ({}) to be greater than or equal to dist ({})",
						self.data_ptr, dist);
					let (lhs_cell, rhs_cell) = (self.data_ptr - dist, self.data_ptr);
					self.cell_sub_cell(lhs_cell, rhs_cell);
					self.memory[self.data_ptr] = 0;
				}
			},

			BFInstr::FindZeroCellLeft(step_size) => {
				while self.memory[self.data_ptr] != 0 {
					self.data_ptr -= step_size;
				}
			},

			BFInstr::FindZeroCellRight(step_size) => {
				while self.memory[self.data_ptr] != 0 {
					self.data_ptr += step_size;
				}
			},
		}
	}

	#[inline(always)]
	pub fn cell_add_cell(&mut self, lhs_cell: usize, rhs_cell: usize) {
		let rhs = self.memory[rhs_cell];
		self.cell_add_imm(lhs_cell, rhs);
	}

	#[inline(always)]
	pub fn cell_sub_cell(&mut self, lhs_cell: usize, rhs_cell: usize) {
		let rhs = self.memory[rhs_cell];
		self.cell_sub_imm(lhs_cell, rhs);
	}

	#[inline(always)]
	pub fn cell_add_imm<I: Into<usize>>(&mut self, cell: usize, amt: I) {
		self.memory[cell] = (self.memory[cell] as usize).wrapping_add(amt.into()) as BFCellValue;
	}

	#[inline(always)]
	pub fn cell_sub_imm<I: Into<usize>>(&mut self, cell: usize, amt: I) {
		self.memory[cell] = (self.memory[cell] as usize).wrapping_sub(amt.into()) as BFCellValue;
	}

	pub fn get_instr_count(&self) -> usize {
		self.instr_count
	}

	pub fn get_instructions<'p>(&'p self) -> &'p [BFInstr] {
		&self.instructions
	}

	fn optimize_loop(&mut self, loop_start: usize, workspace: &mut Vec<BFInstr>) -> bool {
		let mut optimized = false;

		{
			let window = &self.instructions[(loop_start + 1)..(self.instructions.len() - 1)];

			for optim in OPTIMIZATIONS.iter() {
				if optim(window, workspace) {
					optimized = true;
					break;
				}
			}
		}

		if optimized {
			self.instructions.truncate(loop_start);
			self.instructions.append(workspace);
			workspace.clear();
		} else if cfg!(feature = "dverbose") {
			let loop_size = (self.instructions.len() - 1) - (loop_start + 1);
			if loop_size <= 128 {
				println!("SKIPPED OPT: {}", format_bf_window(&self.instructions[(loop_start + 1)..(self.instructions.len() - 1)]));
			}
		}

		optimized
	}

	fn optimize_find_zero(window: &[BFInstr], workspace: &mut Vec<BFInstr>) -> bool {
		if window.len() == 1 {
			if let BFInstr::DecPC(step_size) = window[0] {
				workspace.push(BFInstr::FindZeroCellLeft(step_size));
				return true;
			} else if let BFInstr::IncPC(step_size) = window[0] {
				workspace.push(BFInstr::FindZeroCellRight(step_size));
				return true;
			}
		}
		false
	}

	fn optimize_zero(window: &[BFInstr], workspace: &mut Vec<BFInstr>) -> bool {
		if window.len() == 1 {
			if let BFInstr::DecVal(1) = window[0] {
				// OPTIMIZES [-]
				workspace.push(BFInstr::ZeroCurrentCell);
				return true;
			} else if let BFInstr::ZeroCurrentCell = window[0] {
				// OPTIMIZES [[-]]
				workspace.push(BFInstr::ZeroCurrentCell);
				return true;
			}
		}
		false
	}

	fn optimize_move_data(window: &[BFInstr], workspace: &mut Vec<BFInstr>) -> bool {
		if window.len() == 4 {
			if let BFInstr::DecVal(1) = window[0] {
				if let BFInstr::IncVal(1) = window[2] {
					if let BFInstr::IncPC(dist_a) = window[1] {
						if let BFInstr::DecPC(dist_b) = window[3] {
							if dist_a == dist_b {
								// OPTIMIZES: ->+<
								workspace.push(BFInstr::AddCellValueRight(dist_a));
								return true;
							}
						}
					} else if let BFInstr::DecPC(dist_a) = window[1] {
						if let BFInstr::IncPC(dist_b) = window[3] {
							if dist_a == dist_b {
								// OPTIMIZES: -<+>
								workspace.push(BFInstr::AddCellValueLeft(dist_a));
								return true;
							}
						}
					}
				} else if let BFInstr::DecVal(1) = window[2] {
					if let BFInstr::IncPC(dist_a) = window[1] {
						if let BFInstr::DecPC(dist_b) = window[3] {
							if dist_a == dist_b {
								// OPTIMIZES: ->-<
								workspace.push(BFInstr::SubCellValueRight(dist_a));
								return true;
							}
						}
					} else if let BFInstr::DecPC(dist_a) = window[1] {
						if let BFInstr::IncPC(dist_b) = window[3] {
							if dist_a == dist_b {
								// OPTIMIZES: -<->
								workspace.push(BFInstr::SubCellValueLeft(dist_a));
								return true;
							}
						}
					}
				}
			}
		}
		false
	}
}

fn format_bf_window(window: &[BFInstr]) -> String {
	let mut s = String::new();
	format_bf_window_into(window, &mut s);
	s
}

fn push_repeat_chars(s: &mut String, ch: char, n: usize) {
	if GROUP_REPEAT_PRINTS {
		s.push_str(&format!(">({})", n))
	} else {
		for _ in 0..n { s.push(ch); }
	}
}

fn format_bf_window_into(window: &[BFInstr], s: &mut String){
	for instr in window.iter() {
		match instr {
			&BFInstr::IncPC(n) => push_repeat_chars(s, '>', n),
			&BFInstr::DecPC(n) => push_repeat_chars(s, '<', n),
			&BFInstr::IncVal(n) => push_repeat_chars(s, '+', n),
			&BFInstr::DecVal(n) => push_repeat_chars(s, '-', n),
			&BFInstr::Output(n) => push_repeat_chars(s, '.', n),
			&BFInstr::Input(n) => push_repeat_chars(s, ',', n),
			&BFInstr::LoopStart(_) => s.push('['),
			&BFInstr::LoopEnd(_) => s.push(']'),

			&BFInstr::ZeroCurrentCell => s.push('Z'),
			&BFInstr::AddCellValueRight(dist) => s.push_str(&format!("Ar({})", dist)),
			&BFInstr::AddCellValueLeft(dist) => s.push_str(&format!("Al({})", dist)),
			&BFInstr::SubCellValueRight(dist) => s.push_str(&format!("Sr({})", dist)),
			&BFInstr::SubCellValueLeft(dist) => s.push_str(&format!("Sl({})", dist)),

			&BFInstr::FindZeroCellLeft(step_size) => s.push_str(&format!("Fzl({})", step_size)),
			&BFInstr::FindZeroCellRight(step_size) => s.push_str(&format!("Fzr({})", step_size)),
		}
	}
}