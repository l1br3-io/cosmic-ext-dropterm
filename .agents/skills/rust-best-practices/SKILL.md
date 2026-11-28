---
name: rust-best-practices
description: >
  Idiomatic, safe, performant Rust. Use when writing, reviewing, or refactoring
  Rust code or its Cargo dependencies.
license: MIT
compatibility: Rust edition 2024, Cargo
metadata:
  author: https://github.com/kulapoo
  version: "1.0.0"
  sources: "Effective Rust (David Drysdale); Rust for Rustaceans (Jon Gjengset)"
allowed-tools: Bash(cargo:*) Bash(rustc:*) Bash(rustfmt:*) Bash(just:*) Bash(make:*) Read Write Edit Glob Grep
---

# Rust Best Practices

Citations like `coding-style §3` name a section in `references/`.

## Steps

### 1. Route

Load every reference whose branch the change touches, and only those:

| Branch | Load |
|--------|------|
| Any Rust code | `references/coding-style.md` |
| New types, modules, traits, or public API shape | + `references/patterns.md` |
| `unsafe`, FFI, untrusted input (network, files, env vars, PTY output), secrets, dependency changes | + `references/security.md` |
| Tests | + `references/testing.md` |
| Performance explicitly in scope | + `references/performance.md` |

Done when every matching branch's file is loaded.

### 2. Write or review

Apply the core rules below plus every loaded reference.

- **Writing**: done when the code satisfies every loaded rule, or your final
  report names each departure and why.
- **Reviewing**: done when every changed item has been checked against every
  loaded rule. Each finding cites its rule (`security §3`, or a core-rule heading)
  and names the replacement.

### 3. Gates

Run the repo's own gate recipe when one exists (`just`, `make`, or the command
`AGENTS.md` names). Otherwise run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc --all-features
```

Done when every gate is green. After a dependency change, also run the
supply-chain gates in `security §6`.

## Core rules

### Ownership

- Take the *least powerful reference* that works: `&T` over `&mut T` over `T`.
- Parameters take `&str` or `&[T]`; own (`String`, `Vec<T>`) only to store or
  mutate. Small `Copy` types pass by value. `impl AsRef<_>`/`impl Into<_>` only
  on public convenience APIs: each caller type compiles its own copy.
- `Cow<'_, T>` when a function sometimes borrows, sometimes owns.
- A `clone()` of owned data added to satisfy the borrow checker signals a
  misshapen ownership model: restructure (borrow, indices, `Rc`/`Arc` for
  shared reads). The same holds for a lifetime the borrow checker rejects.
- Graphs, trees with back-edges, and self-references: nodes in a
  `Vec`/`SlotMap`, linked by index (`patterns §9`).

### Errors and panics

- Fallible operations return `Result<T, E>` and propagate with `?`; reach for
  `map`/`and_then`/`ok_or` before a hand-written `match`.
- Libraries: a `thiserror` enum, one variant per actionable failure, sources
  via `#[from]`. Binaries: `anyhow::Result` with
  `.context(...)`.
- Panic (`unwrap`, `expect`, indexing, `/`) only where the state is *provably
  unreachable* from type invariants, or in tests. Write `expect("why this
  holds")`; `clippy::unwrap_used` catches bare `unwrap()` (see *Lints*). On *untrusted
  input*, use the checked forms in `security §3`.

### Types

- Make invalid states *unrepresentable*: a comment like "X is None when Y" is
  an enum waiting to exist (`patterns §1`).
- Newtype domain primitives (`UserId(u64)`), enum over `bool` parameters,
  enum or newtype over stringly-typed identifiers.
- Public error enums are `#[non_exhaustive]`. Other public enums get it only
  when variants are expected to grow: it blocks downstream exhaustive `match`.

### Dispatch

Generics (`impl Trait`, `<T: Trait>`) by default. Closed set of behaviours:
enum + `match` (`patterns §10`). `dyn Trait` when heterogeneous storage or
object safety demands it.

### Lints

New crates start with
this baseline in `Cargo.toml` (`[workspace.lints]` plus `lints.workspace = true`
in a workspace), and `allow-unwrap-in-tests = true` in `clippy.toml`. On an
existing crate, propose the baseline rather than adding it unasked: it can
surface many findings at once.

```toml
[lints.rust]
missing_docs = "warn"                        # libraries only

[lints.clippy]
unwrap_used = "warn"
undocumented_unsafe_blocks = "warn"
allow_attributes = "warn"
allow_attributes_without_reason = "warn"
```

The `-D warnings` gate turns each `warn` into a failure. Silence a lint at the
narrowest scope with `#[expect(clippy::lint, reason = "...")]`: it fails once
the lint stops firing, so the suppression cannot go stale.

### Docs and comments

Before writing or editing any comment, apply `/writing-code-comments`.
Rust-specific additions:

- Every public item gets `///` with a one-line summary, plus `# Errors`,
  `# Panics`, `# Safety` sections where they apply, and the type invariants
  callers or `unsafe` code rely on.
- `# Examples` run as doc tests (`testing §4`).
- A `// SAFETY:` comment precedes every `unsafe` block, naming the precondition
  and why it holds (`security §1`).
- Every `TODO` links an issue: `// TODO(#42): ...`.
