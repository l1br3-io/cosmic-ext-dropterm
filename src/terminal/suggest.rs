// SPDX-License-Identifier: GPL-3.0-only

//! The autosuggest pure model (autosuggest spec): fish-style ghost
//! text computed entirely terminal-side. Everything here is policy
//! over one [`Observation`] of the grid and the kernel's foreground
//! truth — the anchor/ring/capture lifecycle, the eligibility guard
//! chain, the accept splitter, and the ghost tint — with no engine or
//! widget knowledge: the engine (Task 2) builds the observation under
//! the term lock `update()` already holds and owns the [`Suggest`]
//! state; the widget (Task 3) renders the resulting [`Showing`] as an
//! overlay, never in the grid.
//!
//! Failure is always silence: a suggestion shows only when every
//! guard is certain, so any uncertain snapshot degrades to `None`,
//! never to a wrong suffix.

use alacritty_terminal::{
    term::color::Colors,
    vte::ansi::{NamedColor, Rgb},
};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use super::engine::WINDOW_BG_COLOR;

/// The ring's bound (OQ-1, ruled at approval): generous for a
/// drop-down's working set, trivially cheap in memory.
const RING_CAP: usize = 256;

/// Where a displayed suggestion came from (autosuggest spec). The
/// provider seam, ruled at clarification: phase 1 ships exactly one
/// variant, and the pipeline — eligibility, rendering, acceptance —
/// talks to this enum, never to history matching directly, so `Ai`
/// and external providers join additively behind the same match
/// later. Never a plugin registry; never async machinery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SuggestionSource {
    /// The tab's own scraped command ring.
    History,
    /// The filesystem listing of the typed token's parent directory
    /// (path-suggest spec) — local-machine knowledge, gated on the
    /// spawn allowlist.
    Path,
    /// The curated-specs table (flag-specs spec): subcommands and
    /// long flags of a hand-curated command set — universal
    /// knowledge, riding the same spawn allowlist gate by ruling.
    Specs,
    /// The executable names of the spawn-time `$PATH`
    /// (command-catalog spec) — local-machine knowledge, gated on
    /// the same spawn allowlist.
    Catalog,
}

/// How much of the showing suffix an accept key writes (autosuggest
/// spec): the whole remainder (Right), or one fish-shaped forward
/// word (Alt+Right).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptKind {
    /// The entire suffix — Right's accept.
    Full,
    /// One forward word — Alt+Right's accept; the remainder
    /// re-suggests after the echo settles.
    Word,
}

/// One displayed suggestion (autosuggest spec). The suffix is the
/// exact byte string a Right-accept writes through
/// `Terminal::input_no_scroll` — never the grid, never the
/// cosmic-text buffer; inert pixels until an accept key sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Showing {
    /// The full escaped suffix an accept key writes — whole, even
    /// when only a prefix renders.
    pub(crate) suffix: String,
    /// What the widget draws: the longest prefix of `suffix` fitting
    /// the free columns minus two, followed by `…` — or, with fewer
    /// than three free columns, empty (nothing draws; the showing
    /// survives for accept).
    pub(crate) visible: String,
    /// Provenance.
    pub(crate) source: SuggestionSource,
}

/// One update's snapshot of everything the model may look at
/// (autosuggest spec): pure inputs, already reduced — the engine
/// builds it under the term lock `update()` already holds, and the
/// model never touches the grid, the PTY, or the clock.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a snapshot of independent observations; each bool is its own kernel/grid fact, not a state to reshape"
)]
pub(crate) struct Observation<'a> {
    /// `autosuggestions_enabled` as resolved into the terminal's
    /// options.
    pub(crate) enabled: bool,
    /// The shared spawn gate (path-suggest spec, ruled for every
    /// static local provider): the spawned program is an allowlisted
    /// shell. Static local knowledge — path listings, PATH
    /// executables — describes the *local* machine: on a tab spawned
    /// as ssh, `/proc/{shell_pid}/cwd` and this process's `$PATH`
    /// are the wrong machine's truth, so the providers behind this
    /// gate stay silent there and only grid-scraped history keeps
    /// suggesting (remote-truthful). A typed ssh session is already
    /// silent end-to-end via the foreground guard.
    pub(crate) paths_eligible: bool,
    /// The path provider's config gate (path-suggest spec):
    /// `autosuggestions_paths_enabled` as resolved into the
    /// terminal's options — engine-`true` until the config key lands
    /// (PS-4); a live flip applies at the next typed prefix. History
    /// is unaffected: `false` restores phase-1 behavior exactly.
    pub(crate) paths_enabled: bool,
    /// The path provider's reduced listing (path-suggest spec): the
    /// sorted (name, is-a-directory) entries of the trailing token's
    /// canonical parent directory, read engine-side and keyed by that
    /// parent — present only for a path-shaped token on an eligible
    /// tab; `None` is no path-completion context (silence). Borrowed
    /// from the engine's cache; the model never touches the
    /// filesystem.
    pub(crate) path_listing: Option<&'a [(String, bool)]>,
    /// The command-name provider's config gate (command-catalog
    /// spec): `autosuggestions_commands_enabled` as resolved into
    /// the terminal's options — engine-`true` until the config key
    /// lands (CC-3); a live flip applies at the next typed prefix.
    /// History is unaffected: `false` restores phase-1 behavior
    /// exactly.
    pub(crate) commands_enabled: bool,
    /// The curated-specs provider's config gate (flag-specs spec):
    /// `autosuggestions_specs_enabled` as resolved into the
    /// terminal's options — engine-`true` until the config key
    /// lands (the catalog's CC-3 precedent, ruled OQ 4); a live
    /// flip applies at the next typed prefix. History is
    /// unaffected: `false` restores the provider's absence exactly.
    pub(crate) specs_enabled: bool,
    /// The command-name provider's reduced snapshot (command-catalog
    /// spec): the sorted executable basenames of the spawn-time
    /// `$PATH`, walked once at spawn — borrowed from the engine's
    /// per-tab state; the model never touches the filesystem.
    pub(crate) command_catalog: &'a [String],
    /// The terminal's focus state.
    pub(crate) focused: bool,
    /// A foreground child holds the PTY (`tcgetpgrp` vs the shell's
    /// process group): true reads "not at a prompt", for any shell,
    /// transparently over SSH.
    pub(crate) foreground_child: bool,
    /// The grid's display offset: non-zero means the user is scrolled
    /// back into history.
    pub(crate) display_offset: usize,
    /// The cursor sits on the viewport's bottom row — a grid fact the
    /// debug trace prints. No longer a showing guard (the 2026-10-24
    /// fresh-terminal ruling): a new tab's first prompt sits at the
    /// top of an empty grid, and the widget draws the ghost at the
    /// cursor's own row.
    pub(crate) cursor_on_bottom_row: bool,
    /// The grid's column count.
    pub(crate) columns: usize,
    /// The cursor's cell column.
    pub(crate) cursor_column: usize,
    /// The bottom line's cells left of the cursor as text (the
    /// `display_iter` walk's char-per-cell shape, wide spacers
    /// skipped).
    pub(crate) before_cursor: &'a str,
    /// Every cell right of the cursor on the bottom line is blank —
    /// the "cursor at line end" the anchor adoption requires.
    pub(crate) after_cursor_blank: bool,
    /// This update is a known fresh-prompt moment: the update that
    /// closed the spawn settle, or the first update after a
    /// running→idle transition (the settle+finish ruling).
    pub(crate) fresh_prompt: bool,
    /// The idle-delay display gate (suggest-delay spec):
    /// `autosuggestions_delay_ms` as resolved into the terminal's
    /// options — a qualifying candidate is withheld until the typed
    /// segment has been stable this many milliseconds. `0` (the
    /// default) is today's behavior bit-for-bit: fish parity, the
    /// record's twice-proven ruling (VS Code's `minShowDelay`,
    /// ble.sh's `complete_auto_delay`), the remedy opt-in.
    pub(crate) delay_ms: u16,
    /// How long the typed segment has been stable, engine-resolved
    /// against the last `before_cursor` change (suggest-delay spec):
    /// the model never touches a clock, so stability arrives as a
    /// number. Any keystroke that changes the line restarts it, and
    /// a fresh-prompt anchor adoption resets it (a new line is new
    /// typing).
    pub(crate) typed_idle_ms: u16,
}

