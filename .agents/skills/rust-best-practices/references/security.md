# Rust Security

For `unsafe`, FFI, untrusted input, secrets, and dependency changes. Safe Rust
removes memory-safety and data-race bugs; this file covers the residual risk.
Every boundary *fails closed*: invalid input returns an error, never a partial
result or an abort.

## 1. `unsafe`

### Exhaust safe alternatives first

| Need | Safe route |
|------|------------|
| C bindings | an existing `-sys` crate behind a safe wrapper (§2) |
| Speed | measure first (`performance §0`); the safe version is often as fast |
| Bit-pattern conversion | `try_from`, `bytemuck::Pod` |
| Cross-thread sharing | channels, `Arc<Mutex<_>>`, atomics |

### Required when `unsafe` stays

1. **Minimal scope**: the smallest block around the one unsafe operation.
2. **`// SAFETY:`** directly above, naming the precondition and why it holds.
3. **Encapsulation**: a safe API whose private fields and types make the
   invariant impossible to violate from safe code.
4. **`# Safety` doc** on every `unsafe fn`, stating the caller's obligations;
   prefer a safe wrapper so callers never write `unsafe`.
5. **Validate inputs** crossing into the block: lengths, nullness, bit patterns.

Document both **validity** (the bit pattern is legal) and **safety** (logical
invariants). A field the type assumes valid stays private with no `&mut` escape.

### Extra scrutiny

| Operation | Prefer / verify |
|-----------|-----------------|
| `mem::transmute` | a specific conversion API |
| `get_unchecked`, pointer indexing | bounds proven by construction |
| manual `unsafe impl Send/Sync` | written justification of soundness |
| `union` reads | validate the active variant on every access |

## 2. FFI

The boundary is a security perimeter.

- `bindgen` from headers, regenerated in `build.rs`.
- Validate on entry: null pointers, lengths vs buffers, enum discriminants,
  UTF-8, integer ranges; *fail closed* with an error code.
- **Opaque handles**: `Box::into_raw`/`from_raw` plus an explicit free
  function; foreign code sees only the pointer.
- **Ownership**: document who allocates and frees, with which allocator; mixing
  `malloc`/`free` with Rust's allocator is UB.
- **Callbacks** are `extern "C"`; wrap the body in `catch_unwind` or build with
  `panic = "abort"`, since unwinding into C is UB.
- **Strings**: `CStr`/`CString`/`OsStr` with checked conversion.
- **Layout**: `#[repr(C)]` on every struct crossing the boundary.

## 3. Panics on untrusted input

A panic on attacker-influenced input is a denial-of-service. Hard guardrail:
on untrusted input, only the checked form is allowed.

| Panicking | Checked |
|-----------|---------|
| `.unwrap()`, `.expect()` | `?`, `.ok_or(...)?` |
| `v[i]`, `&v[a..b]` | `v.get(i)`, `v.get(a..b)` |
| `a / b`, `a % b`, `+ - *` on sizes | `checked_div`, `checked_rem`, `checked_add`, ... |
| `x as u32` on sizes/offsets | `u32::try_from(x)?` |
| `from_utf8_unchecked`, `char::from_u32_unchecked` | `from_utf8`, `char::from_u32` |

Parser and decoder modules add
`#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]` so the
clippy gate enforces the table.

Money and quantities: integer minor units (`i64`) with checked math, or a
decimal crate; never `f64`.

## 4. Cryptography and secrets

- Cryptography comes from audited crates: `rustls`, `ring`, `RustCrypto`,
  `aws-lc-rs`. Password hashing: `argon2`.
- TLS verification can be disabled only behind an explicitly named
  `danger_*` option that no default enables.
- Secret types zeroize on drop (`zeroize::Zeroizing<T>`), redact in
  `Debug`/`Display` (`"***"`), and implement `Clone` only deliberately.
- Token and MAC comparison is constant-time (`subtle::ConstantTimeEq`).
- Security randomness comes from `OsRng`/`rand::rng()`; seeded RNGs are for
  tests and simulations.

## 5. Deserialization and untrusted input

- Bound input size before parsing; stream unbounded sources.
- Typed structs over `serde_json::Value` (unbounded nesting and size);
  `#[serde(deny_unknown_fields)]` for strict schemas.
- Validate after parse: ranges, discriminants, string lengths.
- Paths from input: canonicalize, then check the allowed prefix.

## 6. Supply chain

After any dependency change, these gates must be green (`cargo deny` only where
the repo has a `deny.toml`):

```bash
cargo audit
cargo deny check
```

Before adding a dependency, check maintainer activity and last release,
dependents, open security issues, transitive footprint (`cargo tree -d`), and
whether a feature flag can scope it. Build scripts and
proc-macros execute at build time with the developer's privileges: audit them
like application code. Commit `Cargo.lock`; `rust-toolchain.toml` pins the
toolchain.

## 7. Concurrency

Safe Rust still allows deadlocks and logic races.

- Atomics default to `SeqCst`; `Relaxed` only for counters and flags with no
  ordering dependency.
- Message passing (`std::sync::mpsc`, `crossbeam`, `tokio::sync`) over shared
  `Mutex<State>`: channels localize the critical section.
- Stress-test with `loom` (`testing §7`).

## 8. Logging and error leakage

- Logs carry identifiers, never secrets, tokens, auth headers, or PII: wrap
  sensitive values in a redacting `Debug` before tracing.
- User-facing error `Display` stays generic; detail goes to structured
  server-side fields.
