# Rust Patterns

Each pattern names its trigger and gives a sketch.

## 1. Make Invalid States Unrepresentable  *(highest-leverage pattern)*

Encode invariants in types so illegal combinations fail to compile.

```rust
// BAD: fg_color validity depends on monochrome
struct DisplayProps { monochrome: bool, fg_color: RgbColor }

// GOOD
enum Color { Monochrome, Foreground(RgbColor) }
struct DisplayProps { color: Color }
```

Trigger: a comment like "X must be None if Y is true", or a struct of
`Option<T>` fields representing mutually exclusive states.

## 2. Newtype

Wrap a primitive to give it domain meaning and prevent mix-ups.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountId(u64);
// A UserId cannot be passed where an AccountId is expected.
```
- A private field plus a validating constructor makes the newtype an invariant
  carrier, not just a label.
- Conversions via `From`/`TryFrom`; `Deref` only when the newtype is truly
  value-like.

## 3. Builder

For types with many optional/incompatible fields or multi-step construction.

```rust
pub struct Server { host: String, port: u16, tls: bool /* ... */ }

#[derive(Default)]
pub struct ServerBuilder { host: Option<String>, port: Option<u16>, tls: bool }

impl ServerBuilder {
    pub fn host(mut self, h: impl Into<String>) -> Self { self.host = Some(h.into()); self }
    pub fn port(mut self, p: u16) -> Self { self.port = Some(p); self }
    pub fn tls(mut self) -> Self { self.tls = true; self }
    pub fn build(self) -> Result<Server, BuildError> {
        Ok(Server {
            host: self.host.ok_or(BuildError::MissingHost)?,
            port: self.port.unwrap_or(80),
            tls: self.tls,
        })
    }
}
impl Server {
    pub fn builder() -> ServerBuilder { ServerBuilder::default() }
}
```
- `build()` returns `Result` for missing required fields; for compile-time
  required-field checks use `typed-builder` or type-state (§4).
- `#[derive(Default)]` on the builder cuts boilerplate.

## 4. Type-State

Phantom type parameters encode a protocol state into the type, moving runtime
checks to compile time.

```rust
use std::marker::PhantomData;

pub struct Sealed;
pub struct Unsealed;
pub struct Channel<State = Unsealed> { buf: Vec<u8>, _state: PhantomData<State> }

impl Channel<Unsealed> {
    pub fn seal(self) -> Channel<Sealed> { Channel { buf: self.buf, _state: PhantomData } }
}
impl Channel<Sealed> {
    pub fn checksum(&self) -> u32 { /* ... */ }
}
```
Reserve for protocols where a wrong-order call is a real bug: each state
multiplies type complexity.

## 5. RAII / Drop Guard

Tie resource acquisition to object lifetime; release in `Drop`.

```rust
pub struct TempFile { path: PathBuf }
impl Drop for TempFile { fn drop(&mut self) { let _ = std::fs::remove_file(&self.path); } }
```
- Anything owning an OS handle, lock, allocation, or transaction releases it in
  `Drop`, so cleanup happens on early return and panic alike.
- Pair acquisition with a guard type (`MutexGuard`, scoped threads).

## 6. Error-as-Enum with `thiserror`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("invalid format")]
    Invalid(#[from] serde_json::Error),
}
```
Rules: `SKILL.md` core rules, *Errors and panics*.

## 7. Combinator Pipeline

For pure transformations; side-effecting bodies stay `for` loops
(`coding-style §3`).

```rust
// Prefer:
let total: u64 = ids.iter().map(|i| i.0).sum();

// Over:
let mut total = 0u64;
for i in &ids { total += i.0; }
```
And for `Option`/`Result`:
```rust
let name = user.and_then(|u| u.name).ok_or_else(|| Error::NoName)?;
```

## 8. Extension Trait

Add methods to foreign types via a trait.

```rust
pub trait JsonExt {
    fn to_json(&self) -> serde_json::Result<String>;
}
impl<T: Serialize> JsonExt for T {
    fn to_json(&self) -> serde_json::Result<String> { serde_json::to_string(self) }
}
```

## 9. SlotMap / Indexed Storage

Graphs and ASTs with arbitrary sharing: store nodes in a `Vec`/`SlotMap` and
link by `u32` keys. Keys are `Copy`, and the runtime borrow panics of
`Rc<RefCell<_>>` disappear.

## 10. Enum Dispatch over Dynamic Dispatch

A closed set of behaviours: an enum with `match` is exhaustiveness-checked and
usually faster than `Box<dyn Trait>`.

```rust
enum Shape { Circle(f64), Rect(f64, f64) }
impl Shape {
    fn area(&self) -> f64 {
        match self { Shape::Circle(r) => std::f64::consts::PI*r*r, Shape::Rect(w,h)=>w*h }
    }
}
```

## 11. Crate Prelude

Libraries with commonly needed traits ship
`pub mod prelude { pub use crate::{Foo, BarTrait}; }`; `use yourcrate::prelude::*;`
is the one glob import (`coding-style §2`).

## Smell → replacement

| Smell | Replacement |
|-------|-------------|
| `Rc<RefCell<Node>>` tree or graph | indices / `SlotMap` (§9) |
| `Box<dyn Trait>` parameter | `impl Trait` generic; enum for a closed set (§10) |
| Struct of `Option<T>` for exclusive states | enum (§1) |
| Builder that panics in `build()` | `Result` or type-state (§3, §4) |
| `for` + `push` building a pure transformation | iterator + `collect` (§7) |
