extern crate clap;
extern crate llvm_sys as llvm;

mod bf;
mod bfllvm;

use clap::{Arg, App};
use std::fs::File;
use std::io::prelude::*;
use std::process::exit;

macro_rules! println_err(
    ($($arg:tt)*) => {{
        writeln!(&mut ::std::io::stderr(), $($arg)*).expect("Failed to write to stderr")
    }}
);
macro_rules! time_op {
    ($op:expr) => ({
        use std::time::SystemTime;
        let now = SystemTime::now();
        $op;
        let _elapsed = now.elapsed().expect("Failed to get elapsed.");
        _elapsed
    })
}

fn open_file(filename: &str) -> File {
    match File::open(&filename) {
        Ok(f) => f,
        Err(err) => {
            println_err!("Failed to open file: {}", err);
            exit(101);
        }
    }
}

fn as_millis(d: std::time::Duration) -> f64 {
	(d.as_secs() as f64) * 1000.0f64 + (d.subsec_nanos() as f64) / 1000000f64
}

fn run_bf_program_llvm<R: Read+Sized>(input: R, show_debug: bool, show_timing: bool) {
    println!("Using LLVM");
    let mut program = bfllvm::BFLLVMProgram::new();
    let compile_dur = time_op! { program.compile(input) };

    if show_debug {
        println!("LLVM IR:");
        println!("==============");
        program.dump_llvm_ir();
        println!("==============");
    }

    if show_timing {
        println!("Compiled In: {:.2}ms", as_millis(compile_dur));
        println!("Running...");
        println!();
        let dur = time_op! { program.run() };
        println!();
        println!("Finished Running In: {:.2}ms", as_millis(dur));
    } else {
        program.run();
    }
}

fn run_bf_program<R: Read+Sized>(input: R, show_debug: bool, show_timing: bool) {
    let mut program = bf::BFProgram::new();
    let compile_dur = time_op! { program.compile(input) };

    let instr_count = program.get_instr_count();
    let reduced_instr_count = program.get_instructions().len();
    let reduced_instr_percent = if instr_count > 0 {
         reduced_instr_count as f32 / instr_count as f32
    } else { 1.0f32 };
    
    if show_debug {
        if cfg!(not(feature = "nooptim")) {
            println!("Optimizations Enabled");
        } else {
            println!("Optimizations Disabled.");
        }

        println!("Program Size: {} instructions [{} after reduction] [{:.2}% reduction]",
            instr_count,
            reduced_instr_count,
            100.0f32 - reduced_instr_percent * 100.0f32
        );

        println!("Loop Count: {} ({} | {:.2}% optimized)",
            program.stats.loop_count,
            program.stats.optimized_loop_count,
            if program.stats.loop_count > 0 {
                (program.stats.optimized_loop_count as f32 / program.stats.loop_count as f32) * 100.0
            } else {100.0f32});
    }

    if show_timing {
        println!("Compiled In: {:.2}ms", as_millis(compile_dur));
        println!("Running...");
        println!();
        let dur = time_op! { program.run() };
        println!();
        println!("Finished Running In: {:.2}ms", as_millis(dur));
    } else {
        program.run();
    }
}

fn main() {
    let matches = App::new("BF Assembler")
        .version("1.0")
        .author("Adolph C.")
        .about("Assembles BF ASM or runs a BF program.")
        .arg(Arg::with_name("debug")
            .short("d")
            .help("Print debug information."))
        .arg(Arg::with_name("time")
            .short("t")
            .help("Print timing information."))
        .arg(Arg::with_name("llvm")
            .short("l")
            .help("Use LLVM."))
        .arg(Arg::with_name("INPUT")
            .help("Sets the input file to use")
            .required(true)
            .index(1))
        .get_matches();
    
    let input = matches.value_of("INPUT").unwrap();
    let show_debug = matches.is_present("debug");
    let show_timing = matches.is_present("time");
    let llvm = matches.is_present("llvm");

    if llvm {
        run_bf_program_llvm(open_file(input), show_debug, show_timing);
    } else {
        run_bf_program(open_file(input), show_debug, show_timing);
    }
    return;
}