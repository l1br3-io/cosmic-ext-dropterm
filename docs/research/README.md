# Research — DropTerm's decision records

Every non-trivial DropTerm feature started as a research record in this
directory: a peer survey with the evidence, a recommendation graded by
precedent, and the alternatives that were rejected and why. The records are
the project's ADRs — this index is their map.

## The rules

- **Records are immutable.** A record captures what was known and decided on
  its date; it is never edited to match later reality. When a decision is
  reversed, a new record supersedes the old one and both stay.
- **A plan record is the maintained exception.** A record that consolidates
  decisions *and* carries their execution roadmap is updated in place while
  its phases run (checklists tick, status lines move, results append, gates
  record verdicts), with its evidence sections as fixed as any record's. The
  records it consolidates move to the archive as superseded, untouched. Its
  header says so; [portability-plan](portability-plan.md) is the current
  example.
- **Status lives here, not in the records.** The tables below are the only
  place that tracks whether a verdict shipped, stalled, or was overtaken —
  the records' "What DropTerm has today" sections are period snapshots and
  are allowed to go stale.
- **A row is a map entry, not a history.** One-sentence verdict, current
  status, where it shipped. The record carries the evidence, the rejected
  alternatives, and the stage-by-stage narrative; this file's git history
  carries whatever a row used to say.
- **Retired records move to `archive/`.** When nothing in a record remains
  pending, it moves to the archive below: the record text is untouched, only
  its link paths are adjusted so they keep resolving, and its row stays in
  the archive table.
- **A new record follows the house format:** a question for a title, a date,
  a TL;DR with the verdict in the first sentence, numbered findings with
  line-level citations into the surveyed sources, a "what DropTerm has
  today" grounding section, a recommendation with rejected alternatives
  spelled out, and sources pinned by repo, commit, and fetch date. Add its
  row here when the verdict lands.

## Decision log

Ordered by record date. "Shipped as" names the code or doc that carries the
verdict today.

