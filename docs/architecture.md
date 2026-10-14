# Architecture

How the DropTerm codebase is laid out: two crates, one module inventory, a handful of load-bearing seams, and where the reasoning behind each lives.

## Two crates, one workspace

- **`cosmic-ext-dropterm`** (`src/`) - the terminal itself, built as a lib plus bin. The lib target exists so the applet can share the D-Bus state contract (`src/lib.rs` re-exports `dbus_state`) instead of duplicating it.
- **`cosmic-ext-dropterm-applet`** (`applet/`) - the COSMIC panel applet. A separate process by design: one binary carrying both would unify wgpu features and break panel rendering (the reasoning is in the [research record](research/v3-panel-applet.md)).

## Module inventory

The recurring split is *pure model* vs *wiring*: pure modules hold plain data and functions, free of iced types, unit-testable headless; wiring modules own the runtime halves that need a compositor. Unit tests live inline in `#[cfg(test)]` sibling modules (`src/*/tests.rs`).

| Module | Role |
|---|---|
| `main.rs` | CLI entry: clap verbs (`--is-visible`, the `ToggleAction` subcommands), TERM env setup, the single-instance handoff |
| `app.rs` | The `cosmic::Application`: the message enum, update/view, config subscription - the composition root |
| `app/spawn.rs` | Spawn resolution: shell/cmd/profile decisions and the cwd chains (`plain_spawn_cwd`, `live_preset_cwd`) |
| `app/surface_lifecycle.rs` | Show/hide/slide-out/destroy lifecycle of the layer surface |
| `app/tab_actions.rs` | Context-menu picks and inline renames funneled into app messages |
| `app/remote.rs` | The CLI verbs handled inside the running instance |
| `config.rs` | The 43-key on-disk config (`DropTermConfig`): enums, clamping, shell resolution |
| `context_menu.rs` | The terminal context-menu model - rows, labels, item→`terminal::Action` mapping (pure; picks land in the same path keybinds use) |
| `dbus_state.rs` | The read-only served state: `Visible` and `TabCount` on the `…CosmicExtDropterm.V1` interface |
| `drawer_handle.rs` | The settings drawer's drag-resize handle |
| `geometry.rs` | Pure geometry math plus the single `apply_geometry` surface path every reshape goes through |
| `grab_bar.rs` | The surface-height drag bar |
| `keybinds.rs` | The default chord map plus the per-action `custom_key_binds` overlay (pure) |
| `notify.rs` | Bell → freedesktop desktop notification (direct D-Bus; COSMIC has no Notification portal) |
| `profile.rs` | The profile model and its pure resolution seams (`profile ?? global`, per field) |
| `session.rs` | Session persistence: versioned RON schema, capture/restore planning, corrupt-file quarantine, atomic save (pure file layer) |
| `settings.rs` | The settings drawer: the write contract, the control→key map, five pages, the profile manager form, the keybind editor |
| `slide.rs` | The 130 ms slide: draw-time translation/clip inside a fixed surface - no layer-shell traffic while animating |
| `status_bar.rs` | The 28 px status band: pure segment model plus widget |
| `surface.rs` | Layer-shell surface creation - a measuring pass stretches to the output, then reshapes in place |
| `tab_bar.rs` | The tab strip widget: buttons, `+`, gear, drag reorder, inline rename, per-tab menu, pins, borders |
| `tabs.rs` | The pure tab model: insert/close/reorder/cycle, title fallback |
| `terminal/` | The engine and widget adapted from cosmic-term: `engine.rs` (PTY + `alacritty_terminal` grid + cosmic-text), `widget.rs` (`TerminalBox`), `mouse_reporter.rs`, `theme_source.rs` (the 12-scheme registry with per-scheme license provenance) |
| `applet/` | The applet crate: the panel button (state-driven opacity, tooltip), the D-Bus watches, the click decision |

`main.rs` and `lib.rs` also carry the pedantic-lint table `just clippy` enforces - `standing:` owner-approved exceptions and `transient:` worklist entries that self-clean as their lint class is cleared.

## The load-bearing seams

- **Single-instance D-Bus.** A second launch of the binary never starts a second app: it rides libcosmic's `org.freedesktop.DbusActivation` to the running instance. The CLI verbs (`src/app/remote.rs`) and the applet's toggle use the same activation surface; state flows out read-only through `dbus_state`'s V1 interface. Everything on this seam is fire-and-forget by design - it structurally cannot offer replies ([research](research/remote-control-commands.md)).
- **Config as the source of truth.** Every setting is one RON file under cosmic-config, watched live. The settings drawer is a *writer* with the same contract - write-through per commit point, no Apply button - so drawer edits and hand edits are the same event stream to the app.
- **One geometry path.** Every transition that shapes the surface (show, config reshape, drag) goes through `geometry::apply_geometry`, so no path can leave a stale anchor, margin, or size.
- **One spawn funnel.** `app/spawn.rs` decides what a new tab runs - shell, `--cmd`, or profile - and where it starts, composing `new_tab_cwd`/`default_cwd`/profile cwd with explicit degradation, never an error surface.
- **Theme by name.** Schemes are registry names resolved in `terminal/theme_source.rs`; the profile appearance rule is `profile ?? global` per field, re-resolved on every config pass so live tabs follow edits.

## Testing posture

- **Headless** - the pure modules above carry `#[cfg(test)]` unit tests; `cargo test` (or `just test`) runs them.
- **On hardware** - anything needing a real compositor is verified by the [manual test checklist](testing.md); there is no middle tier by design.
- `just check` (format, clippy `-D warnings`, test) is the change Definition of Done.

## Where the why lives

Module doc-comments carry the local reasoning (dated rulings, gotchas); the feature-level reasoning - peer surveys, recommendations, rejected alternatives - lives in the research records, indexed by the [decision log](research/README.md).
