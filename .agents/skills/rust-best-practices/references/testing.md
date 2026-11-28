# Rust Testing

Every test is *hermetic*: it passes on any machine, timezone, and order, with no
real clock, network, RNG, env var, or host filesystem layout.

## 1. Layers

| Layer | Location | Access | Purpose |
|-------|----------|--------|---------|
| Unit | `#[cfg(test)] mod tests` in src | private | edge cases |
| Integration | `tests/*.rs` | public API | wiring, visibility, ergonomics |
| Doc | `///` blocks | public API | examples that stay correct |
| Property | `proptest` | any | invariants over input domains |
| Fuzz | `cargo-fuzz` targets | byte-input fns | untrusted-input robustness |
| Bench | `benches/` (`criterion`) | hot paths | performance regressions |

Every public capability has at least one integration or doc test exercising it
end to end.

## 2. Unit tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() { assert!(parse(b"").unwrap().is_empty()); }

    #[test]
    fn rejects_non_utf8() {
        assert!(matches!(parse(&[0xFF, 0xFE]), Err(ParseError::InvalidUtf8)));
    }
}
```

- Same file as the code, under `#[cfg(test)]`.
- One behaviour per `#[test]`; the name states the scenario.
- Clock, RNG, filesystem, and network are injected.
- Each test runs in under 100 ms; share expensive setup through `LazyLock`.

## 3. Integration tests

- `tests/<concern>.rs` uses only the public API; shared helpers in
  `tests/common/mod.rs`.
- *Hermetic* fixtures: `tempfile` dirs, in-process servers, mock transports.

## 4. Doc tests

- Every public item with non-trivial behaviour has a runnable example.
- `no_run` for side effects, `compile_fail` to prove a misuse is rejected;
  `#`-prefixed lines hide setup.

## 5. Property tests

For any function with a clear input domain:

- **Round-trips**: `parse(format(x)) == x`.
- **Laws**: associativity, identity, monotonicity.
- **Invariants**: the parser returns `Ok` or `Err` on any input; it never panics.

```rust
proptest! {
    #[test]
    fn roundtrip(n in 0u64..) {
        prop_assert_eq!(parse_u64(&n.to_string()).unwrap(), n);
    }
}
```

Commit `proptest-regressions/` so CI replays every shrunk failure.

## 6. Fuzz tests

Parsers, decoders, and anything accepting outside bytes get a target:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| { let _ = my_crate::parse(data); });
```

Each crash becomes a regression unit test.

## 7. Concurrency and async

- `#[tokio::test]` (or the runtime's equivalent) with `.await`.
- Time: `tokio::time::pause()` + `advance`, or an injected `Clock`.
- Lock-free structures: model-check with `loom`; otherwise stress-test with
  many spawned threads or tasks.

## 8. Quality tools

- Coverage (`cargo llvm-cov`): every new line covered; track the trend, not a
  project-wide percentage.
- Mutation testing (`cargo mutants`) finds tests that pass against a wrong impl.
- Snapshot tests (`insta`) for stable serialized output; review snapshot diffs.

## 9. What to test

- Every reachable `Err` arm.
- Boundaries: empty, single, max, overflow.
- Unicode: multibyte, combining characters, invalid UTF-8.
- Malformed input for parsers.
- Concurrency: interleavings, timeouts, cancellation.
- A regression test for every fixed bug, citing the issue.

`#[should_panic(expected = "...")]` is for documented panics (`# Panics`) only;
an expected failure mode returns `Err` and is tested as one.

## 10. Hygiene

Tests restore what they mutate: temp files via `tempfile`, env vars saved and
restored (or avoided), ports bound to `0`. The suite passes under
`cargo test -- --test-threads=1`.

## 11. Extra gates

The standard gates live in `SKILL.md` step 3. Scheduled or nightly CI adds:

```bash
cargo bench --no-run
cargo +nightly fuzz run <target> -- -max_total_time=60
cargo llvm-cov --workspace
```

Matrix across target platforms and both stable and the MSRV toolchain.