| Record | Date | Verdict | Status | Shipped as |
|---|---|---|---|---|
| [status-bar-ux](status-bar-ux.md) | 2026-10-11 | Keep the glance bar and its fixed height; resist CPU/memory/network segments; never auto-hide, degrade gracefully per segment | Shipped — iterating | `src/status_bar.rs` |
| [status-bar-capability](status-bar-capability.md) | 2026-10-17 | The band's next tier is command lifecycle, a scroll-position chip, an ssh-destination path stand-in, and selection idioms; every OSC-fed segment blocked by the `alacritty_terminal` event seam until upstream grows an OSC passthrough — the named revisit trigger | In progress — the OSC-fed tier shipped on the fired trigger (`osc7-cwd`, `osc94-progress`, `osc7501-status`) | `src/status_bar.rs`, the timer in `src/terminal/engine.rs` |
| [documentation-improvement](documentation-improvement.md) | 2026-10-23 | The docs match the gold standards on reference depth; the five gaps that matter are zero screenshots/demos, a packaging section, an unreachable Pages site, no how-to layer, and no FAQ/changelog/sample config | In progress — the how-to/FAQ layer, the releases page, and the generated sample config are the first three to build | the docs additions it seeds (each page carries its own row as it ships) |
| [terminal-images](terminal-images.md) | 2026-11-05 | iTerm2 OSC 1337 first through the existing OSC fork arm, images stored as cells in the scrollback grid; then the kitty graphics subset through the fork's APC parser; Sixel deferred, overlay placements and file/shm reads rejected | Shipped — detection is the last module | the record |
| [bare-word-path-completion](bare-word-path-completion.md) | 2026-11-06 | Bare argument words complete as paths as a fallback (every surveyed peer ships one), directories-only for cd/pushd; menu-only rows, a path-command allowlist, and unconditioned fish-parity ghosting rejected | Proposed | the record; the carve would land in `src/terminal/suggest.rs` + `src/terminal/engine.rs` |
| [paste-image-ux](paste-image-ux.md) | 2026-11-08 | The pasted path stays the grid's only trace; a per-tab images panel carries the five actions; a conditional band chip opens it; the full-size auto-render leaves the paste handler; placeholder-only rejected | Shipped — the config gate and retention (PI-3/PI-4) remain | `src/tabs.rs`, `src/app.rs`, `src/clipboard.rs`, `src/status_bar.rs` |
| [portability-plan](portability-plan.md) | 2026-11-10 | Supersedes gpui-migration and portability-roadmap and carries the never-committed gpui-kit survey: GPUI can replace libcosmic as a full UI-layer rewrite gated on a hardware spike, whose crux is shaping the surface at creation on every show plus the mid-slide keyboard re-grab; platforms tiered (layer-shell Wayland per-release primary, X11/GNOME/macOS milestone, Windows builds-only) over six compile-time seams, runtime plugins rejected; the chrome adopts gpui-kit only with its gpui-pre snapshot as the sole GPUI source | Plan, reviewed 2026-11-10: the Phase 0 reality check, the base spike, the identity flag day, and the config-store move open first; the Kit ruling and its timing are open | the record (its capability map, work packages, Phase 0 checklist, and spike table) |
| [gpui-spike](gpui-spike.md) | 2026-11-13 | The Phase 2 spike says GO: GPUI carries the lifecycle on COSMIC and sway — release show 34 ms against the 130 ms slide, heights honored, the recreate path re-grabs the keyboard at ~one extra show (ruling 3's numbers), the clipboard gate is focus+serial so the anchor dies, dbus coexists (the "never owned" bug was a dropped connection in the harness), and the root workspace absorbs the frontend; carried ledger: fractional visible output on COSMIC plus integer-rounded display metadata, #61420 dormant; checks 12–13 parked on hardware. The Kit pass (same record, "Kit pass" section) then ruled **GO for the pin**: the gpui-pre 0.3.12 set content-verified against zed d2a4fb6 (wayland backend byte-identical, one real delta — wgpu 30.0.2 — invisible in the checks 0–4 re-run), the consumer LayerShell build maps, the overlays kill check is green on both compositors, C3 holds, and the theme generator applies through apply_config — with one carried debt: the Kit's Root auto-border inflates a layer surface 2×20 px (1320×360 for a 1280×320 drawer), needing a one-const Kit patch or accepted non-flush edges | Reported — GPUI GO + Kit GO for the pin (kit-pass done 2026-11-13) | the record (fed into the plan's "Phase 2 results") |
| [kit-root-border](kit-root-border.md) | 2026-11-13 | The border debt the Kit GO carried is ruled: the drawer stays edge-to-edge via a one-const carried patch — a private `[patch]` copy of gpui-component with the Root auto-border zeroed, never a public fork — retiring only when upstream ships a public opt-out; the record is the standing per-bump checklist | Ruled — the patch lands with Phase 3's frontend; the upstream watch outlives it | the record (the durable copy, outliving the spec/task set); later the Phase 3 `[patch]` entry + [development](../development.md)'s carried-patches section |
| [theme-override](theme-override.md) | 2026-11-15 | The theme pipeline's three regressions (the `then`-consumed pin message, the refresh-after ordering, the watcher startup race) are documented in the record, with source keeping only the durable whys per the comment gate | Complete | the record; the surviving comments in `src/app/theme_glue.rs` |

## Archive

Completed, withdrawn, and superseded records with nothing pending. The
records live in `archive/` untouched; their rows stay below for the full
decision history.

| Record | Date | Verdict | Status | Shipped as |
|---|---|---|---|---|
| [panel-visibility](archive/panel-visibility.md) | 2026-10-01 | The panel can stay visible beside the overlay: an exclusive zone of 0 reserves space, so a `respect_panel` knob beats forcing `layer: Top` | Shipped | the `respect_panel` key + the Behavior toggle (`src/config.rs`, `src/settings.rs`) |
| [terminal-de-integration](archive/terminal-de-integration.md) | 2026-10-02 | Notifications, single-instance D-Bus, and cwd inheritance in; shell-integration injection, OSC 133, portals, and Flatpak out — the 13-entry rejection ledger | Shipped (the seeded parts) | `src/notify.rs`, `src/dbus_state.rs`, new-tab cwd inheritance |
| [settings-trigger](archive/settings-trigger.md) | 2026-10-02 | The right-click context menu carries "Settings…"; gear and grab-bar buttons rejected; `preferences-system` is the icon Pop's theme actually has | Shipped | `src/context_menu.rs` |
| [terminal-theming](archive/terminal-theming.md) | 2026-10-02 | Schemes live in config as a named registry, the Application-theme pin drives light/dark, and the per-scheme licensing audit governs what ships; `terminal-colors.d(5)` ruled out | Shipped | `src/terminal/theme_source.rs`, [Theming](../theming.md) |
| [settings-sidebar](archive/settings-sidebar.md) | 2026-10-03 | The settings UI is a libcosmic context drawer, not a sidebar; side-by-side rejected (it would resize the PTY on every open); the second window deleted | Shipped | `src/settings.rs`, the drawer in `src/app.rs`, [Settings](../settings.md) |
| [remote-control-commands](archive/remote-control-commands.md) | 2026-10-03 | Fire-and-forget tab verbs over the single-instance seam; move-tab and query rejected — the seam structurally cannot offer replies | Shipped (a superset) | `ToggleAction` in `src/app/remote.rs`, [CLI](../cli.md) |
| [v3-panel-applet](archive/v3-panel-applet.md) | 2026-10-03 | An out-of-process COSMIC panel applet (desktop-file discovery, read-only D-Bus state) over an SNI tray icon; a dedicated crate dodges wgpu feature unification on panel surfaces | Shipped, removed from the tree 2026-11-15 (the one-crate ruling) | the archive record; `src/dbus_state.rs` now rides the terminal crate's `core` module |
| [tab-title-generation](archive/tab-title-generation.md) | 2026-10-04 | A `tab_title_mode` enum over a template mini-language (rejected as overkill); a user rename wins in every mode; an empty report resets the title | Shipped (a superset — five modes) | `TabTitleMode` in `src/config.rs` |
| [settings-tabs](archive/settings-tabs.md) | 2026-10-05 | The drawer navigates by `segmented_button` tabs; vertical nav, segmented controls, context pages, and strip reuse rejected; `src/settings.rs` stays one file | Shipped (grown to five tabs) | `SettingsTab` pages in `src/settings.rs` |
| [terminal-profile-settings](archive/terminal-profile-settings.md) | 2026-10-05 | Profiles take the named-model core from Windows Terminal and iTerm2 without GUIDs; GNOME's inherit-on-new-tab default deliberately not adopted | Shipped | `src/profile.rs`, the Profiles tab in `src/settings.rs` |
| [tab-context-menu](archive/tab-context-menu.md) | 2026-10-08 | A six-item per-tab menu graded by peer precedent; terminal-scoped verbs excluded (zero precedents), Pin the one extension | Shipped | the per-tab menu in `src/tab_bar.rs` |
| [packaging-and-release](archive/packaging-and-release.md) | 2026-10-10 | Native deb/AUR/nix per the System76 release convention; cargo-dist rejected; the Flathub leg withdrawn (the store listing offered less than the native install) | Withdrawn (flatpak leg, 2026-11-09) | the GitHub-release path (`just release`, `release.yml`) |
| [terminal-autocomplete](archive/terminal-autocomplete.md) | 2026-10-17 | Fish-style history ghost text: the `tcgetpgrp` idle check detects the prompt, per-tab scraped history feeds prefix suggestions, a dim overlay renders them; editor replacement, shell-emitted protocols, and a vte OSC patch rejected | Shipped — stage 2 complete (2026-10-22) | `src/terminal/suggest.rs` |
| [autosuggest-stage2-crosscheck](archive/autosuggest-stage2-crosscheck.md) | 2026-10-21 | The stage-2 rulings re-verified against fish source and every flag-knowledge peer; seven of nine stand, the two contradictions applied before build | Complete | `src/terminal/suggest.rs` |
| [autosuggest-display-mode](archive/autosuggest-display-mode.md) | 2026-10-24 | One numeric idle-delay key (`autosuggestions_delay_ms`, default 0) over Zed's eager/subtle semantics; min-prefix-chars, Clink's list-first split, and an eagerness enum rejected | Shipped (the floors shipped 2026-11-02) | the gate in `src/terminal/suggest.rs`, the engine clock in `src/terminal/engine.rs`, the reveal tick in `src/app.rs` |
| [shell-presence](archive/shell-presence.md) | 2026-10-24 | The dock entry is a never-destroyed 1×1 layer surface with an empty input region no listing can see, behind `show_in_dock` (default hidden); the panel applet is the persistent handle | Shipped | the `show_in_dock` key, the `Anchor` swap in `src/app.rs` |
| [release-orchestration](archive/release-orchestration.md) | 2026-11-01 | Keep the gated skill and adopt its five sharpenings plus the unreleased-changes accumulator; release-please, changesets, RC/nightly channels, and `-dev` versioning rejected | Complete | `.agents/skills/release-orchestration/SKILL.md` |
| [clipboard-ownership](archive/clipboard-ownership.md) | 2026-11-02 | Every clipboard write was focus-coupled below DropTerm, proven live; the fix is DropTerm owning its clipboard I/O over the data-control protocol; patching smithay-clipboard, queueing writes, and waiting on upstream rejected | Shipped | `src/clipboard.rs`, the routed sites in `src/app/tab_actions.rs` + `src/app.rs`, the `Copied` report in `src/status_bar.rs` |
| [autosuggest-warp-parity](archive/autosuggest-warp-parity.md) | 2026-11-02 | Close the Warp delta with a second surface — an explicit Ctrl+Space menu whose Tab/Enter are scoped to the open menu — superseding the menus-deferred rejection; breadth grows down the loadable-file path; spawn generators and AI rejected | Shipped — all four modules | `src/terminal/suggest.rs`, the overlay in `src/terminal/widget.rs`, the engine seam, the keys in `src/config.rs`, [Autosuggestions](../autosuggestions.md) |
| [autosuggest-dynamic-generators](archive/autosuggest-dynamic-generators.md) | 2026-11-02 | Owner-ordered supersession of the spawn rejection: DropTerm-defined read-only generator programs (VS Code's git discipline verbatim), scrubbed env, the tab's `/proc` cwd, 5s deadline, stale-while-revalidate; the spec `script` fields stay rejected | Shipped (suggest-generators, 2026-11-03; docker/systemctl later slices) | the fifth source in `src/terminal/suggest.rs`, the runner in `src/terminal/engine.rs` |
| [autosuggest-alias-resolution](archive/autosuggest-alias-resolution.md) | 2026-11-02 | Owner-ordered supersession of the alias rejection: a user-declared `[autosuggestions.aliases]` map re-targets the specs/catalog providers at the expansion; shell-state extraction and rc-file heuristics rejected | Shipped (suggest-aliases, 2026-11-02) | `resolve_alias` in `src/terminal/suggest.rs`, the live swap in `src/terminal/engine.rs` |
| [completions-seed-audit](archive/completions-seed-audit.md) | 2026-11-03 | The seed's licensing audit: fourteen commands converted from withfig/autocomplete at a pinned commit (MIT, notice preserved), data only — `script` fields structurally excluded | Shipped (suggest-specs-data Task 4, 2026-11-03) | [`data/completions.toml`](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/data/completions.toml), the format reference in [Configuration](../configuration.md) |
| [suggest-menu-layering](archive/suggest-menu-layering.md) | 2026-11-06 | `with_layer` is the proven fix, not a workaround: iced_wgpu renders a layer by primitive kind, so the menu quad could never paint above the grid's text in one layer; the Overlay-trait route is the next step when the menu wants mouse | Complete | the `with_layer` draw site in `src/terminal/widget.rs:1147`, `menu_panel_style` at `src/terminal/widget.rs:2861` |
| [completions-breadth](archive/completions-breadth.md) | 2026-11-07 | Scale the specs table from withfig/autocomplete's static corpus in maintainer-run tranches, embedded in the binary; the parser to reimplement is inshellisense's; fish/zsh sources closed on license and shape | Shipped — all four modules (2026-11-07) | the schema and walker in `src/terminal/suggest/`, the embedded `BUILTIN_SPECS`, the `autosuggestions_completions_extended` key |
| [completions-engine-rea](archive/completions-engine-rea.md) | 2026-11-07 | The engine is a 154-line four-state lexer plus three recursive walkers; a Rust port honoring six spec fields is the live engine's own model; every `script`/`generator` key sits on an `Arg`, so dropping them keeps the whole tree | Shipped — pinned by test | `src/terminal/suggest/walker.rs`, the schema in `src/terminal/suggest.rs` |
| [paste-uri](archive/paste-uri.md) | 2026-11-09 | The paste reads `text/uri-list` first; exactly one qualifying `file:` URI opens the one file, admitted by its own first bytes alone (regular-file gate, one bounded open, 32 MiB refused); multi-image copies, gnome-copied-files, and quoted paths rejected | Shipped (2026-11-09) | `src/clipboard.rs`, the paste section in [Inline images](../images.md) |
| [gpui-migration](archive/gpui-migration.md) | 2026-11-09 | The UI layer can move to GPUI, but it is a full UI-layer rewrite (engine, config model, zbus contract survive); the decision is gated on a hardware spike proving the remaining layer-shell lifecycle parity | Superseded by [portability-plan](portability-plan.md) (2026-11-10) | its spike, carried forward and corrected in the plan |
| [portability-roadmap](archive/portability-roadmap.md) | 2026-11-09 | All platforms in scope, tiered: layer-shell Wayland the per-release primary, X11/GNOME/macOS milestone-secondary, Windows builds-only; six compile-time seams carry the port; a runtime plugin system rejected | Superseded by [portability-plan](portability-plan.md) (2026-11-10) | its roadmap and Phase 0 checklist, carried forward and corrected in the plan |
