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
use std::collections::VecDeque;
use std::path::Path;
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
    /// Bytes after the cursor, sent verbatim on accept.
    pub(crate) suffix: String,
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
    /// The path provider's spawn gate (path-suggest spec): the
    /// spawned program is an allowlisted shell. Static local path
    /// knowledge describes the *local* machine — on a tab spawned as
    /// ssh, `/proc/{shell_pid}/cwd` is the wrong machine's truth, so
    /// paths stay silent there and only grid-scraped history keeps
    /// suggesting (remote-truthful). A typed ssh session is already
    /// silent end-to-end via the foreground guard. The gate, not the
    /// provider: the listing and candidates land behind it in later
    /// tasks.
    // The gate ships before its consumer: the path provider's guard
    // test reads this when PS-3 lands.
    #[allow(dead_code)]
    pub(crate) paths_eligible: bool,
    /// The path provider's reduced listing (path-suggest spec): the
    /// sorted (name, is-a-directory) entries of the trailing token's
    /// canonical parent directory, read engine-side and keyed by that
    /// parent — present only for a path-shaped token on an eligible
    /// tab; `None` is no path-completion context (silence). Borrowed
    /// from the engine's cache; the model never touches the
    /// filesystem.
    // The listing ships before its consumer: the path provider's
    // candidate logic reads this when PS-3 lands.
    #[allow(dead_code)]
    pub(crate) path_listing: Option<&'a [(String, bool)]>,
    /// The terminal's focus state.
    pub(crate) focused: bool,
    /// A foreground child holds the PTY (`tcgetpgrp` vs the shell's
    /// process group): true reads "not at a prompt", for any shell,
    /// transparently over SSH.
    pub(crate) foreground_child: bool,
    /// The grid's display offset: non-zero means the user is scrolled
    /// back into history.
    pub(crate) display_offset: usize,
    /// The cursor sits on the viewport's bottom row.
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
}

/// The engine-owned autosuggest state (autosuggest spec): the adopted
/// prompt anchor, the bounded history ring, the continuous idle-line
/// capture, the foreground-diff flag, and the current showing. One
/// per terminal — it dies with the terminal, so a respawn (a fresh
/// terminal) resets history by construction.
pub(crate) struct Suggest {
    anchor: Option<String>,
    adopt_armed: bool,
    capture: String,
    last_foreground_child: bool,
    ring: VecDeque<String>,
    showing: Option<Showing>,
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
        }
    }

    /// The current showing — the suffix an accept key writes, and the
    /// only thing the widget renders.
    pub(crate) fn showing(&self) -> Option<&Showing> {
        self.showing.as_ref()
    }

    /// Whether an anchor is adopted — the engine's settle-prompt
    /// latch clears once a prompt exists.
    pub(crate) fn anchored(&self) -> bool {
        self.anchor.is_some()
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

    suggest.showing = showing(obs, suggest);
    suggest.showing.clone()
}

/// The guard chain (autosuggest spec): every condition must hold, and
/// each one's failure alone means silence — config off, unfocused, a
/// foreground child, scrolled back, off the bottom row, unanchored, a
/// line that left the anchor, an empty typed segment, no candidate
/// from any provider, or a suffix that would cross the line end
/// (clip-don't-wrap drops it). The candidate itself comes from the
/// merged seam's dispatch; the guards and the fit rule apply after
/// the merge, unchanged.
fn showing(obs: &Observation<'_>, suggest: &Suggest) -> Option<Showing> {
    if !obs.enabled
        || !obs.focused
        || obs.foreground_child
        || obs.display_offset != 0
        || !obs.cursor_on_bottom_row
    {
        return None;
    }
    let anchor = suggest.anchor.as_ref()?;
    let typed = obs.before_cursor.strip_prefix(anchor)?;
    if typed.is_empty() {
        return None;
    }
    let (suffix, source) = merged_candidate(typed, suggest)?;
    // The fit is grid cells, not chars: the ghost starts one cell
    // past the cursor and must end inside the line.
    let remaining = obs.columns.saturating_sub(obs.cursor_column + 1);
    if cell_width(&suffix) > remaining {
        return None;
    }
    Some(Showing { suffix, source })
}

