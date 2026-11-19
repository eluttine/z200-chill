z200-chill/
├── AGENTS.md
├── README.md
├── Cargo.toml            # or go.mod (if implementing z200c in Go)
├── src/
│   ├── main.rs           # or main.go — CLI entrypoint (`z200c`)
│   ├── cli.rs            # Subcommands: init, build, run, test, check
│   ├── manifest.rs       # Package manifest parser & width profile validation
│   ├── lexer.rs          # Tokenizer for CHILL subset
│   ├── parser.rs         # Recursive-descent / Pratt parser -> AST
│   ├── ast.rs            # AST + module/spec/grant/seize representations
│   ├── sema.rs           # Symbol table, visibility checks, mode/width checking
│   └── codegen.rs        # Lowering to target IR / C / LLVM IR
├── runtime/
│   ├── Makefile
│   └── chillrt.c         # Starter Linux runtime (entry/exit, basic I/O, later processes/signals)
├── tests/
│   ├── parse_ok/         # Valid .ch syntax snippets
│   ├── parse_fail/       # Invalid syntax with expected diagnostics
│   ├── sema_fail/        # Mode mismatches, bad visibility, invalid width profiles
│   └── run_ok/           # End-to-end .ch programs + .expected stdout/exit files
├── examples/
│   └── hello/
│       ├── Chill.toml
│       └── src/
│           └── main.ch
└── docs/
    └── subset.md         # Documented Z.200 subset & width profile rules

