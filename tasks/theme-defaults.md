# Chunk: theme-defaults — first install opens GitHub-themed, following the system

Spec: [SPEC-theme-defaults.md](../SPEC-theme-defaults.md) · Module id:
`theme-defaults` · post-cutover feature, main · spec approved
2026-11-18 · planned 2026-11-18.

**Files touched:** `dropterm/src/core/theme_source.rs` (the GitHub
pair, the registry order, the name constants, the pins),
`dropterm/src/core/config.rs` (the default pair),
`dropterm/src/core/theme_glue.rs` (the unknown-name fallback),
`dropterm/src/core/settings/tests.rs` (the dropdown pins),
`dropterm/src/core/system_mode.rs` (new — the mode cell, the portal
watch), the `core` module list (one line),
`dropterm/src/app_shell.rs` (resolve/boot/pump wiring),
`dropterm/src/terminal/view.rs` (the two pane-side sites),
`docs/configuration.md`, `docs/theming.md`.

## Task td-1: The GitHub pair ships as the default

**Description:** Two new `ColorScheme` consts in `theme_source.rs` —
`GITHUB_DARK` / `GITHUB_LIGHT` — transcribed from GitHub's own
palettes: backgrounds, foreground, and the accent anchors from
Primer (dark canvas `#0D1117`, light canvas `#FFFFFF`, light accent
blue `#0969DA`, dark accent blue `#2F81F7`), the 16 ANSI slots from
github/github-vscode-theme's terminal block (GitHub-maintained, same
Primer family). Real backgrounds, never the `None` system-defer — a
fresh install paints GitHub's canvas, not the opaque-black fallback.
Slots with no upstream value derive through the existing conversion
(dims at 0.66, cursor/bright fg from fg) and the spot-check test says
so. Registry order: **GitHub Dark, GitHub Light first** (the
defaults-first convention kept), the COSMIC pair moves to rows 2-3,
the alphabetical rest follows. The name constants live in
`theme_source.rs` (the schemes' definition site); `config.rs`'s
`Default` uses them; `theme_glue`'s unknown-name fallback resolves to
the GitHub pair (the `cosmic_dark()`/`cosmic_light()` arm flips).
One new agreement pin ties all three sites: registry rows 0-1's names
== the config default constants == the fallback pair. The four hard
pins that assume today's shape update mechanically: the 12-name order
pin (14 now), `previews.len() == 12`, the dropdown options/commit
rows, and the hole-check's `iter().skip(2)` — it skips the
background-less pair **by name**, not index. The chrome accent must
land on the anchor blues: `theme_gen` rides primary on Blue and
accent on BrightBlue, so the entries' slots are chosen to make the
generated accent `#0969DA`/`#2F81F7`, any deviation from the port's
table cited. `docs/configuration.md`: the `color_scheme_*` default
rows and the "falls back to the COSMIC pair" sentence (line 119).

**Acceptance criteria:**
- [x] `by_name` resolves both GitHub names; the order pin carries the
      14-name array, GitHub pair first, COSMIC at 2-3
- [x] Every color slot cited to its upstream token in the spot-check
      tests (Primer or github-vscode-theme); derived slots say so
- [x] Config defaults pin the GitHub pair (constants shared with the
      registry, no second definition); `theme_preference` stays
      `System`
- [x] Unknown scheme name → the GitHub pair, and the agreement pin
      (registry == defaults == fallback) is green
- [x] The generated chrome accent equals `#0969DA` (light) /
      `#2F81F7` (dark), pinned
- [x] Pickers/previews show 14 entries with GitHub swatches; the
      dropdown pins updated; `just check` green

**Verification:**
- [x] `just test` green (the new + updated pins)
- [x] `just check` green

**Dependencies:** none · **Files:** `core/theme_source.rs`,
`core/config.rs`, `core/theme_glue.rs`, `core/settings/tests.rs`,
`docs/configuration.md` · **Scope:** M

## Task td-2: Follow system reads the OS for real

**Description:** New `core/system_mode.rs`: a process-global cell
(`AtomicBool` current-mode + `AtomicU64` revision), initialized dark
— the fallback is also the pre-read state; the pure mapping
`portal_color_scheme_is_dark(u32)` (freedesktop's
`org.freedesktop.appearance color-scheme`: 0 no-preference → dark,
1 prefer-dark → dark, 2 prefer-light → light), unit-pinned; and
`spawn_watch()` — its own std thread with a current-thread tokio
runtime (the `dbus.rs` spawn shape, no coupling to the V1 server's
lifecycle), a zbus session connection, a
`#[proxy] org.freedesktop.portal.Settings` with `ReadOne` +
`SettingChanged`, filtering to the appearance namespace. Every error
path (no session bus, no portal, unknown key, read failure) logs
once and stays dark — byte-identical to today on bare sway. Boot
spawns the watch before the boot theme apply: **fallback-then-
correct** — the apply reads the cell (dark until the read lands; the
app boots hidden, so the correction lands before any human sees a
frame), and the landed value bumps the revision. The three
literal-`true` sites read the cell — `resolve_theme`
(`app_shell.rs:133`), `boot_colors` (`view.rs:2180`),
`PaneRequest::for_command` (`view.rs:2277`) — a global read works in
all three contexts, including the pre-Shell fleet boot. The pump
gains the tick's revision check beside `tick_mem_watch`: on a bump
it runs the same resolve → identity → `pending_theme` block
`drain_config_changes` uses (extract it into one helper both
callers share); `apply_pending_theme` already fans out to Kit tokens
plus every pane, and the per-frame render sites follow through
`resolve_theme` automatically. Tests use a lock-guarded override
seam (`with_system_mode(dark, …)`) so parallel tests can't race the
global. Sweep the four deferral comments ("until the live
system-mode read lands" / "resolves dark until … ships"). Re-truth
`docs/theming.md`: Follow system now tracks the OS through the
portal color-scheme (any compliant desktop, COSMIC included) with a
dark fallback — lines 9-11, 17, 19 (scheme count and defaults), and
line 21's stale libcosmic claim about the COSMIC pair's background
(it falls to opaque black in this build, and now says so). Check
`docs/testing.md`'s rows at 422-432 name no stale scheme. The
owner's manual rows (2026-11-14 ruling) issued as one concise
instruction at the end: fresh no-config install on COSMIC light →
GitHub Light / dark → GitHub Dark; an OS appearance flip mid-run
re-themes without restart (chrome + panes + drawer); preference
Dark → Follow system re-derives on the spot; bare sway (no portal) →
GitHub Dark; a new tab under a light OS carries light colors.

**Acceptance criteria:**
- [ ] The mapping pins (0/1/2) and the error-arm semantics (stay
      dark) are green headless; the watch thread never panics the
      app on a missing bus/portal
- [ ] `resolve_theme` / `boot_colors` / `for_command` follow an
      overridden cell; no literal `true` remains at any site
- [ ] A revision bump sets `pending_theme` and the applied identity
      updates — chrome tokens and every pane re-theme; a
      geometry-only config echo still re-themes nothing (the
      existing gate pin holds)
- [ ] The four deferral comments are gone; `docs/theming.md` states
      the portal truth, the 14 schemes, the GitHub defaults, and the
      dark fallback
- [ ] `just check` green; the owner's row instruction issued, and
      only the owner's reported result recorded

**Verification:**
- [ ] `just test` green (mapping, seam, funnel pins)
- [ ] `just check` green
- [ ] Owner-run rows: the fresh-install pair, the mid-run flip, the
      preference round-trip, bare sway, the light-mode new tab

**Dependencies:** td-1 · **Files:** `core/system_mode.rs` (new),
the `core` module list, `app_shell.rs`, `terminal/view.rs`,
`docs/theming.md` (+ a `docs/testing.md` touch if rows name schemes)
· **Scope:** M

## Checkpoint — first install is GitHub-themed, following the OS

- [ ] Both tasks landed, `just check` green, the owner's rows
      reported green; the accent is GitHub blue on both modes, the
      dark fallback holds on bare sway, and the docs tell the truth.
