# Carried patches

Private source copies that replace a published crate through the root
manifest's `[patch.crates-io]` — never public forks. The ledger, the
upgrade ritual, and the verification every bump re-runs live in
[docs/development.md](../docs/development.md)'s carried-patches
section.

| Copy | Replaces | The change |
|---|---|---|
| `gpui-component/` | gpui-component 0.9.1 | one const: the Linux `SHADOW_SIZE` in `src/window_border.rs` zeroed, so a Client-decorated layer surface negotiates its configured size instead of 2×20 px larger |