/// The engine-owned autosuggest state (autosuggest spec): the adopted
/// prompt anchor, the bounded history ring, the continuous idle-line
/// capture, the foreground-diff flag, the current showing, and the
/// idle-delay withheld fact. One per terminal — it dies with the
/// terminal, so a respawn (a fresh terminal) resets history by
/// construction.
pub(crate) struct Suggest {
    anchor: Option<String>,
    adopt_armed: bool,
    capture: String,
    last_foreground_child: bool,
    ring: VecDeque<String>,
    showing: Option<Showing>,
    /// A qualifying candidate is waiting out the idle-delay gate
    /// (suggest-delay spec) — the engine's cue to expose a reveal
    /// deadline and arm the one-shot tick.
    withheld: bool,
}

impl Suggest {
    /// Fresh state, armed for the settle-close adoption: a new
    /// terminal's spawn settle is the first fresh-prompt moment (the
    /// settle+finish ruling), and a fresh terminal is idle.
    pub(crate) fn new() -> Self {
        Self {
            anchor: None,
            adopt_armed: true,
            capture: String::new(),
            last_foreground_child: false,
            ring: VecDeque::new(),
            showing: None,
            withheld: false,
        }
    }

    /// The current showing — the suffix an accept key writes, and the
    /// only thing the widget renders.
    pub(crate) fn showing(&self) -> Option<&Showing> {
        self.showing.as_ref()
    }

    /// Whether a qualifying candidate is waiting out the idle-delay
    /// gate (suggest-delay spec) — the engine's cue that a reveal
    /// deadline exists (its last-change Instant plus the resolved
    /// delay); the model stays clock-free.
    pub(crate) fn withheld(&self) -> bool {
        self.withheld
    }

    /// Whether an anchor is adopted — the engine's settle-prompt
    /// latch clears once a prompt exists.
    pub(crate) fn anchored(&self) -> bool {
        self.anchor.is_some()
    }

    /// The adopted anchor, if one exists — the engine's
    /// typed-stability tracking reads it (a changed anchor is a
    /// fresh adoption: a new line, new typing).
    pub(crate) fn anchor(&self) -> Option<&str> {
        self.anchor.as_deref()
    }

    /// Takes the showing's suffix and clears the showing state: an
    /// accept writes the bytes exactly once, and the next update
    /// re-derives what (if anything) shows after the echo settles —
    /// a word-accept's remainder reappears as the shorter suffix.
    pub(crate) fn take_suffix(&mut self) -> Option<String> {
        let showing = self.showing.take()?;
        Some(showing.suffix)
    }

    /// The typed segment of `before_cursor` — the line minus the
    /// adopted anchor — when the line still starts with it
    /// (path-suggest spec): the engine's carve for the path listing,
    /// the same strip the guard chain performs, so both sides key on
    /// one token.
    pub(crate) fn typed_segment<'a>(&self, before_cursor: &'a str) -> Option<&'a str> {
        before_cursor.strip_prefix(self.anchor.as_deref()?)
    }

    /// Seeds the ring before the first update (history-seed spec):
    /// each entry commits in order, so the caller feeds oldest first
    /// and the most recent lands at the back — the same seat live
    /// captures take. Reuses [`commit`]'s dedupe, promote, and cap, so
    /// a seeded ring is indistinguishable from a well-lived-in one.
    pub(crate) fn seed_history(&mut self, entries: impl IntoIterator<Item = String>) {
        for entry in entries {
            commit(&mut self.ring, &entry);
        }
    }
}

impl Default for Suggest {
    fn default() -> Self {
        Self::new()
    }
}

/// The eligibility step (autosuggest spec) — the whole lifecycle over
/// one observation, pure: diff the foreground (idle→running commits
/// the freshest capture to the ring; running→idle arms anchor
/// adoption), adopt the anchor at a fresh-prompt moment with the
/// cursor at line end, refresh the capture while idle and anchored,
/// then derive the showing through the guard chain. Every guard that
/// cannot hold returns `None`: a suggestion shows only when every
/// condition is certain, so failure is always silence, never a wrong
/// suffix.
pub(crate) fn step(obs: &Observation<'_>, suggest: &mut Suggest) -> Option<Showing> {
    // The foreground diff: idle→running is Enter — the freshest
    // captured line is the command that just ran; running→idle is a
    // finish — the next fresh-prompt observation may adopt.
    let was_running = suggest.last_foreground_child;
    suggest.last_foreground_child = obs.foreground_child;
    if !was_running && obs.foreground_child {
        commit(&mut suggest.ring, &suggest.capture);
    } else if was_running && !obs.foreground_child {
        suggest.adopt_armed = true;
    }

    // Anchor adoption (the settle+finish ruling): at a fresh-prompt
    // moment, idle, with the cursor at line end and the line ending
    // in whitespace — a bare prompt by near-universal convention. The
    // whitespace tail is the type-ahead guard: buffered keystrokes
    // echoed into the same update as the prompt redraw leave a
    // half-typed line that must not become the anchor.
    if suggest.adopt_armed
        && obs.fresh_prompt
        && !obs.foreground_child
        && obs.after_cursor_blank
        && obs.before_cursor.ends_with(char::is_whitespace)
    {
        suggest.anchor = Some(obs.before_cursor.to_string());
        suggest.adopt_armed = false;
    }

    // The capture refresh: while idle and anchored, the typed segment
    // (the line minus the anchor) is the freshest command candidate,
    // re-read every update so the commit at the next idle→running
    // transition records exactly what Enter ran. A line that left the
    // anchor leaves the capture standing.
    if !obs.foreground_child
        && let Some(anchor) = &suggest.anchor
        && let Some(typed) = obs.before_cursor.strip_prefix(anchor)
    {
        suggest.capture = typed.to_string();
    }

    let outcome = showing(obs, suggest);
    suggest.showing = outcome.shown().cloned();
    suggest.withheld = outcome.is_withheld();
    suggest.showing.clone()
}

/// The guard chain's outcome (suggest-delay spec): silence, a
/// qualifying candidate the idle-delay gate holds back, or a showing
/// the widget may draw. The withheld fact is the model's whole answer
/// to "does a candidate wait?" — the engine pairs it with its own
/// last-change Instant to expose the reveal deadline, keeping every
/// clock out of the model.
enum Outcome {
    /// A guard failed or no provider answered: nothing exists to
    /// show, now or at any deadline.
    Silent,
    /// A candidate qualified under the whole chain — provenance
    /// known, only the reveal waits: the typed segment has not been
    /// stable for `delay_ms` milliseconds.
    Withheld,
    /// A showing past every gate, the delay included.
    Shown(Showing),
}

