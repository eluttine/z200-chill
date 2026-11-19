# z200-chill

`z200-chill` is an open-source compiler and lightweight package/build tool (`z200c`) for the **CHILL** (**ITU-T Z.200**) programming language targeting **Linux**.

CHILL was designed for large, long-lived, high-reliability systems (such as telecommunications switching and event-driven control). `z200-chill` aims to make a practical subset of CHILL approachable, buildable, and testable on modern Linux without requiring legacy proprietary workstations or closed hardware.

## Project Status

Early development / experimental starter layout. The project is building incrementally from a core CHILL subset (modules, procedures, declarations, expressions, conditionals, loops, and configurable integer widths) toward richer language features and runtime support.

## Repository Structure

```text
z200-chill/
├── AGENTS.md           # Guidelines and conventions for coding agents & contributors
├── README.md           # This file
├── src/                # Compiler and z200c CLI implementation
├── runtime/            # Linux runtime support library (libz200rt / libchillrt)
├── tests/              # Parser, semantic, and end-to-end execution tests
├── examples/
│   └── hello/          # Starter CHILL package and manifest example
└── docs/               # Subset notes, manifest format, and design docs
