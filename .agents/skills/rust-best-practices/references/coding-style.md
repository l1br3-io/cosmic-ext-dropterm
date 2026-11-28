# Rust Coding Style

Conventions beyond what `cargo fmt` and clippy enforce. Ownership, errors,
types, lints, and comments live in `SKILL.md` core rules.

## 1. Naming

Accessors drop the `get_` prefix: `fn name(&self)`, `fn name_mut(&mut self)`.
`get` alone is reserved for fallible lookups (`map.get(k)`).

## 2. Modules and visibility

- Items start private; widen only for a real external caller. `pub(crate)` for
  cross-module internals.
- Import named items: `use std::io::Read;`. The one glob is a crate's own
  `prelude` (`patterns §11`).
- Re-export from `lib.rs` every type that appears in the public API.

## 3. Iterators

- Pure transformations (map, filter, sum, build a collection) are one lazy
  pipeline, collected once at the end with an explicit target (`: Vec<_>`).
- A `for` loop when the body has side effects, early exits across several
  conditions, or a chain that would need nested closures: pick whichever reads
  more plainly.
- Fallible pipelines: `collect::<Result<Vec<_>, _>>()` or `try_for_each` with `?`.

## 4. Traits

Public types derive or implement every standard trait with a natural meaning (`Debug`,
`Clone`, `PartialEq`/`Eq`, `Hash`, `Default`, `Display`, `FromStr`), and
`Copy` for small plain values: downstream crates cannot add them later.

## 5. Macros

- Function, trait, or generic first; a macro only when none of them can express
  it.
- `macro_rules!` for internal repetition; proc-macros for cross-cutting derives,
  budgeted as costly (compile time, debuggability).

## 6. Edition

`Cargo.toml` sets `edition = "2024"` and `rust-version` (the MSRV). Use the
edition's syntax: `let`-`else`, `if let` chains, inline `format!("{x}")`.