impl Outcome {
    /// The showing past every gate — `None` while a candidate waits
    /// or nothing qualifies.
    fn shown(&self) -> Option<&Showing> {
        match self {
            Outcome::Shown(showing) => Some(showing),
            _ => None,
        }
    }

    /// Whether a qualifying candidate waits out the idle delay.
    fn is_withheld(&self) -> bool {
        matches!(self, Outcome::Withheld)
    }
}

/// The guard chain (autosuggest spec): every condition must hold, and
/// each one's failure alone means silence — config off, unfocused, a
/// foreground child, scrolled back, unanchored, a line that left the
/// anchor, an empty typed segment, or no candidate from any provider.
/// The candidate itself comes from the merged seam's dispatch; the
/// guards apply after the merge, unchanged. A candidate younger than
/// the idle delay is withheld, not silent (suggest-delay spec) — the
/// outcome carries the difference so the engine can expose a reveal
/// deadline. A too-wide suffix no longer silences (ghost-fit spec):
/// the display clips to the free columns, and the accept stays whole.
/// No bottom-row requirement (the 2026-10-24 fresh-terminal ruling):
/// a new tab's first prompt sits near the top of an empty grid, the
/// widget draws the ghost at the cursor's own row, and the chain's
/// geometry needs — not scrolled back, the cursor at line end — are
/// both still checked.
fn showing(obs: &Observation<'_>, suggest: &Suggest) -> Outcome {
    if !obs.enabled || !obs.focused || obs.foreground_child || obs.display_offset != 0 {
        return Outcome::Silent;
    }
    let Some(anchor) = suggest.anchor.as_ref() else {
        return Outcome::Silent;
    };
    let Some(typed) = obs.before_cursor.strip_prefix(anchor) else {
        return Outcome::Silent;
    };
    if typed.is_empty() {
        return Outcome::Silent;
    }
    let Some((suffix, source)) = merged_candidate(typed, suggest, obs) else {
        return Outcome::Silent;
    };
    // The display gate (suggest-delay spec): the candidate still
    // computes — the model is pure and free — but a nonzero delay
    // withholds the showing until the typed segment has been stable
    // for that many milliseconds; any keystroke restarts the wait by
    // resetting the engine-resolved idle fact. Default 0 is today's
    // behavior bit-for-bit (fish parity, the record's ruling).
    if obs.typed_idle_ms < obs.delay_ms {
        return Outcome::Withheld;
    }
    // The fit is grid cells, not chars: the ghost starts one cell
    // past the cursor, and a too-wide one clips to what fits rather
    // than wrapping onto a grid row it does not own.
    let remaining = obs.columns.saturating_sub(obs.cursor_column + 1);
    let visible = clip_visible(&suffix, remaining);
    Outcome::Shown(Showing {
        suffix,
        visible,
        source,
    })
}

/// The merge policy, ruled at clarification: history answers first —
/// the tab's own truth — then path, then the curated specs, then the
/// command catalog. One pure function of the provider variant,
/// pinned by its test, so the dispatch derives its order here and
/// later modules cannot reorder the merge by accident.
fn source_rank(source: SuggestionSource) -> usize {
    match source {
        SuggestionSource::History => 0,
        SuggestionSource::Path => 1,
        SuggestionSource::Specs => 2,
        SuggestionSource::Catalog => 3,
    }
}

/// History's candidate function — the exact phase-1 ring walk,
/// unchanged, moved behind the provider seam: the most recent entry
/// with the typed line as a strict prefix, its remainder the
/// candidate suffix (fish's backbone, zsh-autosuggestions' default
/// history strategy).
fn history_candidate(typed: &str, ring: &VecDeque<String>) -> Option<String> {
    let entry = ring
        .iter()
        .rev()
        .find(|entry| entry.starts_with(typed) && entry.len() > typed.len())?;
    Some(entry[typed.len()..].to_string())
}

/// The path provider's candidate function (path-suggest spec):
/// complete the trailing token's final component from the parent's
/// reduced listing — a unique match completes fully (directories
/// suffixed `/`), an ambiguous field completes the **longest prefix
/// every match shares** (the fish common-prefix rule: a guess among
/// live alternatives is a wrong suffix half the time, the shared
/// prefix is right always), and no shared extension is silence.
/// Dotfiles match only a `.`-prefixed typed component (the shell
/// convention). Gated on the config toggle, the spawn allowlist, and
/// the listing itself; the escape rule decides the rest — an entry
/// that cannot be safely written is silence, never a wrong suffix.
fn path_candidate(typed: &str, obs: &Observation<'_>) -> Option<String> {
    if !obs.paths_enabled || !obs.paths_eligible {
        return None;
    }
    let token = path_token(Some(typed))?;
    let listing = obs.path_listing?;
    // The final component: after the last `/` — or the empty
    // component when the token is the `~` root alone.
    let component = match token.rfind('/') {
        Some(idx) => &token[idx + 1..],
        None => "",
    };
    let matches: Vec<&(String, bool)> = listing
        .iter()
        .filter(|(name, is_dir)| {
            name.starts_with(component)
                && (*is_dir || name.len() > component.len())
                && (component.starts_with('.') || !name.starts_with('.'))
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    let completion = if matches.len() == 1 {
        let (name, is_dir) = matches[0];
        let mut remainder = name[component.len()..].to_string();
        if *is_dir {
            remainder.push('/');
        }
        remainder
    } else {
        // The listing is sorted, so the first and last matches bound
        // every match: their shared prefix is the field's shared
        // prefix.
        let prefix = shared_prefix(&matches[0].0, &matches[matches.len() - 1].0);
        if prefix.len() <= component.len() {
            return None;
        }
        prefix[component.len()..].to_string()
    };
    escape_for_shell(&completion)
}

/// The longest prefix two names share, cut on a char boundary — the
/// ambiguous field's common completion.
fn shared_prefix<'a>(first: &'a str, last: &'a str) -> &'a str {
    let mut end = first.len().min(last.len());
    while end > 0 && (!first.is_char_boundary(end) || !last.starts_with(&first[..end])) {
        end -= 1;
    }
    &first[..end]
}

/// The command-name provider's candidate function (command-catalog
/// spec): complete the command word at the command position from the
/// spawn-time PATH snapshot — among names with the typed word as a
/// strict prefix, the shortest wins, ties broken alphabetically (the
/// ruled policy: `gi` suggests `git` ahead of `giftopnm`); the match
/// must extend the word, so an exactly-typed name yields nothing.
/// The suffix rides the same escape rule as paths: a name the shell
/// would mangle escapes, one outside the safe sets silences. Gated
/// on the config toggle and the shared spawn allowlist (the catalog
/// describes the local machine); argument position and path-shaped
/// words never consult it — and neither does a one-character word
/// (the no-information floor, suggest-accuracy spec: `gi` → `git` is
/// the floor's own threshold).
fn catalog_candidate(typed: &str, obs: &Observation<'_>) -> Option<String> {
    if !obs.commands_enabled || !obs.paths_eligible {
        return None;
    }
    if !is_command_position(typed) {
        return None;
    }
    let word = typed.trim_start();
    if word.is_empty() || is_path_shaped(word) {
        return None;
    }
    // The no-information floor (suggest-accuracy spec): a one-character
    // strict prefix over the uncurated PATH namespace carries no intent —
    // shortest-wins among hundreds of names is a coin flip. Two
    // characters is the flagship's own threshold: `gi` → `git`.
    if word.chars().count() < 2 {
        return None;
    }
    // The snapshot is sorted, so `min_by_key` over the length keeps
    // the alphabetically-first of equal-length ties.
    let name = obs
        .command_catalog
        .iter()
        .filter(|name| name.starts_with(word) && name.len() > word.len())
        .min_by_key(|name| name.len())?;
    escape_for_shell(&name[word.len()..])
}

/// One curated command's completion table (flag-specs spec): the
/// hand-curated high-traffic subcommands and long flags — every
/// entry ASCII, non-empty, and trimmed by construction, pinned by
/// test, so a table entry is safe to write by construction. The
/// lists are sorted, which is what makes the ruled tiebreak
/// (shortest wins, ties alphabetical) fall out of `min_by_key` the
/// same way the sorted catalog's does.
struct CommandSpec {
    /// The command word this entry completes.
    command: &'static str,
    /// Its high-traffic subcommands.
    subcommands: &'static [&'static str],
    /// Its long flags — uniform across subcommands in v1 (the flat
    /// table; per-subcommand flag trees are the growth path's, not
    /// v1's). No short flags, ruled: a single-choice ghost pays a
    /// wrong accept for a wrong guess.
    flags: &'static [&'static str],
}