/// The merge policy, ruled at clarification: history answers first —
/// the tab's own truth — then path, then the curated specs, then the
/// command catalog. One pure function of the provider variant,
/// pinned by its test, so the dispatch derives its order here and
/// later modules cannot reorder the merge by accident.
fn source_rank(source: SuggestionSource) -> usize {
    match source {
        SuggestionSource::History => 0,
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

/// One provider's candidate function, keyed by its variant — the
/// dispatch table a later module extends with its arm (and its
/// variant in the list `merged_candidate` walks) without touching
/// any call site.
fn provider_candidate(source: SuggestionSource, typed: &str, suggest: &Suggest) -> Option<String> {
    match source {
        SuggestionSource::History => history_candidate(typed, &suggest.ring),
    }
}

/// The merged seam's dispatch (suggest-merge): every provider, in
/// `source_rank` order — the sort is the priority, never a hand
/// order — and the first non-`None` candidate wins, its provenance
/// carried in the pair. Exactly one suggestion or none (autosuggest
/// spec).
fn merged_candidate(typed: &str, suggest: &Suggest) -> Option<(String, SuggestionSource)> {
    let mut providers = [SuggestionSource::History];
    providers.sort_by_key(|&source| source_rank(source));
    providers.into_iter().find_map(|source| {
        provider_candidate(source, typed, suggest).map(|suffix| (suffix, source))
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
// The seam's vocabulary ships before its consumers: command-catalog
// cites this when its module lands.
#[allow(dead_code)]
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
mod tests {
    use super::*;

    /// The clear case's defaults: an anchored idle prompt, "gi"
    /// typed, and a two-entry ring. Every guard test mutates exactly
    /// one field away from this.
    fn obs() -> Observation<'static> {
        Observation {
            enabled: true,
            paths_eligible: true,
            path_listing: None,
            focused: true,
            foreground_child: false,
            display_offset: 0,
            cursor_on_bottom_row: true,
            columns: 80,
            cursor_column: 4,
            before_cursor: "$ gi",
            after_cursor_blank: true,
            fresh_prompt: false,
        }
    }

    /// A suggest adopted at the "$ " settle prompt, seeded with
    /// history: "git push" older, "git status" most recent.
    fn adopted() -> Suggest {
        let mut suggest = Suggest::new();
        let settle = Observation {
            before_cursor: "$ ",
            cursor_column: 2,
            fresh_prompt: true,
            ..obs()
        };
        step(&settle, &mut suggest);
        let mut ring = VecDeque::new();
        ring.push_back("git push".to_string());
        ring.push_back("git status".to_string());
        suggest.ring = ring;
        suggest
    }

    // --- the guard table: each guard alone yields None ---

    #[test]
    fn guard_config_off_yields_none() {
        let mut suggest = adopted();
        let o = Observation {
            enabled: false,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_unfocused_yields_none() {
        let mut suggest = adopted();
        let o = Observation {
            focused: false,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_foreground_child_yields_none() {
        let mut suggest = adopted();
        let o = Observation {
            foreground_child: true,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_scrolled_back_yields_none() {
        let mut suggest = adopted();
        let o = Observation {
            display_offset: 3,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_cursor_off_bottom_row_yields_none() {
        let mut suggest = adopted();
        let o = Observation {
            cursor_on_bottom_row: false,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_unanchored_yields_none() {
        // A suggest that never adopted: no anchor, so no typed
        // segment can ever be carved out of the line.
        let mut suggest = Suggest::new();
        assert_eq!(step(&obs(), &mut suggest), None);
    }

    #[test]
    fn guard_line_left_the_anchor_yields_none() {
        // The honest degradation for multi-line and right-side
        // prompts (and `read`-style prompts): the line no longer
        // starts with the adopted anchor, so nothing is certain.
        let mut suggest = adopted();
        let o = Observation {
            before_cursor: "root@host:~# gi",
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_empty_typed_segment_yields_none() {
        let mut suggest = adopted();
        let o = Observation {
            before_cursor: "$ ",
            cursor_column: 2,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn guard_no_history_match_yields_none() {
        let mut suggest = adopted();
        let mut ring = VecDeque::new();
        ring.push_back("vim notes".to_string());
        suggest.ring = ring;
        assert_eq!(step(&obs(), &mut suggest), None);
    }

    #[test]
    fn guard_empty_suffix_yields_none() {
        // Typed equals the full most-recent entry: there is nothing
        // left to suggest.
        let mut suggest = adopted();
        let mut ring = VecDeque::new();
        ring.push_back("gi".to_string());
        suggest.ring = ring;
        assert_eq!(step(&obs(), &mut suggest), None);
    }

    #[test]
    fn guard_suffix_crossing_the_line_end_yields_none() {
        // Three columns remain after the cursor; "t status" needs
        // eight. Clip, don't wrap — wrong-cell ghost text is worse
        // than none.
        let mut suggest = adopted();
        let o = Observation {
            columns: 8,
            ..obs()
        };
        assert_eq!(step(&o, &mut suggest), None);
    }

    #[test]
    fn clear_case_yields_the_most_recent_match_suffix() {
        let mut suggest = adopted();
        let showing = step(&obs(), &mut suggest).expect("the clear case shows");
        // "git status" is the most recent match, not "git push".
        assert_eq!(showing.suffix, "t status");
        assert_eq!(showing.source, SuggestionSource::History);
    }

    #[test]
    fn suffix_fit_measures_grid_cells_not_chars() {
        // "中文" is two chars but four cells: it fits four remaining
        // columns and drops at three — by chars both would fit.
        let mut suggest = adopted();
        let mut ring = VecDeque::new();
        ring.push_back("echo中文".to_string());
        suggest.ring = ring;
        let typing = Observation {
            before_cursor: "$ echo",
            cursor_column: 6,
            ..obs()
        };
        let fits = Observation {
            columns: 11,
            ..typing
        };
        let drops = Observation {
            columns: 10,
            ..typing
        };
        let shown = step(&fits, &mut suggest).expect("four cells fit four columns");
        assert_eq!(shown.suffix, "中文");
        assert_eq!(step(&drops, &mut suggest), None);
    }

    #[test]
    fn take_suffix_yields_once_and_clears_the_showing_state() {
        // The accept-once contract: an accept key writes the bytes
        // exactly once; what shows next is re-derived by the next
        // step, never re-read from a stale suffix.
        let mut suggest = adopted();
        let shown = step(&obs(), &mut suggest).expect("the clear case shows");
        assert_eq!(
            suggest.showing().map(|s| s.suffix.as_str()),
            Some("t status")
        );
        assert_eq!(suggest.take_suffix().as_deref(), Some("t status"));
        assert_eq!(suggest.take_suffix(), None, "the suffix is spent");
        assert_eq!(suggest.showing(), None);
        assert_eq!(shown.suffix, "t status");
    }

    // --- the ring ---

    #[test]
    fn ring_evicts_the_oldest_past_the_cap() {
        let mut ring = VecDeque::new();
        for i in 0..=RING_CAP {
            commit(&mut ring, &format!("cmd-{i}"));
        }
        assert_eq!(ring.len(), RING_CAP);
        assert_eq!(
            ring.front().map(String::as_str),
            Some("cmd-1"),
            "the oldest entry (cmd-0) evicted"
        );
    }

    #[test]
    fn ring_promotes_a_retyped_command_to_most_recent() {
        let mut ring = VecDeque::new();
        commit(&mut ring, "ls");
        commit(&mut ring, "cd");
        commit(&mut ring, "ls");
        assert_eq!(ring.len(), 2, "the retype promoted, never duplicated");
        assert_eq!(ring.back().map(String::as_str), Some("ls"));
    }

    #[test]
    fn ring_never_records_blank_lines() {
        let mut ring = VecDeque::new();
        commit(&mut ring, "");
        commit(&mut ring, " \t ");
        assert!(ring.is_empty());
    }

    #[test]
    fn commit_fires_on_the_idle_to_running_transition_with_the_freshest_capture() {
        let mut suggest = adopted();
        // Typing refreshes the capture but records nothing.
        step(
            &Observation {
                before_cursor: "$ git stash",
                cursor_column: 11,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(
            suggest.ring.back().map(String::as_str),
            Some("git status"),
            "idle typing never commits"
        );
        // Enter: the child takes the foreground, and the freshest
        // capture lands in the ring as the most recent entry.
        step(
            &Observation {
                foreground_child: true,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(suggest.ring.back().map(String::as_str), Some("git stash"));
    }

    // --- the anchor ---

    #[test]
    fn anchor_adopts_at_the_settle_fresh_prompt() {
        let mut suggest = Suggest::new();
        step(
            &Observation {
                before_cursor: "$ ",
                cursor_column: 2,
                fresh_prompt: true,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(suggest.anchor.as_deref(), Some("$ "));
    }

    #[test]
    fn anchor_re_adopts_after_each_finish_replacing_the_old_one() {
        let mut suggest = Suggest::new();
        // The settle adopts "$ ".
        step(
            &Observation {
                before_cursor: "$ ",
                cursor_column: 2,
                fresh_prompt: true,
                ..obs()
            },
            &mut suggest,
        );
        // A command runs, then finishes under a different prompt:
        // the running→idle diff arms, the fresh-prompt observation
        // adopts, and the old anchor is replaced.
        step(
            &Observation {
                before_cursor: "$ git status",
                cursor_column: 11,
                ..obs()
            },
            &mut suggest,
        );
        step(
            &Observation {
                foreground_child: true,
                ..obs()
            },
            &mut suggest,
        );
        step(
            &Observation {
                before_cursor: "% ",
                cursor_column: 2,
                fresh_prompt: true,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(suggest.anchor.as_deref(), Some("% "));
        // And the new anchor keys suggestions immediately: ring
        // entries are commands, anchor-less.
        let mut ring = VecDeque::new();
        ring.push_back("echo hi".to_string());
        suggest.ring = ring;
        let shown = step(
            &Observation {
                before_cursor: "% ec",
                cursor_column: 4,
                ..obs()
            },
            &mut suggest,
        )
        .expect("the replaced anchor keys the new prompt");
        assert_eq!(shown.suffix, "ho hi");
    }

    #[test]
    fn anchor_adoption_requires_a_fresh_prompt_observation() {
        // Armed (fresh state) but no fresh-prompt moment: no
        // adoption, so typing stays silent.
        let mut suggest = Suggest::new();
        step(
            &Observation {
                before_cursor: "$ ",
                cursor_column: 2,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(suggest.anchor, None);
    }

    #[test]
    fn anchor_adoption_requires_the_cursor_at_line_end() {
        // The post-finish update caught a line with text after the
        // cursor: not certainly a bare prompt, so no adoption.
        let mut suggest = Suggest::new();
        step(
            &Observation {
                before_cursor: "$ vi src",
                cursor_column: 8,
                after_cursor_blank: false,
                fresh_prompt: true,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(suggest.anchor, None);
    }

    #[test]
    fn anchor_adoption_declines_a_non_whitespace_terminated_line() {
        // In-run ruling (build record): a type-ahead user's buffered
        // keystrokes can echo into the same update as the post-finish
        // prompt redraw, leaving a half-typed line that still has the
        // cursor at its end. A line that does not end in whitespace
        // is not certainly a bare prompt — adoption waits rather than
        // key suggestions on the half-typed tail (never a wrong
        // suffix).
        let mut suggest = Suggest::new();
        step(
            &Observation {
                before_cursor: "$ l",
                cursor_column: 3,
                fresh_prompt: true,
                ..obs()
            },
            &mut suggest,
        );
        assert_eq!(suggest.anchor, None);
    }

    // --- the splitter ---

    #[test]
    fn splitter_takes_leading_spaces_through_the_next_boundary() {
        assert_eq!(split_word(" -la --color"), (" -la", " --color"));
    }

    #[test]
    fn splitter_takes_a_bare_word_and_leaves_the_rest_suggestable() {
        assert_eq!(split_word("src/main.rs -la"), ("src/main.rs", " -la"));
    }

    #[test]
    fn splitter_consumes_a_lone_word_entirely() {
        assert_eq!(split_word(" -la"), (" -la", ""));
    }

    // --- the tint ---

    #[test]
    fn ghost_color_is_the_fg_bg_channel_average_never_the_background() {
        let mut colors = Colors::default();
        colors[NamedColor::Foreground as usize] = Some(Rgb {
            r: 200,
            g: 100,
            b: 50,
        });
        colors[NamedColor::Background as usize] = Some(Rgb { r: 0, g: 20, b: 40 });
        let ghost = ghost_color(&colors);
        assert_eq!(ghost.as_rgba(), [100, 60, 45, 255]);
        assert_ne!(ghost, cosmic_text::Color::rgb(0, 20, 40));
    }

    // --- the token shape (the merged seam's input vocabulary) ---

    #[test]
    fn command_position_holds_while_the_first_word_is_open() {
        assert!(is_command_position("gi"));
        // Leading whitespace never starts an argument.
        assert!(is_command_position("  car"));
        // The escaped space keeps the word open — still command
        // position.
        assert!(is_command_position("git\\ myp"));
    }

    #[test]
    fn command_position_ends_at_the_first_unescaped_whitespace() {
        assert!(!is_command_position("git che"));
        assert!(!is_command_position("echo he"));
    }

    #[test]
    fn the_trailing_token_carves_the_word_being_entered() {
        assert_eq!(trailing_token("git che"), "che");
        // The escaped space keeps the word open: the carve yields the
        // whole still-open word.
        assert_eq!(trailing_token("git\\ myp"), "git\\ myp");
        assert_eq!(trailing_token("  car"), "car");
        assert_eq!(trailing_token("git che -la"), "-la");
    }

    #[test]
    fn a_path_shaped_token_is_the_home_root_or_carries_a_slash() {
        for token in ["~", "~/sr", "../co", "./te", "/usr/bi", "sr/x"] {
            assert!(is_path_shaped(token), "{token:?} is path-shaped");
        }
    }

    #[test]
    fn a_plain_word_or_flag_is_not_path_shaped() {
        // A lone `.`/`..`, a `~user` reference, and a dotfile name
        // are plain words until a slash appears; flags never are.
        for token in ["src", "-la", "--flag", "che", ".", "..", ".hidden", "~jpt"] {
            assert!(!is_path_shaped(token), "{token:?} is not path-shaped");
        }
    }

    // --- the path provider's spawn gate (path-suggest) ---

    #[test]
    fn the_shell_allowlist_admits_the_etc_shells_regulars() {
        // The ruled default set, pinned as data: the eight
        // `/etc/shells` regulars. `nu` and `elvish` are deliberately
        // absent — an unknown shell degrades to silence, never a
        // guess.
        for shell in SHELL_ALLOWLIST {
            assert!(is_allowlisted_shell(shell), "{shell} is allowlisted");
        }
    }

    #[test]
    fn the_spawn_gate_reads_the_spawned_programs_basename() {
        // Spawn truth arrives as the resolved program word — a path
        // for configured and recorded shells, a bare name for a
        // profile command's leading program; the gate reads the
        // basename either way.
        assert!(is_allowlisted_shell("/bin/bash"));
        assert!(is_allowlisted_shell("/usr/bin/fish"));
    }

    #[test]
    fn the_spawn_gate_rejects_non_shells_unknown_shells_and_nothing_at_all() {
        // `ssh` and `python3` are the honesty line — those tabs never
        // suggest local paths. `nu`/`elvish` are unknown shells; a
        // custom-path shell outside the list is not a regular; the
        // empty string is the options default. All silent.
        for program in ["ssh", "python3", "nu", "elvish", "/opt/fancysh", ""] {
            assert!(
                !is_allowlisted_shell(program),
                "{program} is not allowlisted"
            );
        }
    }

    // --- the merged seam's dispatch ---

    #[test]
    fn source_rank_pins_the_ruled_priority() {
        // The ruled priority, stated as data: history answers first —
        // the tab's own truth — then path, then the curated specs,
        // then the command catalog. Each later module appends its
        // variant here; the ranks must keep strictly increasing in
        // exactly this order.
        let ruled_order = [SuggestionSource::History];
        assert_eq!(source_rank(SuggestionSource::History), 0);
        for pair in ruled_order.windows(2) {
            assert!(
                source_rank(pair[0]) < source_rank(pair[1]),
                "the order above is the merge's order"
            );
        }
    }

    #[test]
    fn the_dispatch_asks_history_and_carries_its_provenance() {
        let suggest = adopted();
        let (suffix, source) =
            merged_candidate("gi", &suggest).expect("history answers the clear case");
        assert_eq!(suffix, "t status");
        assert_eq!(source, SuggestionSource::History);
    }
}
