// SPDX-License-Identifier: GPL-3.0-only

//! The persisted keybind model — the config file's shape (the
//! `keybinds.ron` section's binding type). The libcosmic action
//! mapping — default chords, `Action` ids, and the runtime `KeyBind`
//! conversion — stays in the terminal frontend's `keybinds` module,
//! which re-exports these types: the model is shared, the
//! toolkit-typed half is not (shared-core spec).

use serde::{Deserialize, Serialize};

/// The serde-friendly modifier names a persisted [`Binding`] carries —
/// cosmic's own `Modifier` is not serializable, so the config shape
/// spells its own four and the terminal frontend converts at the
/// boundary. Ported from cosmic-term.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ModifierName {
    Ctrl,
    Shift,
    Alt,
    Super,
}

/// A persisted key binding: modifier names plus a key name string
/// (`"C"`, `"Tab"`, `"PageUp"`, `"Space"`, …) — the config file's
/// shape. The terminal frontend converts it to its runtime matching
/// type, yielding nothing when the key name does not parse (a
/// hand-edited config can hold one).
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Binding {
    pub modifiers: Vec<ModifierName>,
    pub key: String,
}