/// The curated-specs table (flag-specs spec, OQ 5): the ruled v1
/// five — git, cargo, docker, systemctl, npm. Curated means
/// curated: no attempt at completeness. Static in-code data by
/// ruling: no runtime derivation from `--help`, no user-editable
/// file; the lookup sits behind `specs_candidate` alone, so the
/// growth path (a loadable data file) touches neither merge nor
/// guards.
const SPECS: &[CommandSpec] = &[
    CommandSpec {
        command: "cargo",
        subcommands: &[
            "build", "check", "clean", "doc", "fetch", "fix", "fmt", "init", "install", "publish",
            "run", "test", "update",
        ],
        flags: &[
            "--bin",
            "--frozen",
            "--lib",
            "--locked",
            "--manifest-path",
            "--offline",
            "--quiet",
            "--release",
            "--verbose",
        ],
    },
    CommandSpec {
        command: "docker",
        subcommands: &[
            "build", "commit", "cp", "create", "exec", "images", "inspect", "logs", "pull", "push",
            "rm", "rmi", "run", "start", "stop", "version",
        ],
        flags: &[
            "--detach",
            "--env",
            "--interactive",
            "--name",
            "--publish",
            "--tty",
            "--volume",
        ],
    },
    CommandSpec {
        command: "git",
        subcommands: &[
            "add",
            "branch",
            "checkout",
            "cherry-pick",
            "clone",
            "commit",
            "diff",
            "fetch",
            "init",
            "log",
            "merge",
            "mv",
            "pull",
            "push",
            "rebase",
            "reset",
            "restore",
            "revert",
            "rm",
            "show",
            "stash",
            "status",
            "switch",
            "tag",
        ],
        flags: &[
            "--all",
            "--amend",
            "--branch",
            "--dry-run",
            "--force",
            "--global",
            "--help",
            "--interactive",
            "--message",
            "--quiet",
            "--set-upstream",
            "--verbose",
            "--version",
        ],
    },
    CommandSpec {
        command: "npm",
        subcommands: &[
            "ci",
            "config",
            "install",
            "publish",
            "run",
            "test",
            "uninstall",
            "update",
        ],
        flags: &[
            "--global",
            "--legacy-peer-deps",
            "--prefix",
            "--registry",
            "--save-dev",
            "--save-exact",
            "--verbose",
        ],
    },
    CommandSpec {
        command: "systemctl",
        subcommands: &[
            "daemon-reload",
            "disable",
            "enable",
            "restart",
            "start",
            "status",
            "stop",
        ],
        flags: &["--force", "--host", "--no-pager", "--quiet", "--type"],
    },
];

/// The specs provider's context carve (flag-specs spec): the line's
/// command word and the trailing token, when the token holds a
/// seat the table answers. A `--`-prefixed token is the **flag
/// seat**, consulted from any argument seat (OQ 2: git accepts
/// flags before and after subcommands alike — the flat table's
/// approximation of fish's per-subcommand gating, which is where
/// the growth path refines if the table ever grows flags worth
/// separating). Any other token is the **subcommand seat**, the
/// first argument seat only (ruled: once an argument holds it, a
/// later word never consults the subcommand table). `None` is no
/// specs context: the command word itself being entered (the
/// catalog's seat) or a later non-flag argument seat.
fn specs_context(typed: &str) -> Option<(&str, &str)> {
    if is_command_position(typed) {
        return None;
    }
    let token = trailing_token(typed);
    let completed = completed_words(&typed[..typed.len() - token.len()]);
    let command = *completed.first()?;
    if token.starts_with("--") || completed.len() == 1 {
        Some((command, token))
    } else {
        None
    }
}

/// The typed segment's completed shell words (flag-specs spec):
/// split on unescaped whitespace — a backslash-escaped space keeps
/// its word open, the same carve `is_command_position` and
/// `trailing_token` perform. The trailing (still-open) word is not
/// included.
fn completed_words(typed: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (idx, c) in typed.char_indices() {
        if c.is_whitespace() && !escaped {
            if idx > start {
                words.push(&typed[start..idx]);
            }
            start = idx + c.len_utf8();
        }
        escaped = c == '\\';
    }
    if start < typed.len() {
        words.push(&typed[start..]);
    }
    words
}

/// The curated-specs provider's candidate function (flag-specs
/// spec): complete the trailing token from the curated table —
/// subcommands at the subcommand seat, `--`-prefixed long flags
/// from any argument seat (no short flags, ruled: the collision
/// space with bundled short flags is guesswork in a single-choice
/// ghost). Strict prefix, must extend, shortest wins, ties
/// alphabetical — the catalog's ruled match policy reused verbatim
/// (the table's sorted lists make `min_by_key` keep the
/// alphabetically-first of equal-length ties). A bare trailing
/// space is no word at all (the catalog's empty-word rule, one
/// policy across providers), and the suffix rides
/// `escape_for_shell` like every provider even though the table is
/// safe by construction. Gated on the config toggle and the shared
/// spawn allowlist (OQ 1: one gate is simpler to explain); an
/// unknown command is silence, never a guess — and so is a
/// flag-seat token of exactly `--`, the no-information floor
/// (suggest-accuracy spec: a bare `--` matches every flag equally;
/// `--a` and longer discriminate).
fn specs_candidate(typed: &str, obs: &Observation<'_>) -> Option<String> {
    if !obs.specs_enabled || !obs.paths_eligible {
        return None;
    }
    let (command, token) = specs_context(typed)?;
    if token.is_empty() {
        return None;
    }
    // The no-information floor (suggest-accuracy spec): a token of
    // exactly `--` matches every long flag in the table equally — the
    // tiebreak picks one with no evidence. One character past the
    // dashes (`--a`) is a strict prefix that discriminates, and stays
    // ruled behavior.
    if token == "--" {
        return None;
    }
    let spec = SPECS.iter().find(|spec| spec.command == command)?;
    let entries = if token.starts_with("--") {
        spec.flags
    } else {
        spec.subcommands
    };
    let entry = entries
        .iter()
        .filter(|entry| entry.starts_with(token) && entry.len() > token.len())
        .min_by_key(|entry| entry.len())?;
    escape_for_shell(&entry[token.len()..])
}

