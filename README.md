BF
===

Brainfuck interpreter.

Running examples:
---
`cargo run --release -- bf-test/[testcase]`  
Mandelbrot: `cargo run --release -- bf-test/mandelbrot.bf`
Factor: `echo "179424691" | cargo run --release -- bf-test/factor.bf`

With Debug & Timing Info:  
Mandelbrot: `cargo run --release -- -dt bf-test/mandelbrot.bf`

Features:  
- `dverbose`: Prints extra debugging information. For now just prints which small (<128 chars) loops weren't optimized.
- `stats`: For now just prints most run loops.
- `nooptim`: Disables loop optimizations.