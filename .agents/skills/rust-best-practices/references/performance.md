# Rust Performance

Every optimization is *measured*: a claim without a before/after benchmark is a
guess.

## 0. Process

1. Write the clear, idiomatic version first.
2. Measure with a profiler and a benchmark (§11).
3. Change the measured hotspot; re-measure. Keep the change only for a
   reproducible gain that justifies its complexity.
4. Comment non-obvious optimizations with the benchmark that justifies them.

Done when the before/after numbers are in your final report (and the PR
description, if one exists).

## 1. Heap allocation

- `String`, `Vec`, `HashMap`, `Box`, `Rc` allocate; each allocation in a hot
  loop is a candidate for removal.
- Capacity hints (`with_capacity(n)`) when the size is predictable.
- Hot functions write into a caller-supplied `&mut Vec<_>` / `&mut String`
  instead of returning a fresh one.
- Small bounded collections: `[T; N]`, `arrayvec::ArrayVec`,
  `smallvec::SmallVec`.

## 2. Clones

A `.clone()` of `String`/`Vec`/`HashMap` in a loop is a hidden allocation.
Ownership rules: `SKILL.md` core rules, *Ownership*.

## 3. Iterators

- Lazy combinators fuse into one pass: an intermediate `.collect()` in a hot
  path is an allocation to remove (`coding-style §3`).
- `chunks_exact` / `windows` for slices let the compiler drop bounds checks.

## 4. Strings

- Hot paths build with `write!(buf, ...)` into a reused `String`; each `format!`
  allocates.
- Validated ASCII: `as_bytes()` iteration beats `chars()`.
- `Cow<str>` when most inputs pass through unchanged.

## 5. Hashing

- std `HashMap` uses DoS-resistant SipHash. For keys an attacker cannot
  control, swap the hasher (`ahash`, `rustc-hash`) in hot paths.
- A handful of entries: linear scan of `Vec<(K, V)>`.
- `BTreeMap` for ordering or range queries.

## 6. Branches and layout

- Closed sets in hot code: enum dispatch (`patterns §10`).
- `#[inline]` on small hot functions called across crates; the optimizer
  handles the rest.
- Default struct layout already minimizes padding; `#[repr(C)]` is for FFI.
- Large boolean collections: a bitset (`bitvec`, `fixedbitset`).

## 7. Concurrency and parallelism

- CPU-bound: `rayon`. I/O-bound with many connections: async (`tokio`).
- Shared state, when unavoidable: `arc-swap` for read-heavy values, `dashmap`
  or sharded locks over one `Mutex<HashMap>`.
- Bounded channels, so load applies backpressure instead of growing memory.
- Pooled workers (`rayon`, the `tokio` runtime) over spawn-per-item.

## 8. Async

- Keep futures small: drop large locals before `.await`.
- Release a `std::sync::Mutex` guard before `.await`; `tokio::sync::Mutex` only
  when the lock must be held across an await.
- Blocking I/O and CPU-heavy work go to `spawn_blocking`; `block_on` stays out
  of async contexts.
- Independent operations run together via `join!` / `FuturesUnordered`.

## 9. Generic vs dynamic dispatch

Generics monomorphize (no dispatch cost, larger binary); `dyn Trait` costs one
vtable call. When monomorphization bloats binary size or compile time, use a
thin generic front end that calls a non-generic `dyn` inner function.

## 10. Build speed

- Workspace crate splits keep incremental rebuilds cheap.
- `cargo nextest run` for faster test runs; `mold`/`lld` linkers and `sccache`
  for faster dev iteration.
- Release profile: `lto = "thin"`, `codegen-units = 1` (slower build, faster
  binary).

## 11. Measurement tooling

```bash
cargo install flamegraph samply          # profilers, one-time
cargo flamegraph --bin <name>            # CPU flame graph
samply record -- target/release/<name>   # sampling profiler
cargo bench                              # criterion benches in benches/
```

`criterion` is a `[dev-dependencies]` entry with `[[bench]] harness = false`;
compare runs with its saved baselines (`--save-baseline`, `--baseline`).

## Smell → replacement

| Smell | Replacement |
|-------|-------------|
| Optimization with no benchmark | measure first (§0) |
| `Box<dyn Trait>` in a hot loop | generic or enum dispatch (§6, §9) |
| `Vec` grown in a known-size loop | `with_capacity` (§1) |
| Unbounded channel or accumulation under load | bounded channel (§7) |
| `unsafe` for speed | safe version, measured (`security §1`) |
| Blocking call inside an async task | `spawn_blocking` (§8) |