/// The characters a suggested suffix backslash-escapes (path-suggest
/// spec, ruled at clarification): whitespace and the POSIX-and-fish
/// shell metacharacters — the bytes that are certainly safe to write
/// once escaped.
const ESCAPED_CHARS: [char; 19] = [
    ' ', '\t', '(', ')', '&', ';', '<', '>', '*', '?', '[', ']', '#', '$', '~', '!', '"', '\'',
    '\\',
];

/// The conservative printable whitelist's punctuation (path-suggest
/// spec): alongside ASCII alphanumerics, these ride verbatim —
/// neither the shells nor fish reinterprets them inside a word.
const WHITELIST_PUNCT: &str = "-_./:=,+@%^{}";

/// Escapes one matched entry's remainder for the shell (the ruled
/// escape sets, extended by the quote tier; the path and command-name
/// providers ride it alike), in two tiers: ASCII alphanumerics and
/// the whitelist ride verbatim with the escapable set backslashed —
/// byte-identical to the phase-1 form for everything it covered —
/// and a remainder that reaches past both sets, when every character
/// is printable and quote-free, rides whole inside single quotes
/// instead (`café` becomes `'café'`). Single quotes are literal to
/// the POSIX shells and fish alike, so the quoted form is certainly
/// safe to write where a bare one is not. What still silences: a
/// control character (a newline, a bell), and a name needing the
/// quote tier that itself contains a quote — never a wrong suffix.
/// The suffix-fit rule measures whichever escaped form wins.
fn escape_for_shell(raw: &str) -> Option<String> {
    let verbatim = |c: char| c.is_ascii_alphanumeric() || WHITELIST_PUNCT.contains(c);
    if raw
        .chars()
        .all(|c| verbatim(c) || ESCAPED_CHARS.contains(&c))
    {
        let mut escaped = String::with_capacity(raw.len());
        for c in raw.chars() {
            if verbatim(c) {
                escaped.push(c);
            } else {
                escaped.push('\\');
                escaped.push(c);
            }
        }
        return Some(escaped);
    }
    if raw.chars().all(|c| !c.is_control() && c != '\'') {
        return Some(format!("'{raw}'"));
    }
    None
}

/// One provider's candidate function, keyed by its variant — the
/// dispatch table a later module extends with its arm (and its
/// variant in the list `merged_candidate` walks) without touching
/// any call site.
fn provider_candidate(
    source: SuggestionSource,
    typed: &str,
    suggest: &Suggest,
    obs: &Observation<'_>,
) -> Option<String> {
    match source {
        SuggestionSource::History => history_candidate(typed, &suggest.ring),
        SuggestionSource::Path => path_candidate(typed, obs),
        SuggestionSource::Specs => specs_candidate(typed, obs),
        SuggestionSource::Catalog => catalog_candidate(typed, obs),
    }
}

/// The merged seam's dispatch (suggest-merge): every provider, in
/// `source_rank` order — the sort is the priority, never a hand
/// order — and the first non-`None` candidate wins, its provenance
/// carried in the pair. Exactly one suggestion or none (autosuggest
/// spec).
fn merged_candidate(
    typed: &str,
    suggest: &Suggest,
    obs: &Observation<'_>,
) -> Option<(String, SuggestionSource)> {
    let mut providers = [
        SuggestionSource::History,
        SuggestionSource::Path,
        SuggestionSource::Specs,
        SuggestionSource::Catalog,
    ];
    providers.sort_by_key(|&source| source_rank(source));
    providers.into_iter().find_map(|source| {
        provider_candidate(source, typed, suggest, obs).map(|suffix| (suffix, source))
    })
}

/// One completion-menu row (suggest-menu spec): the label the menu
/// displays — the completed whole, a full history line or a whole
/// table entry or PATH name — beside the suffix an accept writes
/// through `Terminal::input_no_scroll` (the one funnel, the same
/// escaped bytes the ghost's accepts carry), a description when a
/// source carries one (none today; the specs data file in
/// suggest-specs-data is the supplier), and the provenance the
/// merge ranking orders by.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "caller-less until Task 3's key routing lands")
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MenuCandidate {
    pub(crate) label: String,
    pub(crate) suffix: String,
    pub(crate) description: Option<String>,
    pub(crate) source: SuggestionSource,
}

/// The menu's candidate collection (suggest-menu spec): every
/// provider's **whole** strict-prefix set — where the single-choice
/// ghost must settle for one answer (the newest match, the shared
/// prefix, the shortest name), the menu carries every match —
/// concatenated under the merge's ruled priority ([`source_rank`],
/// history on top). Each provider rides its own gates unchanged
/// (config toggles, the spawn allowlist, the seat rules); pure, like
/// every seam here. Empty input is silence — no token, no list.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the menu's open path lands in Task 3; tests consume it today"
    )
)]
pub(crate) fn candidates(
    typed: &str,
    suggest: &Suggest,
    obs: &Observation<'_>,
) -> Vec<MenuCandidate> {
    if typed.is_empty() {
        return Vec::new();
    }
    let mut providers = [
        SuggestionSource::History,
        SuggestionSource::Path,
        SuggestionSource::Specs,
        SuggestionSource::Catalog,
    ];
    providers.sort_by_key(|&source| source_rank(source));
    providers
        .into_iter()
        .flat_map(|source| source_candidates(source, typed, suggest, obs))
        .collect()
}

/// One provider's whole candidate set, keyed by its variant — the
/// menu-side twin of [`provider_candidate`]'s dispatch.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "reached only through candidates; live at Task 3")
)]
fn source_candidates(
    source: SuggestionSource,
    typed: &str,
    suggest: &Suggest,
    obs: &Observation<'_>,
) -> Vec<MenuCandidate> {
    match source {
        SuggestionSource::History => history_candidates(typed, &suggest.ring),
        SuggestionSource::Path => path_candidates(typed, obs),
        SuggestionSource::Specs => specs_candidates(typed, obs),
        SuggestionSource::Catalog => catalog_candidates(typed, obs),
    }
}

/// History's whole set: every ring entry with the typed line as a
/// strict prefix, most recent first — the ghost's own walk continued
/// past the first hit, bounded by the ring's own cap.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "reached only through candidates; live at Task 3")
)]
fn history_candidates(typed: &str, ring: &VecDeque<String>) -> Vec<MenuCandidate> {
    ring.iter()
        .rev()
        .filter(|entry| entry.starts_with(typed) && entry.len() > typed.len())
        .map(|entry| MenuCandidate {
            label: entry.clone(),
            suffix: entry[typed.len()..].to_string(),
            description: None,
            source: SuggestionSource::History,
        })
        .collect()
}

