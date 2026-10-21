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
    let (suffix, source) = merged_candidate(typed, suggest, obs)?;
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
        SuggestionSource::Path => 1,
        // The specs variant (flag-specs) slots between path and
        // catalog when it lands, renumbering there — never
        // reordering.
        SuggestionSource::Catalog => 2,
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
/// words never consult it.
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
    // The snapshot is sorted, so `min_by_key` over the length keeps
    // the alphabetically-first of equal-length ties.
    let name = obs
        .command_catalog
        .iter()
        .filter(|name| name.starts_with(word) && name.len() > word.len())
        .min_by_key(|name| name.len())?;
    escape_for_shell(&name[word.len()..])
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
        SuggestionSource::Catalog,
    ];
    providers.sort_by_key(|&source| source_rank(source));
    providers.into_iter().find_map(|source| {
        provider_candidate(source, typed, suggest, obs).map(|suffix| (suffix, source))
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