/// The path provider's whole set (suggest-menu spec): every listing
/// entry matching the typed token's final component under the
/// ghost's own rules — the same gates, the same component carve, the
/// same dotfile convention — each completing to its own escaped
/// remainder (directories suffixed `/`), where the ghost must settle
/// for the one shared prefix. An entry whose remainder cannot be
/// safely written drops alone (the per-entry silence rule), never
/// poisons the set.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "reached only through candidates; live at Task 3")
)]
fn path_candidates(typed: &str, obs: &Observation<'_>) -> Vec<MenuCandidate> {
    if !obs.paths_enabled || !obs.paths_eligible {
        return Vec::new();
    }
    let Some(token) = path_token(Some(typed)) else {
        return Vec::new();
    };
    let Some(listing) = obs.path_listing else {
        return Vec::new();
    };
    // The final component: after the last `/` — or the empty
    // component when the token is the `~` root alone.
    let component = match token.rfind('/') {
        Some(idx) => &token[idx + 1..],
        None => "",
    };
    listing
        .iter()
        .filter(|(name, is_dir)| {
            name.starts_with(component)
                && (*is_dir || name.len() > component.len())
                && (component.starts_with('.') || !name.starts_with('.'))
        })
        .filter_map(|(name, is_dir)| {
            let mut remainder = name[component.len()..].to_string();
            if *is_dir {
                remainder.push('/');
            }
            let suffix = escape_for_shell(&remainder)?;
            Some(MenuCandidate {
                label: if *is_dir {
                    format!("{name}/")
                } else {
                    name.clone()
                },
                suffix,
                description: None,
                source: SuggestionSource::Path,
            })
        })
        .collect()
}

/// The curated-specs provider's whole set (suggest-menu spec): every
/// table entry with the trailing token as a strict prefix —
/// subcommands at the subcommand seat, long flags from any argument
/// seat. The bare `--` floor is the single-choice ghost's alone (a
/// list of every flag is information, not a guess), so the flag seat
/// lists the table whole. Each remainder rides the escape rule like
/// every provider's.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "reached only through candidates; live at Task 3")
)]
fn specs_candidates(typed: &str, obs: &Observation<'_>) -> Vec<MenuCandidate> {
    if !obs.specs_enabled || !obs.paths_eligible {
        return Vec::new();
    }
    let Some((command, token)) = specs_context(typed) else {
        return Vec::new();
    };
    if token.is_empty() {
        return Vec::new();
    }
    let Some(spec) = SPECS.iter().find(|spec| spec.command == command) else {
        return Vec::new();
    };
    let entries = if token.starts_with("--") {
        spec.flags
    } else {
        spec.subcommands
    };
    entries
        .iter()
        .filter(|entry| entry.starts_with(token) && entry.len() > token.len())
        .filter_map(|entry| {
            let suffix = escape_for_shell(&entry[token.len()..])?;
            Some(MenuCandidate {
                label: (*entry).to_string(),
                suffix,
                description: None,
                source: SuggestionSource::Specs,
            })
        })
        .collect()
}

/// The command-name provider's whole set (suggest-menu spec): every
/// catalog name with the typed command word as a strict prefix — no
/// shortest-wins and no one-character floor, both tiebreak-shaped
/// rulings for the single-choice ghost that a list dissolves (a
/// listing of every `g`-word is information, not a coin flip). The
/// seat rules hold: command position, not path-shaped, non-empty.
/// Each remainder rides the escape rule; an unwritable name drops
/// alone.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "reached only through candidates; live at Task 3")
)]
fn catalog_candidates(typed: &str, obs: &Observation<'_>) -> Vec<MenuCandidate> {
    if !obs.commands_enabled || !obs.paths_eligible {
        return Vec::new();
    }
    if !is_command_position(typed) {
        return Vec::new();
    }
    let word = typed.trim_start();
    if word.is_empty() || is_path_shaped(word) {
        return Vec::new();
    }
    obs.command_catalog
        .iter()
        .filter(|name| name.starts_with(word) && name.len() > word.len())
        .filter_map(|name| {
            let suffix = escape_for_shell(&name[word.len()..])?;
            Some(MenuCandidate {
                label: name.clone(),
                suffix,
                description: None,
                source: SuggestionSource::Catalog,
            })
        })
        .collect()
}

/// The menu's visible-row budget (suggest-menu spec, OQ-2, locked):
/// eight rows with a scroll offset — Warp/VS Code show ~10; eight
/// fits the drop-down's short surface.
const MENU_VISIBLE_ROWS: usize = 8;

/// One menu-scoped press (suggest-menu spec): the widget's scoped
/// routing maps the keys the menu claims while open onto these —
/// Up/Down navigate, Tab/Enter accept — and every press it does not
/// claim arrives as [`MenuKey::Other`], whose transition closes the
/// menu before the press flows on through the normal handler.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the widget's routing lands in Task 3")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuKey {
    Up,
    Down,
    Accept,
    Other,
}

/// The completion menu's state (suggest-menu spec): the open fact,
/// the displayed rows, the selected index, and the scroll offset —
/// pure transitions over [`MenuCandidate`] lists, never the grid,
/// the PTY, or the clock. Invariant: open implies non-empty rows
/// (an empty collection declines to open; a filter that empties the
/// set closes), so a closed menu is always a spent one.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the widget's routing lands in Task 3")
)]
#[derive(Debug, Default)]
pub(crate) struct MenuState {
    open: bool,
    rows: Vec<MenuCandidate>,
    selected: usize,
    scroll: usize,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the accessors beyond is_open feed Task 4's renderer"
    )
)]
impl MenuState {
    /// A closed, spent menu — the state a fresh terminal carries.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the menu shows.
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The displayed rows, in display order.
    pub(crate) fn rows(&self) -> &[MenuCandidate] {
        &self.rows
    }

    /// The selected row's index into [`Self::rows`].
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    /// The first visible row's index — the scroll offset the
    /// renderer's window starts at.
    pub(crate) fn scroll(&self) -> usize {
        self.scroll
    }

    /// Opens over the merged collection (the record's ruled
    /// contents): the collection's own source order — history on
    /// top — is the display order, the selection at the top, no
    /// re-ranking here. An empty collection declines to open and
    /// answers `false`: no candidates, no menu. Returns whether the
    /// menu is open.
    pub(crate) fn open(&mut self, rows: Vec<MenuCandidate>) -> bool {
        if rows.is_empty() {
            self.close();
            return false;
        }
        self.rows = rows;
        self.selected = 0;
        self.scroll = 0;
        self.open = true;
        true
    }

    /// Closes and spends the menu — the transition for Escape,
    /// Ctrl+Space's toggle, and any press the menu does not claim.
    pub(crate) fn close(&mut self) {
        self.open = false;
        self.rows.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    /// Re-ranks the displayed rows under a filter query — Warp's
    /// pinned policy via [`filter_rank`]: prefix hits above
    /// subsequence hits, contiguity-aware, ties in provenance
    /// order. The selection and scroll reset to the top; a query
    /// nothing matches closes the menu (an open menu with no rows
    /// is a state the machine refuses).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the auto-popup feeding lands in Task 6")
    )]
    pub(crate) fn filter(&mut self, query: &str) {
        self.rows = filter_rank(query, std::mem::take(&mut self.rows));
        if self.rows.is_empty() {
            self.close();
            return;
        }
        self.selected = 0;
        self.scroll = 0;
    }

    /// One menu-scoped press's transition: navigate, accept (the
    /// exact suffix string an accept writes), or close on any other
    /// key. A closed menu answers `None` to everything and stays
    /// closed.
    pub(crate) fn press(&mut self, key: MenuKey) -> Option<String> {
        if !self.open {
            return None;
        }
        match key {
            MenuKey::Up => {
                self.move_up();
                None
            }
            MenuKey::Down => {
                self.move_down();
                None
            }
            MenuKey::Accept => self.accept(),
            MenuKey::Other => {
                self.close();
                None
            }
        }
    }

    /// Takes the selected row's suffix and closes: the exact string
    /// `Terminal::input_no_scroll` writes — the label, the
    /// description, the provenance never reach the PTY.
    fn accept(&mut self) -> Option<String> {
        let suffix = self.rows.get(self.selected).map(|row| row.suffix.clone());
        self.close();
        suffix
    }

    /// Moves the selection up one row, clamping at the top, the
    /// scroll window following so the selection stays visible.
    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.follow_scroll();
    }

    /// Moves the selection down one row, clamping at the last, the
    /// scroll window following so the selection stays visible.
    fn move_down(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
        self.follow_scroll();
    }

    /// Keeps the selected row inside the visible window after a
    /// move: scrolled past the top edge pulls the window up, past
    /// the bottom edge pushes it down.
    fn follow_scroll(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + MENU_VISIBLE_ROWS {
            self.scroll = self.selected + 1 - MENU_VISIBLE_ROWS;
        }
    }
}

/// One label's verdict under the menu's fuzzy filter: the tier, and
/// within the subsequence tier the contiguity facts
/// [`filter_rank`]'s sort key orders by — longer contiguous runs
/// first (via [`std::cmp::Reverse`]), then earlier first matches.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum MatchTier {
    /// The label starts with the query, case-insensitively — the
    /// vacuous hit when the query is empty, which keeps every row.
    Prefix,
    /// The query's characters appear in the label in order:
    /// `run` the longest contiguous run of query characters the
    /// greedy walk matched adjacently, `first` the first match's
    /// position.
    Subsequence { run: usize, first: usize },
}

/// The menu's fuzzy filter and ranker (suggest-menu spec; Warp's
/// pinned policy, `matchers.rs`): case-insensitive prefix hits
/// first, then subsequence hits — engaged only when the query
/// carries at least one alphanumeric character (Warp's own gate:
/// fuzzy matches on punctuation produce poor results) —
/// contiguity-aware (a longer contiguous matched run outranks a
/// scattered one), earlier matches outranking later. Hand-rolled,
/// zero dependencies; a stable sort, so every tie falls back to the
/// collection's provenance order. Rows that match nothing drop.
fn filter_rank(query: &str, rows: Vec<MenuCandidate>) -> Vec<MenuCandidate> {
    let sort_key = |tier: &MatchTier| match tier {
        MatchTier::Prefix => (0, std::cmp::Reverse(0), 0),
        MatchTier::Subsequence { run, first } => (1, std::cmp::Reverse(*run), *first),
    };
    let mut ranked: Vec<_> = rows
        .into_iter()
        .filter_map(|row| match_tier(query, &row.label).map(|tier| (row, tier)))
        .collect();
    ranked.sort_by_key(|(_, tier)| sort_key(tier));
    ranked.into_iter().map(|(row, _)| row).collect()
}

/// One query's verdict against one label, case-insensitively — the
/// filter's match half; `None` drops the row. The subsequence walk
/// is greedy left-to-right (the fzf-family approximation):
/// contiguity is measured on the greedy match, which is
/// deterministic and exact for the short menu labels it serves.
fn match_tier(query: &str, label: &str) -> Option<MatchTier> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let label: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() || label.starts_with(&query[..]) {
        return Some(MatchTier::Prefix);
    }
    // Warp's own gate: the subsequence fallback exists only for a
    // query with at least one alphanumeric.
    if !query.iter().copied().any(char::is_alphanumeric) {
        return None;
    }
    let mut query_idx = 0;
    let mut first = None;
    let mut run = 0;
    let mut best_run = 0;
    for (idx, c) in label.iter().enumerate() {
        if query_idx < query.len() && *c == query[query_idx] {
            first.get_or_insert(idx);
            run += 1;
            best_run = best_run.max(run);
            query_idx += 1;
        } else {
            run = 0;
        }
    }
    (query_idx == query.len()).then(|| MatchTier::Subsequence {
        run: best_run,
        first: first.unwrap_or(0),
    })
}

/// Commits one command to the ring: empty and whitespace-only lines
/// never record, a re-typed command promotes to most-recent (fish's
/// effective order), and the oldest entry evicts past the cap.
fn commit(ring: &mut VecDeque<String>, command: &str) {
    if command.trim().is_empty() {
        return;
    }
    ring.retain(|entry| entry != command);
    ring.push_back(command.to_string());
    while ring.len() > RING_CAP {
        ring.pop_front();
    }
}

/// Whether the typed segment is still in command position — the
/// first shell word is being entered: after leading whitespace, no
/// unescaped whitespace remains. The two-character escape `\ ` does
/// not end a word, so `git\ myp` is still one open first word. The
/// command-catalog provider keys on this; history does not care.
fn is_command_position(typed: &str) -> bool {
    let mut escaped = false;
    !typed.trim_start().chars().any(|c| {
        let ends_the_word = c.is_whitespace() && !escaped;
        escaped = c == '\\';
        ends_the_word
    })
}

/// The trailing token — the typed segment after its last unescaped
/// whitespace, the word being entered: `git che` carves `che`;
/// `git\ myp` carves itself (the escaped space keeps the word open);
/// leading whitespace never survives the carve. The path provider
/// completes this token.
fn trailing_token(typed: &str) -> &str {
    let mut token_start = 0;
    let mut escaped = false;
    for (idx, c) in typed.char_indices() {
        if c.is_whitespace() && !escaped {
            token_start = idx + c.len_utf8();
        }
        escaped = c == '\\';
    }
    &typed[token_start..]
}

/// Whether a token is path-shaped — the shape the path provider
/// completes: `~` alone (the HOME root), or any token that roots at
/// `~` / `.` / `..` / `/` or otherwise carries a `/` — every rooting
/// shows its slash. A lone `.`/`..`, a `~user` reference, and a
/// dotfile name are plain words until a slash appears; flags
/// (`-la`, `--flag`) never are.
fn is_path_shaped(token: &str) -> bool {
    token == "~" || token.contains('/')
}

/// The path provider's input token (path-suggest spec): the typed
/// segment's trailing token when it is path-shaped — the one word
/// whose parent directory the engine lists. `None` is no
/// path-completion context: a plain word, a flag, or nothing typed.
pub(crate) fn path_token(typed: Option<&str>) -> Option<&str> {
    typed
        .map(trailing_token)
        .filter(|token| is_path_shaped(token))
}

/// The shells whose tabs may suggest local paths (path-suggest
/// spec): the `/etc/shells` regulars, ruled at clarification. The
/// provider's honesty gate — static local path knowledge describes
/// the *local* machine, so it may only run on a tab whose spawn truth
/// is one of these; membership is a one-line flip at approval.
const SHELL_ALLOWLIST: [&str; 8] = ["bash", "zsh", "fish", "sh", "dash", "ksh", "csh", "tcsh"];

/// Whether a spawned program's basename is an allowlisted shell
/// (path-suggest spec) — the spawn truth that decides the
/// observation's [`Observation::paths_eligible`]. The program word
/// arrives spawn-resolved (`/bin/bash` for a configured shell, `ssh`
/// for a spawned-as-ssh profile); the basename is what the allowlist
/// compares, and a non-UTF-8 or empty name is never a shell.
pub(crate) fn is_allowlisted_shell(program: &str) -> bool {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| SHELL_ALLOWLIST.contains(&name))
}

/// One seeded history line's length bound (history-seed spec): a
/// pasted blob's enormous line is not a completion's working set, so
/// it never takes a ring seat.
const HISTORY_ENTRY_MAX: usize = 4096;

/// The shell's own persistent history file (history-seed spec): the
/// per-shell default location under the spawn-time HOME, relative so
/// the engine joins it. Only the three shells with a standard
/// persistent file seed — `sh`, `dash`, `ksh`, `csh`, and `tcsh` have
/// no default everyone agrees on, so they stay scrape-only, and a
/// non-allowlisted program is silence as everywhere. The user's own
/// `HISTFILE` override is not visible at spawn (only HOME and PATH
/// are), so the default location is the honest truth read.
pub(crate) fn shell_history_file(program: &str) -> Option<PathBuf> {
    match Path::new(program).file_name()?.to_str()? {
        "bash" => Some(PathBuf::from(".bash_history")),
        "zsh" => Some(PathBuf::from(".zsh_history")),
        "fish" => Some(PathBuf::from(".local/share/fish/fish_history")),
        _ => None,
    }
}

/// Parses a shell history file's bytes into seed entries
/// (history-seed spec): the per-shell format's conservative read —
/// bash's plain lines (its `#`-led timestamp/comment lines skipped),
/// zsh's plain lines with the extended-history `: <start>:<elapsed>;`
/// prefix stripped when a line carries the exact shape, and fish's
/// `- cmd: ` records with multiline commands' heads dropped whole
/// (their indented tails would seed half a command). Blank and
/// oversized lines never seed. The result is oldest-first, deduped
/// keeping each command's most recent occurrence, capped to the last
/// [`RING_CAP`] entries — exactly the shape a well-lived-in ring
/// would have arrived at on its own.
pub(crate) fn parse_shell_history(program: &str, contents: &str) -> Vec<String> {
    let shell = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut parsed: Vec<String> = match shell {
        "bash" => contents
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(ToString::to_string)
            .collect(),
        "zsh" => contents
            .lines()
            .map(|line| zsh_command(line).to_string())
            .collect(),
        "fish" => parse_fish_history(contents),
        _ => return Vec::new(),
    };
    parsed.retain(|entry| !entry.trim().is_empty() && entry.chars().count() <= HISTORY_ENTRY_MAX);
    // Newest-last dedupe: walk backwards keeping first sightings, then
    // restore file order — the same seat a live re-type promotes to.
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for entry in parsed.iter().rev() {
        if seen.insert(entry.as_str()) {
            unique.push(entry.clone());
            if unique.len() == RING_CAP {
                break;
            }
        }
    }
    unique.reverse();
    unique
}

/// One zsh history line's command: the extended-history metadata
/// prefix (`: <start>:<elapsed>;`) stripped when the line carries the
/// exact digits-colon-digits shape, the line itself otherwise — a
/// plain-format file and a pathological `: `-led command both pass
/// through whole.
fn zsh_command(line: &str) -> &str {
    let Some(rest) = line.strip_prefix(": ") else {
        return line;
    };
    let Some((meta, command)) = rest.split_once(';') else {
        return line;
    };
    let shaped = meta.split_once(':').is_some_and(|(start, elapsed)| {
        !start.is_empty()
            && !elapsed.is_empty()
            && start.chars().all(|c| c.is_ascii_digit())
            && elapsed.chars().all(|c| c.is_ascii_digit())
    });
    if shaped { command } else { line }
}

/// fish's YAML-ish history records: one command per `- cmd: ` line,
/// with a multiline command's head dropped when its indented
/// continuation follows (seeding half a command is seeding a wrong
/// one). The `paths:` blocks and other records never match the
/// prefix and contribute nothing.
fn parse_fish_history(contents: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut pending: Option<String> = None;
    for line in contents.lines() {
        if let Some(command) = line.strip_prefix("- cmd: ") {
            if let Some(head) = pending.take() {
                entries.push(head);
            }
            pending = Some(command.to_string());
        } else if line.starts_with(char::is_whitespace) {
            // An indented tail: the pending head is half a command.
            pending = None;
        } else if let Some(head) = pending.take() {
            entries.push(head);
        }
    }
    if let Some(head) = pending {
        entries.push(head);
    }
    entries
}

/// The accept splitter (autosuggest spec): fish's forward-word shape
/// — one word carries the suffix's leading whitespace plus the next
/// non-whitespace run, and the remainder stays suggestable for the
/// next accept.
pub(crate) fn split_word(suffix: &str) -> (&str, &str) {
    let word_start = suffix.len() - suffix.trim_start().len();
    let word_end = word_start
        + suffix[word_start..]
            .find(char::is_whitespace)
            .unwrap_or(suffix.len() - word_start);
    suffix.split_at(word_end)
}

/// A scraped string's conservative grid-cell count: ASCII prints one
/// cell, anything else counts two (the wide-CJK norm). An
/// overestimate by design — combining marks and narrow non-ASCII
/// inflate it — so a suffix whose true width is uncertain degrades to
/// no suggestion rather than a ghost that crosses the line end
/// (autosuggest spec).
fn cell_width(text: &str) -> usize {
    text.chars().map(|c| usize::from(!c.is_ascii()) + 1).sum()
}

/// The clip-to-fit display (ghost-fit spec, fish 3.7.1 parity): what
/// the widget draws of a too-wide suffix — the longest prefix fitting
/// `remaining − 2` cells under `cell_width`'s estimate (so a
/// two-cell character at the cut drops whole, never half-drawn),
/// followed by the two-cell `…`. With fewer than three free columns
/// nothing draws at all — `…` needs its two reserved cells plus one
/// cell of suffix, and a cut with no marker is never shipped, because
/// the whole accept path depends on the user knowing there is more
/// than they can see. A suffix that fits draws whole: `visible ==
/// suffix`, marker-free.
fn clip_visible(suffix: &str, remaining: usize) -> String {
    if cell_width(suffix) <= remaining {
        return suffix.to_string();
    }
    if remaining < 3 {
        return String::new();
    }
    let budget = remaining - 2;
    let mut end = 0;
    let mut used = 0;
    for (idx, c) in suffix.char_indices() {
        let width = usize::from(!c.is_ascii()) + 1;
        if used + width > budget {
            break;
        }
        used += width;
        end = idx + c.len_utf8();
    }
    let mut visible = suffix[..end].to_string();
    visible.push('…');
    visible
}

/// The ghost tint (autosuggest spec, OQ-2): the scheme's foreground
/// blended ~50% into its background — the plain channel average — so
/// the ghost reads as dimmed foreground on any scheme. Never palette
/// index 8 (the zsh-autosuggestions invisible-text lesson: bright
/// black equals background on whole families of schemes). Computed
/// once per theme apply and stored by the engine, never per frame.
pub(crate) fn ghost_color(colors: &Colors) -> cosmic_text::Color {
    let fg = colors[NamedColor::Foreground].unwrap_or_default();
    let bg = colors[NamedColor::Background].unwrap_or_else(|| {
        // An unset background reads as the theme-applied window
        // background — the `convert_color` neighborhood's own
        // fallback.
        let [r, g, b, _] = cosmic_text::Color(WINDOW_BG_COLOR.load(Ordering::SeqCst)).as_rgba();
        Rgb { r, g, b }
    });
    cosmic_text::Color::rgb(
        u8::midpoint(fg.r, bg.r),
        u8::midpoint(fg.g, bg.g),
        u8::midpoint(fg.b, bg.b),
    )
}

#[cfg(test)]
mod tests;
