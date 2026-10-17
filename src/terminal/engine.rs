// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2023 System76 <info@system76.com>
//
// Adapted from cosmic-term
// (<https://github.com/pop-os/cosmic-term>) src/terminal.rs at commit
// 9129277.
//
// Changes from upstream: this is a drop-down terminal without
// panes/profiles/context-menu/password-manager, so
// `TerminalPaneGrid`, cosmic-term's pane/tab-keyed `EventProxy` (replaced
// by the `TabId`-tagging `TaggedEventProxy`), `MenuState`,
// `ProfileId` and `tab_title_override` have been removed, and the
// coupling to cosmic-term's own `Config`/`ColorSchemeKind`/profile-keyed
// theme map has been replaced with a small local `TerminalOptions`
// struct plus an already-resolved `Colors` value passed in by the
// caller. The PTY/VTE/grid-to-buffer logic itself is unmodified.

use alacritty_terminal::{
    Term,
    event::{Event, EventListener, Notify, OnResize, WindowSize},
    event_loop::{EventLoop, Msg, Notifier},
    grid::Dimensions,
    index::{Boundary, Column, Direction, Line, Point, Side},
    selection::{Selection, SelectionType},
    sync::FairMutex,
    term::{
        Config, TermDamage, TermMode,
        cell::Flags,
        color::{self, Colors},
        search::RegexSearch,
        viewport_to_point,
    },
    tty::{self, Options},
    vte::ansi::{Color, CursorShape, NamedColor, Rgb},
};
use cosmic::iced::{advanced::graphics::text::font_system, mouse::ScrollDelta};
use cosmic_text::{
    Attrs, AttrsList, Buffer, BufferLine, CacheKeyFlags, Family, LineEnding, Metrics, Shaping,
    Stretch, Weight, Wrap,
};
use indexmap::IndexSet;
use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs, io, mem,
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

pub use alacritty_terminal::grid::Scroll as TerminalScroll;

use crate::status_bar::CommandTimer;
use crate::tabs::TabId;
use crate::terminal::mouse_reporter::MouseReporter;
use crate::terminal::suggest::{self, AcceptKind, Suggest};

/// Runtime font and rendering options for a [`Terminal`].
///
/// This is dropterm's local replacement for cosmic-term's `Config`: it
/// carries only the fields the terminal engine itself needs, resolved by
/// the app from [`crate::config::DropTermConfig`] plus the active COSMIC
/// theme, rather than cosmic-term's full serialized app config with its
/// profiles and per-scheme-kind theme map.
#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "options are independent config facts, one field per resolved flag — bright-bold plus one per provider toggle; not a state machine to reshape"
)]
pub struct TerminalOptions {
    pub font_stretch: Stretch,
    pub font_weight: u16,
    pub dim_font_weight: u16,
    pub bold_font_weight: u16,
    pub use_bright_bold: bool,
    pub font_size: u16,
    pub font_size_zoom_step_mul_100: u16,
    /// Whether autosuggestions compute (autosuggest spec): resolved
    /// from the app config's `autosuggestions_enabled` where options
    /// are built — flips apply at the next `set_config` pass, so a
    /// showing suggestion disappears the moment the kill switch
    /// lands.
    pub autosuggestions_enabled: bool,
    /// The program the tab is spawned as (path-suggest spec): the
    /// spawn-resolved program word — the configured or recorded
    /// shell, a command line's leading program, or a profile
    /// command's leading program. Spawn truth: read once at
    /// construction and never rewritten by `set_config` (a respawn is
    /// a fresh terminal), so a mid-run config change cannot repaint a
    /// spawned-as-ssh tab into a shell tab.
    pub spawned_program: String,
    /// The spawn-time HOME (path-suggest spec): the environment the
    /// pty exec inherits — a profile env override when the preset
    /// carries one, else this process's HOME. `~`-rooted path tokens
    /// reduce against it; `None` leaves them unresolvable (silence).
    /// Spawn truth, never rewritten by `set_config`.
    pub spawn_home: Option<String>,
    /// The spawn-time PATH (command-catalog spec): the environment
    /// the pty exec inherits — a profile env override when the preset
    /// carries one, else this process's PATH. The command catalog's
    /// snapshot walks it once at construction. Spawn truth, never
    /// rewritten by `set_config` (a respawn is a fresh terminal, and
    /// its catalog with it).
    pub spawn_path: Option<String>,
    /// Whether the path provider joins the autosuggest merge
    /// (path-suggest spec): resolved from the app config's
    /// `autosuggestions_paths_enabled` — flips apply at the next
    /// `set_config` pass, so a showing path ghost disappears the
    /// moment the toggle lands while history keeps suggesting.
    pub autosuggestions_paths_enabled: bool,
    /// Whether the command-name provider joins the autosuggest merge
    /// (command-catalog spec): resolved from the app config's
    /// `autosuggestions_commands_enabled` — engine-`true` until the
    /// config key lands (CC-3); flips apply at the next
    /// `set_config` pass, so a showing catalog ghost disappears the
    /// moment the toggle lands while history keeps suggesting.
    pub autosuggestions_commands_enabled: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            font_stretch: Stretch::Normal,
            font_weight: Weight::NORMAL.0,
            dim_font_weight: Weight::NORMAL.0,
            bold_font_weight: Weight::BOLD.0,
            use_bright_bold: false,
            font_size: 14,
            font_size_zoom_step_mul_100: 100,
            autosuggestions_enabled: true,
            spawned_program: String::new(),
            spawn_home: None,
            spawn_path: None,
            autosuggestions_paths_enabled: true,
            autosuggestions_commands_enabled: true,
        }
    }
}

impl TerminalOptions {
    fn font_size_adjusted(&self, zoom_adj: i8) -> f32 {
        let font_size = f32::from(self.font_size).max(1.0);
        let adj = f32::from(zoom_adj);
        let adj_step = f32::from(self.font_size_zoom_step_mul_100) / 100.0;
        (font_size + adj * adj_step).max(1.0)
    }

    /// Calculate cosmic-text metrics from font size, adjusted for zoom.
    pub fn metrics(&self, zoom_adj: i8) -> Metrics {
        let font_size = self.font_size_adjusted(zoom_adj);
        let line_height = (font_size * 1.4).ceil();
        Metrics::new(font_size, line_height)
    }
}

/// Minimum contrast between a fixed cursor color and the cell's background.
/// Duplicated from alacritty
pub const MIN_CURSOR_CONTRAST: f64 = 1.5;

/// Maximum number of linewraps followed outside of the viewport during search highlighting.
/// Duplicated from alacritty.
/// A regex expression can start or end outside the visible screen. Therefore, without this constant, some regular expressions would not match at the top and bottom.
pub const MAX_SEARCH_LINES: usize = 100;

/// The URL-detection regex alacritty ships, verbatim:
/// <https://github.com/alacritty/alacritty/blob/4a7728bf7fac06a35f27f6c4f31e0d9214e5152b/alacritty/src/config/ui_config.rs#L36-L39>
fn url_regex_search() -> RegexSearch {
    let url_regex = "(ipfs:|ipns:|magnet:|mailto:|gemini://|gopher://|https://|http://|news:|file:|git://|ssh:|ftp://)\
                         [^\u{0000}-\u{001F}\u{007F}-\u{009F}<>\"\\s{-}\\^⟨⟩`]+";
    RegexSearch::new(url_regex).unwrap()
}

/// Cell-aware pixel size: the widget's size plus the derived cell
/// metrics, implementing `alacritty_terminal`'s `Dimensions`.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub width: u32,
    pub height: u32,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "surface px are < 2^24 (exact in f32); floor is the rows-per-screen contract"
    )]
    fn screen_lines(&self) -> usize {
        ((self.height as f32) / self.cell_height).floor() as usize
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "surface px are < 2^24 (exact in f32); floor is the cols-per-screen contract"
    )]
    fn columns(&self) -> usize {
        ((self.width as f32) / self.cell_width).floor() as usize
    }
}

impl From<Size> for WindowSize {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "WindowSize fields are u16; line/col/cell counts are bounded far below 65536"
    )]
    fn from(size: Size) -> Self {
        Self {
            num_lines: size.screen_lines() as u16,
            num_cols: size.columns() as u16,
            cell_width: size.cell_width as u16,
            cell_height: size.cell_height as u16,
        }
    }
}

/// Forwards alacritty terminal events into the app's global event channel,
/// stamping each one with the id of the tab that produced it.
#[derive(Clone)]
pub struct TaggedEventProxy {
    tab_id: TabId,
    tx: mpsc::UnboundedSender<(TabId, Event)>,
}

impl EventListener for TaggedEventProxy {
    fn send_event(&self, event: Event) {
        // TODO: handle error
        let _ = self.tx.send((self.tab_id, event));
    }
}

fn as_bright(mut color: Color) -> Color {
    if let Color::Named(named) = color {
        color = Color::Named(named.to_bright());
    }
    color
}

fn as_dim(mut color: Color) -> Color {
    if let Color::Named(named) = color {
        color = Color::Named(named.to_dim());
    }
    color
}

/// The terminal pane's effective background color, packed RGBA.
/// Written by the app when the theme applies; read as the fallback
/// for an unset scheme background and by the chrome strips (grab bar,
/// tab bar) to match the pane.
pub static WINDOW_BG_COLOR: AtomicU32 = AtomicU32::new(0xFF00_0000);

fn convert_color(colors: &Colors, color: Color) -> cosmic_text::Color {
    let rgb = match color {
        Color::Named(named_color) => {
            if let Some(rgb) = colors[named_color] {
                rgb
            } else if named_color == NamedColor::Background {
                // Allow using an unset background
                return cosmic_text::Color(WINDOW_BG_COLOR.load(Ordering::SeqCst));
            } else {
                log::warn!("missing named color {named_color:?}");
                Rgb::default()
            }
        }
        Color::Spec(rgb) => rgb,
        Color::Indexed(index) => {
            if let Some(rgb) = colors[index as usize] {
                rgb
            } else {
                log::warn!("missing indexed color {index}");
                Rgb::default()
            }
        }
    };
    cosmic_text::Color::rgb(rgb.r, rgb.g, rgb.b)
}

/// Whether a process group other than the shell's holds this
/// terminal's foreground — kitty's check (close-confirm criterion 2):
/// `tcgetpgrp` on a dup of the PTY master against the shell's own
/// process group. `None` inputs and any syscall failure read as
/// `false` — the check fails open, never trapping a tab behind its
/// own confirmation (criterion 7). Non-Linux reads `false` too: the
/// master handle is only captured there.
pub fn foreground_child_running(master: Option<&std::fs::File>, shell_pid: Option<u32>) -> bool {
    let (Some(master), Some(shell_pid)) = (master, shell_pid) else {
        return false;
    };

    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        // SAFETY: both calls read kernel pty/process state only;
        // neither touches caller memory.
        let fg = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
        let shell_pgrp = i32::try_from(shell_pid).map_or(-1, |pid| unsafe { libc::getpgid(pid) });
        fg > 0 && shell_pgrp > 0 && fg != shell_pgrp
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (master, shell_pid);
        false
    }
}

/// The foreground process group's name — `/proc/<fg-pgid>/comm`,
/// trimmed — for the status band's process segment (status-bar
/// spec). `None` whenever the shell itself owns the foreground
/// (idle), the inputs are missing, or the read fails (a group
/// leader that already exited, a non-Linux target): the band's
/// honest fallback is the recorded shell's basename, never a blank
/// or stale name.
pub fn foreground_process_name(
    master: Option<&std::fs::File>,
    shell_pid: Option<u32>,
) -> Option<String> {
    let (Some(master), Some(shell_pid)) = (master, shell_pid) else {
        return None;
    };

    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        // SAFETY: both calls read kernel pty/process state only;
        // neither touches caller memory.
        let fg = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
        let shell_pgrp = i32::try_from(shell_pid).map_or(-1, |pid| unsafe { libc::getpgid(pid) });
        (fg > 0 && shell_pgrp > 0 && fg != shell_pgrp)
            .then(|| read_comm(fg))
            .flatten()
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (master, shell_pid);
        None
    }
}

/// `/proc/<pid>/comm`, trimmed — the kernel's own name for the
/// process (15 chars at most by construction). An unreadable or
/// empty entry reads `None`: the band's fallback, never a guess.
#[cfg(target_os = "linux")]
fn read_comm(pid: i32) -> Option<String> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The ssh stand-in's destination token (status-remote spec):
/// `/proc/<fg-pid>/cmdline` — NUL-split argv through
/// [`crate::status_bar::ssh_destination`] — read only when the
/// foreground group's own comm is `ssh` (any other comm never
/// reaches the cmdline read). `None` whenever the shell owns the
/// foreground, the inputs are missing, or the argv is unreadable,
/// non-UTF-8, or unparseable: the band keeps today's local path,
/// never a wrong host.
pub fn foreground_ssh_destination(
    master: Option<&std::fs::File>,
    shell_pid: Option<u32>,
) -> Option<String> {
    let (Some(master), Some(shell_pid)) = (master, shell_pid) else {
        return None;
    };

    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        // SAFETY: both calls read kernel pty/process state only;
        // neither touches caller memory.
        let fg = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
        let shell_pgrp = i32::try_from(shell_pid).map_or(-1, |pid| unsafe { libc::getpgid(pid) });
        (fg > 0 && shell_pgrp > 0 && fg != shell_pgrp)
            .then(|| read_ssh_destination(fg))
            .flatten()
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (master, shell_pid);
        None
    }
}

/// `/proc/<pid>/cmdline` as the ssh stand-in's source (status-remote
/// spec): the process's own comm must read `ssh` before the cmdline
/// is touched — the getter's gate, so a non-ssh foreground never
/// pays (or risks) the read. The NUL-split argv goes to the pure
/// parser ([`crate::status_bar::ssh_destination`]); strict UTF-8,
/// and anything unreadable or unparseable reads `None` — the local
/// path stands, never a guessed host.
#[cfg(target_os = "linux")]
fn read_ssh_destination(pid: i32) -> Option<String> {
    if read_comm(pid).as_deref() != Some("ssh") {
        return None;
    }
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    // The comm gate and this read are not atomic: an ssh that
    // exited in between can hand a recycled pid's argv to the
    // parser — an unrelated process's operands. The re-check
    // certifies the read before anything parses it, so the worst
    // case stays the ruled degrade (the local path), never a
    // wrong host.
    if read_comm(pid).as_deref() != Some("ssh") {
        return None;
    }
    // cmdline is NUL-separated with a trailing NUL; the bytes are
    // another process's own — strict UTF-8, degrading on anything
    // else rather than rendering a half-read token.
    let text = String::from_utf8(raw).ok()?;
    let argv: Vec<&str> = text.split('\0').filter(|token| !token.is_empty()).collect();
    crate::status_bar::ssh_destination(&argv)
}

/// Per-cell rendering metadata (background, underline color, cell
/// flags), interned in the terminal's [`Terminal::metadata_set`] so
/// cosmic-text attrs can reference it by index.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Metadata {
    pub bg: cosmic_text::Color,
    pub underline_color: cosmic_text::Color,
    pub flags: Flags,
}

impl Metadata {
    fn new(bg: cosmic_text::Color, underline_color: cosmic_text::Color) -> Self {
        let flags = Flags::empty();
        Self {
            bg,
            underline_color,
            flags,
        }
    }

    fn with_underline_color(self, underline_color: cosmic_text::Color) -> Self {
        Self {
            underline_color,
            ..self
        }
    }

    fn with_flags(self, flags: Flags) -> Self {
        Self { flags, ..self }
    }
}

/// One terminal: the `alacritty_terminal` grid behind a `FairMutex`,
/// its cosmic-text buffer, the PTY notifier, and the state that
/// renders one grid into the other.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is its own engine fact (focus, config, foreground bookkeeping, bright-bold); not a state machine to reshape"
)]
pub struct Terminal {
    pub metadata_set: IndexSet<Metadata>,
    pub needs_update: bool,
    pub term: Arc<FairMutex<Term<TaggedEventProxy>>>,
    pub url_regex_search: RegexSearch,
    pub regex_matches: Vec<alacritty_terminal::term::search::Match>,
    pub active_regex_match: Option<alacritty_terminal::term::search::Match>,
    pub active_hyperlink_id: Option<String>,
    bold_font_weight: Weight,
    buffer: Arc<Buffer>,
    is_focused: bool,
    master_fd: Option<fs::File>,
    colors: Colors,
    default_attrs: Attrs<'static>,
    dim_font_weight: Weight,
    mouse_reporter: MouseReporter,
    notifier: Notifier,
    /// Whether autosuggestions compute (autosuggest spec) — the
    /// resolved options flag, re-read at every `set_config`.
    autosuggestions_enabled: bool,
    /// Whether the path provider joins the merge (path-suggest spec)
    /// — the resolved options flag, re-read at every `set_config`;
    /// also stops the listing cache's reads when off.
    autosuggestions_paths_enabled: bool,
    /// Whether the command-name provider joins the merge
    /// (command-catalog spec) — the resolved options flag, re-read
    /// at every `set_config`. The snapshot itself is spawn truth and
    /// never re-walks.
    autosuggestions_commands_enabled: bool,
    /// The program the tab is spawned as (path-suggest spec) — spawn
    /// truth fixed for the terminal's life; the path provider's
    /// allowlist gate reads it at every observation build.
    spawned_program: String,
    /// The spawn-time HOME (path-suggest spec) — the `~` root, read
    /// from spawn-resolved truth at every listing refresh.
    spawn_home: Option<String>,
    /// The command catalog's spawn-time snapshot (command-catalog
    /// spec): the sorted executable basenames of the spawn-time
    /// `$PATH` — walked once at construction from the environment
    /// the pty exec inherits, never rescanned (PATH changes
    /// mid-session are rare; a rescan policy is ask-first forever).
    command_catalog: Vec<String>,
    /// The path provider's listing cache (path-suggest spec) — see
    /// [`PathListingCache`]; refreshed inside the synchronous update
    /// flow, never from a thread.
    path_cache: PathListingCache,
    /// The ghost tint (autosuggest spec, OQ-2): the scheme fg→bg
    /// blend, computed at construction and every `set_config` —
    /// never per frame.
    ghost_color: cosmic_text::Color,
    /// The foreground-transition bookkeeping (autosuggest spec): the
    /// foreground check's value at the previous update — the
    /// running→idle edge is the finish that marks the next
    /// observation a fresh prompt.
    last_foreground_child: bool,
    /// The command timer (command-lifecycle spec): engine-owned
    /// beside the diff that feeds it, timing always on — an
    /// `Option` write per update, display and tick gated downstream.
    /// A fresh terminal (a respawn) carries a fresh timer: no
    /// duration survives either.
    timer: CommandTimer,
    /// The drain-once finish latch (command-lifecycle spec):
    /// `command-notify`'s seam — armed with the just-frozen duration
    /// on the same edge that froze it, read exactly once
    /// ([`Self::take_command_finished`]). The band's frozen display
    /// is unaffected by the drain: it reads
    /// [`Self::command_frozen`], which serves until the next start.
    command_finished: Option<Duration>,
    /// The settle-close's fresh-prompt latch (autosuggest spec): set
    /// when the spawn settle closes, held until an anchor is adopted
    /// or a child runs — the shell's first prompt can land after the
    /// settling update.
    settle_prompt_pending: bool,
    /// The autosuggest state (autosuggest spec) — engine-owned, per
    /// the status-glance lesson: it dies with the terminal, so a
    /// respawn (a fresh terminal) resets history by construction.
    suggest: Suggest,
    search_regex_opt: Option<RegexSearch>,
    search_value: String,
    shell_pid: Option<u32>,
    size: Size,
    /// The grid recorded at the tab's spawn — the resting grid the
    /// status band's grid segment measures deviation against
    /// (status-glance criterion 1). Owned by the [`SpawnSettle`]
    /// state machine; dies with the terminal, so a respawn (a fresh
    /// terminal) resets it by construction.
    spawn_grid: (u16, u16),
    spawn_settle: SpawnSettle,
    use_bright_bold: bool,
    zoom_adj: i8,
}

/// The spawn-grid settle (status-glance criterion 1): a fresh
/// terminal's first resizes are not user deviations — the
/// construction default, an opening surface's measuring stretch,
/// and the granted-size stream all land before the resting grid
/// exists — so the record re-baselines through them and freezes
/// only once the surface has settled.
enum SpawnSettle {
    /// The surface is opening: every resize re-baselines the record
    /// (opening churn) until the app's granted-size adoption arms
    /// the close.
    WaitingAdoption,
    /// A laid-out surface exists (or an adoption just landed): the
    /// next resize is the settling one — it records the resting
    /// grid and closes the settle.
    NextResize,
    /// The resting grid is recorded; every later resize is a
    /// deviation for the band to report.
    Settled,
}

/// The path provider's engine-side listing cache (path-suggest
/// spec): the canonical parent directory of the token being
/// completed — the cache key — paired with its sorted (name,
/// is-a-directory) entries. Refreshed inside the synchronous update
/// flow, only when the key changes (never per keystroke within one
/// parent) and only on an eligible, enabled tab; any read error
/// caches an empty listing, so IO failure degrades to silence,
/// never a panic and never a stale entry after the key changes.
#[derive(Default)]
struct PathListingCache {
    /// The canonical parent the entries belong to; `None` until a
    /// first resolvable path token arrives.
    key: Option<PathBuf>,
    /// The key's entries, sorted by name; empty on any read error.
    entries: Vec<(String, bool)>,
}

impl PathListingCache {
    /// The reduced listing for `token`'s canonical parent
    /// (path-suggest spec): the observation's `path_listing` input.
    /// `gates_open` is the master toggle and the spawn allowlist
    /// together — a closed gate returns before any filesystem touch —
    /// and `token: None` (no path-shaped word being entered) is no
    /// path-completion context. The filesystem read happens only when
    /// the resolved parent differs from the cached key.
    fn listing_for<'a>(
        &'a mut self,
        gates_open: bool,
        token: Option<&str>,
        home: Option<&str>,
        cwd: Option<&Path>,
    ) -> Option<&'a [(String, bool)]> {
        if !gates_open {
            return None;
        }
        let token = token?;
        let parent = resolve_token_parent(token, home, cwd)?;
        // The canonical parent is the key — same directory under two
        // spellings reads once; the resolved spelling keys the
        // silence when canonicalization itself fails (the directory
        // does not exist, or cannot be walked).
        let key = fs::canonicalize(&parent).unwrap_or(parent);
        if self.key.as_ref() != Some(&key) {
            self.entries = read_sorted_entries(&key);
            self.key = Some(key);
        }
        Some(&self.entries)
    }
}

/// The token's parent directory, resolved absolute (path-suggest
/// spec): everything through the last `/` — or the whole `~` when
/// the token is the HOME root alone — with `~` expanded through the
/// spawn-time HOME and a relative parent joined onto the tab's
/// working directory. `None` when the parent cannot resolve (`~`
/// with no HOME, a relative parent with no cwd): silence, never a
/// guess.
fn resolve_token_parent(token: &str, home: Option<&str>, cwd: Option<&Path>) -> Option<PathBuf> {
    // The parent is everything through the last `/` — or the whole
    // `~` when the token is the HOME root alone.
    let parent = match token.rfind('/') {
        Some(idx) => &token[..=idx],
        None => token,
    };
    let path = if parent == "~" || parent.starts_with("~/") {
        let home = home?;
        let trimmed = home.trim_end_matches('/');
        PathBuf::from(format!("{trimmed}{}", &parent[1..]))
    } else {
        PathBuf::from(parent)
    };
    if path.is_absolute() {
        Some(path)
    } else {
        cwd.map(|cwd| cwd.join(path))
    }
}

/// One directory's sorted (name, is-a-directory) entries
/// (path-suggest spec): all-or-nothing — any error along the way
/// (unreadable directory, a failed stat, a non-UTF-8 name that could
/// never be safely suggested) yields the empty listing. The
/// is-a-directory bit is readdir's own `d_type` truth: a symlink to
/// a directory completes without the `/`, the honest no-follow
/// answer.
fn read_sorted_entries(dir: &Path) -> Vec<(String, bool)> {
    let Some(read_dir) = fs::read_dir(dir).ok() else {
        return Vec::new();
    };
    let mut entries: Vec<(String, bool)> = read_dir
        .map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let is_dir = entry.file_type().ok()?.is_dir();
            Some((name, is_dir))
        })
        .collect::<Option<Vec<(String, bool)>>>()
        .unwrap_or_default();
    entries.sort_unstable();
    entries
}

/// The command catalog's snapshot walk (command-catalog spec): every
/// executable basename on `path` — regular files carrying an execute
/// bit, symlinks followed (the `python3` → `python3.11` shape PATH
/// abuts in) — deduplicated case-sensitively into one sorted set,
/// the shape the candidate logic scans. A missing or unreadable PATH
/// directory contributes nothing, never a guess; a non-UTF-8 name
/// could never be safely suggested, so it is skipped too.
fn command_catalog_from_path(path: Option<&str>) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;

    let Some(path) = path else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    for dir in std::env::split_paths(path) {
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            // `DirEntry::metadata` does not follow symlinks; the
            // joined path's stat does.
            let Ok(meta) = fs::metadata(entry.path()) else {
                continue;
            };
            if meta.is_file()
                && meta.permissions().mode() & 0o111 != 0
                && let Some(name) = entry.file_name().into_string().ok()
            {
                names.insert(name);
            }
        }
    }
    names.into_iter().collect()
}

impl Terminal {
    // TODO: error handling
    #[expect(
        clippy::similar_names,
        reason = "fg/bg pair is terminal color vocabulary"
    )]
    pub fn new(
        tab_id: TabId,
        event_tx: mpsc::UnboundedSender<(TabId, Event)>,
        term_config: Config,
        pty_options: &Options,
        options: &TerminalOptions,
        colors: &Colors,
    ) -> Result<Self, io::Error> {
        let font_stretch = options.font_stretch;
        let font_weight = options.font_weight;
        let dim_font_weight = options.dim_font_weight;
        let bold_font_weight = options.bold_font_weight;
        let use_bright_bold = options.use_bright_bold;

        let metrics = options.metrics(0);

        let default_bg = convert_color(colors, Color::Named(NamedColor::Background));
        let default_fg = convert_color(colors, Color::Named(NamedColor::Foreground));

        let mut metadata_set = IndexSet::new();
        let default_metada = Metadata::new(default_bg, default_fg);
        let (default_metada_idx, _) = metadata_set.insert_full(default_metada);

        // TODO: set color to default fg
        let default_attrs = Attrs::new()
            .family(Family::Monospace)
            .weight(Weight(font_weight))
            .stretch(font_stretch)
            .color(default_fg)
            .metadata(default_metada_idx);

        let mut buffer = Buffer::new_empty(metrics);

        let (cell_width, cell_height) = {
            let mut font_system = font_system().write().unwrap();
            let font_system = font_system.raw();
            buffer.set_wrap(Wrap::None);

            // Use size of space to determine cell size
            buffer.set_text(" ", &default_attrs, Shaping::Advanced, None);
            let layout = buffer.line_layout(font_system, 0).unwrap();
            let w = layout[0].w;
            buffer.set_monospace_width(Some(w));
            (w, metrics.line_height)
        };

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "default 80x24 grid in px: ceil is the min-size contract, far below u32::MAX"
        )]
        let size = Size {
            width: (80.0 * cell_width).ceil() as u32,
            height: (24.0 * cell_height).ceil() as u32,
            cell_width,
            cell_height,
        };
        let event_proxy = TaggedEventProxy {
            tab_id,
            tx: event_tx,
        };
        let term = Arc::new(FairMutex::new(Term::new(
            term_config,
            &size,
            event_proxy.clone(),
        )));

        let window_id = 0;
        let pty = tty::new(pty_options, size.into(), window_id)?;
        #[cfg(not(windows))]
        let shell_pid = Some(pty.child().id());
        #[cfg(windows)]
        let shell_pid = pty.child_watcher().pid().map(|pid| pid.get());

        // The dup'd master handle foreground-process detection reads
        // (close-confirm criterion 2): cloned before `pty` moves into
        // the EventLoop, the same construction-time window that
        // captures `shell_pid`. Non-Linux keeps `None` — the check
        // fails open there. A clone failure also reads as "no
        // process".
        #[cfg(target_os = "linux")]
        let master_fd = pty.file().try_clone().ok();
        #[cfg(not(target_os = "linux"))]
        let master_fd = None;

        let pty_event_loop = EventLoop::new(
            term.clone(),
            event_proxy,
            pty,
            pty_options.drain_on_exit,
            false,
        )?;
        let notifier = Notifier(pty_event_loop.channel());
        let _pty_join_handle = pty_event_loop.spawn();

        // The construction grid is the spawn record's honest starting
        // value: the default the pty opened at, replaced by the
        // resting grid once the surface's opening settles.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "cell counts fit u16 by construction — a screen beyond 65k cells does not exist"
        )]
        let spawn_grid = (size.columns() as u16, size.screen_lines() as u16);

        Ok(Self {
            active_regex_match: None,
            active_hyperlink_id: None,
            url_regex_search: url_regex_search(),
            regex_matches: Vec::new(),
            bold_font_weight: Weight(bold_font_weight),
            buffer: Arc::new(buffer),
            colors: *colors,
            default_attrs,
            dim_font_weight: Weight(dim_font_weight),
            metadata_set,
            mouse_reporter: MouseReporter::default(),
            needs_update: true,
            notifier,
            autosuggestions_enabled: options.autosuggestions_enabled,
            autosuggestions_paths_enabled: options.autosuggestions_paths_enabled,
            autosuggestions_commands_enabled: options.autosuggestions_commands_enabled,
            spawned_program: options.spawned_program.clone(),
            spawn_home: options.spawn_home.clone(),
            command_catalog: command_catalog_from_path(options.spawn_path.as_deref()),
            path_cache: PathListingCache::default(),
            ghost_color: suggest::ghost_color(colors),
            last_foreground_child: false,
            timer: CommandTimer::default(),
            command_finished: None,
            settle_prompt_pending: false,
            suggest: Suggest::new(),
            search_regex_opt: None,
            search_value: String::new(),
            shell_pid,
            size,
            spawn_grid,
            spawn_settle: SpawnSettle::WaitingAdoption,
            term,
            use_bright_bold,
            zoom_adj: Default::default(),
            is_focused: true,
            master_fd,
        })
    }

    pub fn buffer_weak(&self) -> Weak<Buffer> {
        Arc::downgrade(&self.buffer)
    }

    /// Get the internal [`Buffer`]
    pub fn with_buffer<F: FnOnce(&Buffer) -> T, T>(&self, f: F) -> T {
        f(&self.buffer)
    }

    /// Get the internal [`Buffer`], mutably
    pub fn with_buffer_mut<F: FnOnce(&mut Buffer) -> T, T>(&mut self, f: F) -> T {
        f(Arc::make_mut(&mut self.buffer))
    }

    pub fn colors(&self) -> &Colors {
        &self.colors
    }

    pub fn effective_color(&self, index: usize) -> Rgb {
        if index == NamedColor::Background as usize {
            self.colors[index].unwrap_or_else(|| {
                // Allow using an unset background
                let [r, g, b, _] =
                    cosmic_text::Color(WINDOW_BG_COLOR.load(Ordering::SeqCst)).as_rgba();
                Rgb { r, g, b }
            })
        } else {
            self.colors[index].unwrap_or_default()
        }
    }

    pub fn default_attrs(&self) -> &Attrs<'static> {
        &self.default_attrs
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// The terminal's cell dimensions (cols, rows) — an in-memory
    /// derivation from the live [`Size`], read at view assembly
    /// under the same lock as [`Self::zoom_adj`], so the status
    /// band's grid segment tracks resizes without any snapshot or
    /// syscall discipline of its own.
    pub fn grid(&self) -> (u16, u16) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "cell counts fit u16 by construction — a screen beyond 65k cells does not exist"
        )]
        let (cols, rows) = (self.size.columns() as u16, self.size.screen_lines() as u16);
        (cols, rows)
    }

    /// The grid recorded at the tab's spawn — the resting grid the
    /// status band's grid segment measures deviation against: the
    /// band narrates a grid only while the live one differs from
    /// this (status-glance criterion 1). Recorded through the
    /// spawn-settle machinery ([`Self::arm_spawn_grid_next_resize`]);
    /// a respawn is a fresh terminal, so its settle — and record —
    /// start over.
    pub fn spawn_grid(&self) -> (u16, u16) {
        self.spawn_grid
    }

    /// Arms the spawn-grid settle's close: the terminal's next
    /// resize records the resting grid and freezes the record. The
    /// app arms where a laid-out surface's granted size is adopted
    /// (the pin funnel — every show runs one) and, only when a live
    /// surface already exists, at spawn/respawn, where the fresh
    /// terminal's first — settling — resize is the resting one.
    /// Arming a settled record is a no-op: a later adoption (every
    /// re-show) must never re-snapshot a frozen record.
    pub fn arm_spawn_grid_next_resize(&mut self) {
        if matches!(self.spawn_settle, SpawnSettle::WaitingAdoption) {
            self.spawn_settle = SpawnSettle::NextResize;
        }
    }

    pub fn zoom_adj(&self) -> i8 {
        self.zoom_adj
    }

    pub fn set_zoom_adj(&mut self, value: i8) {
        self.zoom_adj = value;
    }

    pub fn set_focused(&mut self, is_focused: bool) {
        let focus_changed = self.is_focused != is_focused;
        self.is_focused = is_focused;

        if focus_changed {
            let report_focus = self.term.lock().mode().contains(TermMode::FOCUS_IN_OUT);
            if report_focus {
                const FOCUS_IN: &[u8] = b"\x1b[I";
                const FOCUS_OUT: &[u8] = b"\x1b[O";

                let input = if is_focused { FOCUS_IN } else { FOCUS_OUT };
                self.input_no_scroll(input);
            }
        }
    }

    pub fn redraw(&self) -> bool {
        self.buffer.redraw()
    }

    pub fn set_redraw(&mut self, redraw: bool) {
        self.with_buffer_mut(|buffer| buffer.set_redraw(redraw));
    }

    pub fn input_no_scroll<I: Into<Cow<'static, [u8]>>>(&self, input: I) {
        self.notifier.notify(input);
    }

    pub fn working_directory(&self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            let shell_pid = self.shell_pid?;
            fs::read_link(format!("/proc/{shell_pid}/cwd")).ok()
        }

        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// Whether a process group other than this terminal's shell holds
    /// the PTY foreground (close-confirm criterion 2) — the "a process
    /// is running" signal, read lazily off the dup'd master handle and
    /// the recorded shell pid. Fails open (criterion 7): a missing
    /// handle or unreadable state reads as "no process".
    pub fn foreground_child_running(&self) -> bool {
        foreground_child_running(self.master_fd.as_ref(), self.shell_pid)
    }

    /// The foreground process group's name for the status band:
    /// [`foreground_process_name`] over this terminal's dup'd master
    /// and recorded shell pid — `None` reads idle/unreadable, and the
    /// band falls back to the recorded shell's basename.
    pub fn foreground_process_name(&self) -> Option<String> {
        foreground_process_name(self.master_fd.as_ref(), self.shell_pid)
    }

    /// The ssh stand-in's destination token for the status band's
    /// path seat (status-remote spec): [`foreground_ssh_destination`]
    /// over this terminal's dup'd master and recorded shell pid —
    /// `None` reads idle, non-ssh, or unparseable, and the band
    /// keeps the local path.
    pub fn ssh_destination(&self) -> Option<String> {
        foreground_ssh_destination(self.master_fd.as_ref(), self.shell_pid)
    }

    /// The running command's state (command-lifecycle spec): whether
    /// a foreground child holds the PTY — the engine's cached kernel
    /// truth from the last update, no syscall here — and its live
    /// elapsed when the timer witnessed the start. A command born
    /// mid-observation runs with no elapsed: the both-edges honesty
    /// rule, never a guessed duration.
    pub fn command_live(&self) -> (bool, Option<Duration>) {
        (
            self.last_foreground_child,
            self.timer.live_elapsed(Instant::now()),
        )
    }

    /// The last finished command's duration (command-lifecycle
    /// spec), served until the next command starts (OQ-5).
    pub fn command_frozen(&self) -> Option<Duration> {
        self.timer.last_duration()
    }

    /// Drains the finish latch once (command-lifecycle spec): the
    /// frozen duration of the most recently finished command,
    /// returned exactly once per finish — `command-notify`'s seam.
    /// The band's frozen display reads [`Self::command_frozen`] and
    /// is unaffected by the drain.
    pub fn take_command_finished(&mut self) -> Option<Duration> {
        self.command_finished.take()
    }

    pub fn input_scroll<I: Into<Cow<'static, [u8]>>>(&self, input: I) {
        self.input_no_scroll(input);
        self.scroll(TerminalScroll::Bottom);
    }

    pub fn paste(&self, value: &str) {
        // This code is ported from alacritty
        let bracketed_paste = {
            let term = self.term.lock();
            term.mode().contains(TermMode::BRACKETED_PASTE)
        };
        if bracketed_paste {
            self.input_no_scroll(&b"\x1b[200~"[..]);
            self.input_no_scroll(value.replace('\x1b', "").into_bytes());
            self.input_scroll(&b"\x1b[201~"[..]);
        } else {
            // In non-bracketed (ie: normal) mode, terminal applications cannot distinguish
            // pasted data from keystrokes.
            // In theory, we should construct the keystrokes needed to produce the data we are
            // pasting... since that's neither practical nor sensible (and probably an impossible
            // task to solve in a general way), we'll just replace line breaks (windows and unix
            // style) with a single carriage return (\r, which is what the Enter key produces).
            self.input_scroll(value.replace("\r\n", "\r").replace('\n', "\r").into_bytes());
        }
    }

    /// Writes the showing suggestion's suffix into the PTY (autosuggest
    /// spec) — the exact funnel every keystroke takes, raw bytes, no
    /// bracketed-paste wrapping: typed characters, not a paste.
    /// [`AcceptKind::Full`] sends the whole suffix,
    /// [`AcceptKind::Word`] one fish-shaped forward word (the
    /// remainder re-suggests after the echo settles). With nothing
    /// showing, nothing is written.
    pub fn accept_suggestion(&mut self, kind: AcceptKind) {
        let Some(suffix) = self.suggest.take_suffix() else {
            return;
        };
        let bytes = match kind {
            AcceptKind::Full => suffix,
            AcceptKind::Word => suggest::split_word(&suffix).0.to_string(),
        };
        if !bytes.is_empty() {
            self.input_no_scroll(bytes.into_bytes());
        }
    }

    /// The showing suggestion's suffix (autosuggest spec) — the only
    /// thing the widget renders; `None` renders nothing.
    pub fn suggestion(&self) -> Option<&str> {
        self.suggest
            .showing()
            .map(|showing| showing.suffix.as_str())
    }

    /// The shared spawn gate (path-suggest + command-catalog specs):
    /// the spawned program is an allowlisted shell — computed from
    /// spawn-resolved truth that never changes over the terminal's
    /// life, read at every observation build. Every static local
    /// provider (paths, catalog) gates on it; history never does.
    fn paths_eligible(&self) -> bool {
        suggest::is_allowlisted_shell(&self.spawned_program)
    }

    /// The ghost tint (autosuggest spec, OQ-2): the scheme's fg→bg
    /// blend, stored at construction and every `set_config`.
    pub fn ghost_color(&self) -> cosmic_text::Color {
        self.ghost_color
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width != self.size.width || height != self.size.height {
            let instant = Instant::now();

            // Clamp dimensions to ensure at least 1 row and 1 column,
            // preventing index-out-of-bounds panics in alacritty_terminal.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "one-cell minimum: cell px are positive and far below u32::MAX"
            )]
            let min_width = self.size.cell_width.ceil() as u32;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "one-cell minimum: cell px are positive and far below u32::MAX"
            )]
            let min_height = self.size.cell_height.ceil() as u32;
            self.size.width = width.max(min_width);
            self.size.height = height.max(min_height);

            // The spawn-grid settle's only write site: opening churn
            // re-baselines, the settling resize records the resting
            // grid and freezes, and everything after is a deviation.
            match self.spawn_settle {
                SpawnSettle::WaitingAdoption => self.spawn_grid = self.grid(),
                SpawnSettle::NextResize => {
                    self.spawn_grid = self.grid();
                    self.spawn_settle = SpawnSettle::Settled;
                    // The settle's close is a fresh-prompt moment
                    // (autosuggest spec): the update that follows
                    // may adopt the shell's first prompt.
                    self.settle_prompt_pending = true;
                }
                SpawnSettle::Settled => {}
            }

            self.notifier.on_resize(self.size.into());
            self.term.lock().resize(self.size);

            #[expect(
                clippy::cast_precision_loss,
                reason = "surface px are < 2^24, exact in f32"
            )]
            self.with_buffer_mut(|buffer| {
                buffer.set_size(Some(width as f32), Some(height as f32));
            });

            self.needs_update = true;

            log::debug!("resize {:?}", instant.elapsed());
        }
    }

    pub fn scroll(&self, scroll: TerminalScroll) {
        self.term.lock().scroll_display(scroll);
    }

    pub fn scroll_to(&self, ratio: f32) {
        let mut term = self.term.lock();
        let grid = term.grid();
        let total = grid.history_size() + grid.screen_lines();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "display offset and line counts are bounded by grid capacity (< i32::MAX)"
        )]
        let old_display_offset = grid.display_offset() as i32;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            clippy::cast_precision_loss,
            reason = "total lines < 2^24 (exact in f32); the ratio math is the scroll contract"
        )]
        let new_display_offset =
            ((total as f32) * (1.0 - ratio)) as i32 - grid.screen_lines() as i32;
        term.scroll_display(TerminalScroll::Delta(
            new_display_offset - old_display_offset,
        ));
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "line counts < 2^24, exact in f32"
    )]
    pub fn scrollbar(&self) -> Option<(f32, f32)> {
        let term = self.term.lock();
        let grid = term.grid();
        if grid.history_size() > 0 {
            let total = grid.history_size() + grid.screen_lines();
            let start = total - grid.display_offset() - grid.screen_lines();
            let end = total - grid.display_offset();
            Some((
                (start as f32) / (total as f32),
                (end as f32) / (total as f32),
            ))
        } else {
            None
        }
    }

    /// The viewport's lines scrolled back from the bottom
    /// (status-scrolled spec): `display_offset` under the same
    /// term lock the scrollbar's math takes, saturated into `u16`.
    /// Zero at the bottom — the band's chip filters it to absence
    /// downstream.
    pub fn scrolled_back_lines(&self) -> u16 {
        let term = self.term.lock();
        u16::try_from(term.grid().display_offset()).unwrap_or(u16::MAX)
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "history and screen line counts are bounded by grid capacity (< i32::MAX)"
    )]
    pub fn search(&mut self, value: &str, forwards: bool) {
        // TODO: set max lines, run in thread?
        {
            let mut term = self.term.lock();

            if self.search_value != value {
                match RegexSearch::new(value) {
                    Ok(search_regex) => {
                        self.search_regex_opt = Some(search_regex);
                        self.search_value = value.to_string();
                        term.selection = None;
                    }
                    Err(err) => {
                        log::warn!("failed to parse regex {value:?}: {err}");
                        return;
                    }
                }
            }

            let Some(search_regex) = &mut self.search_regex_opt else {
                return;
            };

            // Determine search origin
            let grid = term.grid();
            let search_origin = match term
                .selection
                .as_ref()
                .and_then(|selection| selection.to_range(&term))
            {
                Some(range) => {
                    // TODO: determine correct search_origin, along with side below
                    if forwards {
                        range.end.add(grid, Boundary::Grid, 1)
                    } else {
                        range.start.sub(grid, Boundary::Grid, 1)
                    }
                }
                None => {
                    if forwards {
                        Point::new(Line(-(grid.history_size() as i32)), Column(0))
                    } else {
                        Point::new(
                            Line(grid.screen_lines() as i32 - 1),
                            Column(grid.columns() - 1),
                        )
                    }
                }
            };

            // Find next search match
            if let Some(search_match) = term.search_next(
                search_regex,
                search_origin,
                if forwards {
                    Direction::Right
                } else {
                    Direction::Left
                },
                // TODO: determine correct side, along with search_origin above
                if forwards { Side::Left } else { Side::Right },
                None,
            ) {
                // Scroll to match
                if forwards {
                    term.scroll_to_point(*search_match.end());
                } else {
                    term.scroll_to_point(*search_match.start());
                }

                // Set selection to match
                let mut selection =
                    Selection::new(SelectionType::Simple, *search_match.start(), Side::Left);
                selection.update(*search_match.end(), Side::Right);
                term.selection = Some(selection);
            }
        }

        self.update();
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "history and screen line counts are bounded by grid capacity (< i32::MAX)"
    )]
    pub fn select_all(&mut self) {
        {
            let mut term = self.term.lock();
            let grid = term.grid();
            let start = Point::new(Line(-(grid.history_size() as i32)), Column(0));
            let mut end_line = grid.bottommost_line();
            while end_line.0 > 0 {
                if !grid[end_line].is_clear() {
                    break;
                }
                end_line.0 -= 1;
            }
            let end = Point::new(end_line, Column(grid.columns() - 1));
            let mut selection = Selection::new(SelectionType::Lines, start, Side::Left);
            selection.update(end, Side::Right);
            term.selection = Some(selection);
        }
        self.update();
    }

    /// Apply a new [`TerminalOptions`] and an already-resolved color scheme.
    ///
    /// Unlike cosmic-term's `set_config`, the caller (the app) picks the
    /// active theme's `Colors` up front (from
    /// [`crate::terminal::theme_source::by_name`]) rather than this method
    /// looking them up in a profile-keyed theme map.
    pub fn set_config(&mut self, options: &TerminalOptions, colors: &Colors) {
        let mut update_cell_size = false;
        let mut update = false;
        let zoom_adj = self.zoom_adj;
        if self.default_attrs.stretch != options.font_stretch {
            self.default_attrs = self.default_attrs.clone().stretch(options.font_stretch);
            update_cell_size = true;
        }

        if self.default_attrs.weight.0 != options.font_weight {
            self.default_attrs = self
                .default_attrs
                .clone()
                .weight(Weight(options.font_weight));
            update_cell_size = true;
        }

        if self.dim_font_weight.0 != options.dim_font_weight {
            self.dim_font_weight = Weight(options.dim_font_weight);
            update_cell_size = true;
        }

        if self.bold_font_weight.0 != options.bold_font_weight {
            self.bold_font_weight = Weight(options.bold_font_weight);
            update_cell_size = true;
        }

        if self.use_bright_bold != options.use_bright_bold {
            self.use_bright_bold = options.use_bright_bold;
            update_cell_size = true;
        }

        let metrics = options.metrics(zoom_adj);
        if metrics != self.buffer.metrics() {
            self.with_buffer_mut(|buffer| buffer.set_metrics(metrics));
            update_cell_size = true;
        }

        {
            let mut changed = false;
            for i in 0..color::COUNT {
                if self.colors[i] != colors[i] {
                    self.colors[i] = colors[i];
                    changed = true;
                }
            }
            if changed {
                update = true;
            }
        }

        if self.autosuggestions_enabled != options.autosuggestions_enabled {
            self.autosuggestions_enabled = options.autosuggestions_enabled;
            update = true;
        }

        if self.autosuggestions_paths_enabled != options.autosuggestions_paths_enabled {
            self.autosuggestions_paths_enabled = options.autosuggestions_paths_enabled;
            update = true;
        }

        if self.autosuggestions_commands_enabled != options.autosuggestions_commands_enabled {
            self.autosuggestions_commands_enabled = options.autosuggestions_commands_enabled;
            update = true;
        }

        // The ghost tint recomputes from the (possibly just-copied)
        // scheme — three channel averages, cheaper than the diff that
        // guards them.
        self.ghost_color = suggest::ghost_color(&self.colors);

        // NOTE: this is done on every set_config because the changed boolean above does not capture
        // WINDOW_BG changes
        let default_colors_updated = self.update_default_colors();

        if update_cell_size {
            self.update_cell_size();
        } else if update || default_colors_updated {
            self.update();
        }
    }

    #[expect(
        clippy::similar_names,
        reason = "fg/bg pair is terminal color vocabulary"
    )]
    pub fn update_default_colors(&mut self) -> bool {
        let default_bg = convert_color(&self.colors, Color::Named(NamedColor::Background));
        let default_fg = convert_color(&self.colors, Color::Named(NamedColor::Foreground));

        let new_default_metadata = Metadata::new(default_bg, default_fg);
        let curr_metada_idx = self.default_attrs().metadata;

        let updated = new_default_metadata != self.metadata_set[curr_metada_idx];

        if updated {
            self.metadata_set.clear();
            let (default_metadata_idx, _) = self.metadata_set.insert_full(new_default_metadata);

            self.default_attrs = Attrs::new()
                .family(Family::Monospace)
                .weight(self.default_attrs.weight)
                .stretch(self.default_attrs.stretch)
                .color(default_fg)
                .metadata(default_metadata_idx);
        }

        updated
    }

    pub fn update_cell_size(&mut self) {
        let default_attrs = self.default_attrs.clone();
        let (cell_width, cell_height) = {
            let mut font_system = font_system().write().unwrap();
            self.with_buffer_mut(|buffer| {
                buffer.set_wrap(Wrap::None);

                // Use size of space to determine cell size
                buffer.set_text(" ", &default_attrs, Shaping::Advanced, None);
                let layout = buffer.line_layout(font_system.raw(), 0).unwrap();
                let w = layout[0].w;
                buffer.set_monospace_width(Some(w));
                (w, buffer.metrics().line_height)
            })
        };

        let old_size = self.size;
        self.size = Size {
            width: 0,
            height: 0,
            cell_width,
            cell_height,
        };
        self.resize(old_size.width, old_size.height);

        self.update();
    }

    /// Rebuilds the cosmic-text buffer from the grid: text, attrs, and
    /// metadata for every visible cell, plus URL-regex matches.
    pub fn update(&mut self) -> bool {
        // LEFT‑TO‑RIGHT ISOLATE character.
        // This will be added to the beginning of lines to force the shaper to treat detected RTL
        // lines as LTR. RTL text would still be rendered correctly. But this fixes the wrong
        // behavior of it being aligned to the right.
        const LRI: char = '\u{2066}';

        let instant = Instant::now();

        // The autosuggest cycle (autosuggest spec): the kernel's
        // foreground truth read once per update, before the term
        // lock — the running→idle edge is the finish that marks the
        // next observation a fresh prompt.
        let foreground_child = self.foreground_child_running();
        // The path provider's spawn gate (path-suggest spec), read
        // before the buffer's mutable borrow the observation build
        // sits inside: spawn truth, so it never changes mid-life —
        // but the read must still not borrow `self` in there.
        let paths_eligible = self.paths_eligible();
        // The tab's working directory (the `/proc/{shell_pid}/cwd`
        // truth), hoisted for the same borrow reason: relative path
        // parents resolve against it.
        let working_directory = self.working_directory();
        let fresh_by_finish = self.last_foreground_child && !foreground_child;
        self.last_foreground_child = foreground_child;
        // The command timer rides the engine's own diff
        // (command-lifecycle spec) — timestamps always run, one
        // `Option` write per update; never suggest's, which lives
        // behind the autosuggest toggle. The finish edge arms the
        // notify latch with the just-frozen duration (unwitnessed
        // starts freeze nothing, so they latch nothing either).
        self.timer.step(foreground_child, instant);
        if fresh_by_finish && let Some(duration) = self.timer.last_duration() {
            self.command_finished = Some(duration);
        }

        // Only keep default
        self.metadata_set.truncate(1);

        // TODO: is redraw needed after all events?
        // TODO: use LineDamageBounds
        {
            let buffer = Arc::make_mut(&mut self.buffer);

            let mut line_i = 0;
            let mut last_point = None;
            let mut text = String::from(LRI);
            let mut attrs_list = AttrsList::new(&self.default_attrs);
            {
                let mut term = self.term.lock();
                // TODO: use damage?
                match term.damage() {
                    TermDamage::Full => {}
                    TermDamage::Partial(_damage_lines) => {}
                }
                term.reset_damage();

                self.regex_matches.clear();
                {
                    let mut regex_matches: Vec<_> =
                        visible_regex_match_iter(&term, &mut self.url_regex_search).collect();
                    self.regex_matches
                        .extend(regex_matches.drain(..).flat_map(|rm| -> Vec<_> {
                            HintPostProcessor::new(&term, &mut self.url_regex_search, rm).collect()
                        }));
                }

                // The observation + eligibility step (autosuggest
                // spec): one cursor-line scrape off the same walked
                // cells this update already iterates, under the lock
                // it already holds — no second grid lock anywhere in
                // the path.
                {
                    let grid = term.grid();
                    let cursor = grid.cursor.point;
                    let columns = grid.columns();
                    let row = &grid[cursor.line];
                    let mut before_cursor = String::new();
                    let mut after_cursor_blank = true;
                    for column in 0..columns {
                        let cell = &row[Column(column)];
                        if Column(column) < cursor.column {
                            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                                continue;
                            }
                            before_cursor.push(match cell.c {
                                '\t' => ' ',
                                c => c,
                            });
                            if let Some(zerowidth) = cell.zerowidth() {
                                for &c in zerowidth {
                                    before_cursor.push(c);
                                }
                            }
                        } else {
                            let blank = cell.c == ' '
                                && !cell.flags.intersects(
                                    Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
                                )
                                && cell.zerowidth().is_none();
                            if !blank {
                                after_cursor_blank = false;
                            }
                        }
                    }
                    // The path provider's data source (path-suggest
                    // spec): the trailing token's parent listing,
                    // engine-side IO reduced into the observation —
                    // the model never touches the filesystem. The
                    // cache reads only behind both gates and only
                    // when the canonical parent changes.
                    let path_listing = self.path_cache.listing_for(
                        self.autosuggestions_enabled
                            && self.autosuggestions_paths_enabled
                            && paths_eligible,
                        suggest::path_token(self.suggest.typed_segment(&before_cursor)),
                        self.spawn_home.as_deref(),
                        working_directory.as_deref(),
                    );
                    let observation = suggest::Observation {
                        enabled: self.autosuggestions_enabled,
                        paths_eligible,
                        paths_enabled: self.autosuggestions_paths_enabled,
                        path_listing,
                        commands_enabled: self.autosuggestions_commands_enabled,
                        command_catalog: &self.command_catalog,
                        focused: self.is_focused,
                        foreground_child,
                        display_offset: grid.display_offset(),
                        cursor_on_bottom_row: cursor.line == grid.bottommost_line(),
                        columns,
                        cursor_column: cursor.column.0,
                        before_cursor: &before_cursor,
                        after_cursor_blank,
                        fresh_prompt: fresh_by_finish || self.settle_prompt_pending,
                    };
                    suggest::step(&observation, &mut self.suggest);
                    // The settle latch drains once a prompt exists
                    // (or a child runs): until then every update may
                    // be the one that finds the shell's first prompt.
                    if self.suggest.anchored() || foreground_child {
                        self.settle_prompt_pending = false;
                    }
                }

                let grid = term.grid();
                for indexed in grid.display_iter() {
                    if indexed.point.line != last_point.unwrap_or(indexed.point).line {
                        while line_i >= buffer.lines.len() {
                            buffer.lines.push(BufferLine::new(
                                "",
                                LineEnding::default(),
                                AttrsList::new(&self.default_attrs),
                                Shaping::Advanced,
                            ));
                            buffer.set_redraw(true);
                        }

                        if buffer.lines[line_i].set_text(
                            text.clone(),
                            LineEnding::default(),
                            attrs_list.clone(),
                        ) {
                            buffer.set_redraw(true);
                        }
                        line_i += 1;

                        text.clear();
                        text.push(LRI);
                        attrs_list.clear_spans();
                    }
                    // TODO: use indexed.point.column?

                    // TODO: skip leading spacer?
                    if indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        // Skip wide spacers (cells after wide characters)
                        continue;
                    }

                    let start = text.len();
                    // Tab skip/stop is handled by alacritty_terminal
                    text.push(match indexed.cell.c {
                        '\t' => ' ',
                        c => c,
                    });
                    if let Some(zerowidth) = indexed.cell.zerowidth() {
                        for &c in zerowidth {
                            text.push(c);
                        }
                    }
                    let end = text.len();

                    let mut attrs = self.default_attrs.clone();

                    let cell_fg = if indexed.cell.flags.contains(Flags::DIM) {
                        as_dim(indexed.cell.fg)
                    } else if self.use_bright_bold && indexed.cell.flags.contains(Flags::BOLD) {
                        as_bright(indexed.cell.fg)
                    } else {
                        indexed.cell.fg
                    };

                    let (mut fg, mut bg) = if indexed.cell.flags.contains(Flags::INVERSE) {
                        (
                            convert_color(&self.colors, indexed.cell.bg),
                            convert_color(&self.colors, cell_fg),
                        )
                    } else {
                        (
                            convert_color(&self.colors, cell_fg),
                            convert_color(&self.colors, indexed.cell.bg),
                        )
                    };

                    if indexed.cell.flags.contains(Flags::HIDDEN) {
                        fg = bg;
                    }

                    // Change color if cursor
                    if indexed.point == grid.cursor.point
                        && term.renderable_content().cursor.shape == CursorShape::Block
                        && self.is_focused
                    {
                        //Use specific cursor color if requested
                        if term.colors()[NamedColor::Cursor].is_some() {
                            fg = bg;
                            bg = convert_color(term.colors(), Color::Named(NamedColor::Cursor));
                        } else if self.colors[NamedColor::Cursor].is_some() {
                            //Use specific theme cursor color if exists
                            fg = bg;
                            bg = convert_color(&self.colors, Color::Named(NamedColor::Cursor));
                        } else {
                            mem::swap(&mut fg, &mut bg);
                        }
                        let fg_rgb = Rgb {
                            r: fg.r(),
                            g: fg.g(),
                            b: fg.b(),
                        };
                        let bg_rgb = Rgb {
                            r: bg.r(),
                            g: bg.g(),
                            b: bg.b(),
                        };
                        let contrast = fg_rgb.contrast(bg_rgb);
                        if contrast < MIN_CURSOR_CONTRAST {
                            fg = convert_color(&self.colors, Color::Named(NamedColor::Background));
                            bg = convert_color(&self.colors, Color::Named(NamedColor::Foreground));
                        }
                    }

                    // Change color if selected
                    if let Some(selection) = &term.selection
                        && let Some(range) = selection.to_range(&term)
                        && range.contains(indexed.point)
                    {
                        // TODO: better handling of selection
                        mem::swap(&mut fg, &mut bg);
                    }

                    // Convert foreground to linear
                    attrs = attrs.color(fg);

                    let underline_color = indexed
                        .cell
                        .underline_color()
                        .map_or(fg, |c| convert_color(&self.colors, c));

                    let mut flags = indexed.cell.flags;

                    if let Some(active_match) = &self.active_regex_match
                        && active_match.contains(&indexed.point)
                    {
                        flags |= Flags::UNDERLINE;
                    }
                    if let Some(active_id) = &self.active_hyperlink_id {
                        let mut matches_active = indexed
                            .cell
                            .hyperlink()
                            .is_some_and(|link| link.id() == active_id);
                        if !matches_active
                            && indexed.cell.flags.intersects(
                                Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
                            )
                            && indexed.point.column.0 > 0
                        {
                            matches_active = grid[Point::new(
                                indexed.point.line,
                                Column(indexed.point.column.0 - 1),
                            )]
                            .hyperlink()
                            .is_some_and(|link| link.id() == active_id);
                        }
                        if matches_active {
                            flags |= Flags::UNDERLINE;
                        }
                    }

                    let metadata = Metadata::new(bg, fg)
                        .with_flags(flags)
                        .with_underline_color(underline_color);
                    let (meta_idx, _) = self.metadata_set.insert_full(metadata);
                    attrs = attrs.metadata(meta_idx);

                    // TODO: more flags
                    if indexed.cell.flags.contains(Flags::BOLD) {
                        attrs = attrs.weight(self.bold_font_weight);
                    } else if indexed.cell.flags.contains(Flags::DIM) {
                        // if DIM and !BOLD
                        attrs = attrs.weight(self.dim_font_weight);
                    }
                    if indexed.cell.flags.contains(Flags::ITALIC) {
                        // TODO: automatically use fake italic
                        attrs = attrs.cache_key_flags(CacheKeyFlags::FAKE_ITALIC);
                    }
                    if attrs != attrs_list.defaults() {
                        attrs_list.add_span(start..end, &attrs);
                    }

                    last_point = Some(indexed.point);
                }
            }

            // TODO: do not repeat!
            while line_i >= buffer.lines.len() {
                buffer.lines.push(BufferLine::new(
                    "",
                    LineEnding::default(),
                    AttrsList::new(&self.default_attrs),
                    Shaping::Advanced,
                ));
                buffer.set_redraw(true);
            }

            if buffer.lines[line_i].set_text(text, LineEnding::default(), attrs_list) {
                buffer.set_redraw(true);
            }
            line_i += 1;

            if buffer.lines.len() != line_i {
                buffer.lines.truncate(line_i);
                buffer.set_redraw(true);
            }

            // Shape and trim shape run cache
            {
                let mut font_system = font_system().write().unwrap();
                buffer.shape_until_scroll(font_system.raw(), true);
                font_system.raw().shape_run_cache.trim(1024);
            }
        }

        log::debug!("buffer update {:?}", instant.elapsed());

        self.buffer.redraw()
    }

    pub fn viewport_to_point(&self, point: Point<usize>) -> Point {
        let term = self.term.lock();
        viewport_to_point(term.grid().display_offset(), point)
    }

    pub fn report_mouse(
        &mut self,
        event: &cosmic::iced::Event,
        modifiers: cosmic::iced::keyboard::Modifiers,
        x: u32,
        y: u32,
    ) {
        let term_lock = self.term.lock();
        let mode = term_lock.mode();

        #[expect(
            clippy::collapsible_else_if,
            reason = "the SGR/normal split stays parallel if/else blocks, mirroring upstream's shape"
        )]
        if mode.contains(TermMode::SGR_MOUSE) {
            if let Some(code) = self.mouse_reporter.sgr_mouse_code(event, modifiers, x, y) {
                self.input_no_scroll(code);
            }
        } else {
            if let Some(code) = self.mouse_reporter.normal_mouse_code(
                event,
                modifiers,
                mode.contains(TermMode::UTF8_MOUSE),
                x,
                y,
            ) {
                self.input_no_scroll(code);
            }
        }
    }

    pub fn scroll_mouse(
        &mut self,
        delta: ScrollDelta,
        modifiers: cosmic::iced::keyboard::Modifiers,
        x: u32,
        y: u32,
    ) {
        let is_sgr = self.term.lock().mode().contains(TermMode::SGR_MOUSE);

        if is_sgr {
            let codes = self.mouse_reporter.sgr_mouse_wheel_scroll(
                self.size().cell_width,
                self.size().cell_height,
                delta,
                &modifiers,
                x,
                y,
            );

            for code in codes {
                self.notifier.notify(code);
            }
        } else {
            self.scroll_as_arrows(delta);
        }
    }

    pub fn scroll_as_arrows(&mut self, delta: ScrollDelta) {
        const SCROLL_SPEED: u32 = 3;
        let cell_width = self.size().cell_width;
        let cell_height = self.size().cell_height;
        let (_, lines_y) = self
            .mouse_reporter
            .accumulate_scroll(delta, cell_width, cell_height);
        let is_app_cursor = self.term.lock().mode().contains(TermMode::APP_CURSOR);
        let (up, down) = if is_app_cursor {
            (&b"\x1BOA"[..], &b"\x1BOB"[..])
        } else {
            (&b"\x1B[A"[..], &b"\x1B[B"[..])
        };
        for _ in 0..(lines_y.unsigned_abs() * SCROLL_SPEED) {
            if lines_y > 0 {
                self.input_no_scroll(up);
            } else if lines_y < 0 {
                self.input_no_scroll(down);
            }
        }
    }
}
/// Iterate over all visible regex matches.
/// This includes the screen ± 100 lines ([`MAX_SEARCH_LINES`]).
/// Duplicated from alacritty's display/hint.rs.
pub fn visible_regex_match_iter<'a, T>(
    term: &'a Term<T>,
    regex: &'a mut RegexSearch,
) -> impl Iterator<Item = alacritty_terminal::term::search::Match> + 'a {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "display offset is bounded by history capacity (< i32::MAX)"
    )]
    let viewport_start = Line(-(term.grid().display_offset() as i32));
    let viewport_end = viewport_start + term.bottommost_line();
    let mut start = term.line_search_left(Point::new(viewport_start, Column(0)));
    let mut end = term.line_search_right(Point::new(viewport_end, Column(0)));
    start.line = start.line.max(viewport_start - MAX_SEARCH_LINES);
    end.line = end.line.min(viewport_end + MAX_SEARCH_LINES);

    alacritty_terminal::term::search::RegexIter::new(start, end, Direction::Right, term, regex)
        .skip_while(move |rm| rm.end().line < viewport_start)
        .take_while(move |rm| rm.start().line <= viewport_end)
}
/** Copy of <https://github.com/alacritty/alacritty/blob/4a7728bf7fac06a35f27f6c4f31e0d9214e5152b/alacritty/src/display/hint.rs#L433C1-L572C1> */
/// Iterator over all post-processed matches inside an existing hint match.
struct HintPostProcessor<'a, T> {
    /// Regex search DFAs.
    regex: &'a mut RegexSearch,

    /// Terminal reference.
    term: &'a Term<T>,

    /// Next hint match in the iterator.
    next_match: Option<alacritty_terminal::term::search::Match>,

    /// Start point for the next search.
    start: Point,

    /// End point for the hint match iterator.
    end: Point,
}

impl<'a, T> HintPostProcessor<'a, T> {
    /// Create a new iterator for an unprocessed match.
    fn new(
        term: &'a Term<T>,
        regex: &'a mut RegexSearch,
        regex_match: alacritty_terminal::term::search::Match,
    ) -> Self {
        let mut post_processor = Self {
            next_match: None,
            start: *regex_match.start(),
            end: *regex_match.end(),
            term,
            regex,
        };

        // Post-process the first hint match.
        post_processor.next_processed_match(regex_match);

        post_processor
    }

    /// Apply some hint post processing heuristics.
    ///
    /// This will check the end of the hint and make it shorter if certain characters are determined
    /// to be unlikely to be intentionally part of the hint.
    ///
    /// This is most useful for identifying URLs appropriately.
    fn hint_post_processing(
        &self,
        regex_match: &alacritty_terminal::term::search::Match,
    ) -> Option<alacritty_terminal::term::search::Match> {
        let mut iter = self.term.grid().iter_from(*regex_match.start());

        let mut c = iter.cell().c;

        // Truncate uneven number of brackets.
        let end = *regex_match.end();
        let mut open_parents = 0;
        let mut open_brackets = 0;
        loop {
            match c {
                '(' => open_parents += 1,
                '[' => open_brackets += 1,
                ')' => {
                    if open_parents == 0 {
                        alacritty_terminal::grid::BidirectionalIterator::prev(&mut iter);
                        break;
                    }
                    open_parents -= 1;
                }
                ']' => {
                    if open_brackets == 0 {
                        alacritty_terminal::grid::BidirectionalIterator::prev(&mut iter);
                        break;
                    }
                    open_brackets -= 1;
                }
                _ => (),
            }

            if iter.point() == end {
                break;
            }

            match iter.next() {
                Some(indexed) => c = indexed.cell.c,
                None => break,
            }
        }

        // Truncate trailing characters which are likely to be delimiters.
        let start = *regex_match.start();
        while iter.point() != start {
            if !matches!(c, '.' | ',' | ':' | ';' | '?' | '!' | '(' | '[' | '\'') {
                break;
            }

            match alacritty_terminal::grid::BidirectionalIterator::prev(&mut iter) {
                Some(indexed) => c = indexed.cell.c,
                None => break,
            }
        }

        if start > iter.point() {
            None
        } else {
            Some(start..=iter.point())
        }
    }

    /// Loop over submatches until a non-empty post-processed match is found.
    fn next_processed_match(&mut self, mut regex_match: alacritty_terminal::term::search::Match) {
        self.next_match = loop {
            if let Some(next_match) = self.hint_post_processing(&regex_match) {
                self.start = next_match.end().add(self.term, Boundary::Grid, 1);
                break Some(next_match);
            }

            self.start = regex_match.start().add(self.term, Boundary::Grid, 1);
            if self.start > self.end {
                return;
            }

            match self
                .term
                .regex_search_right(self.regex, self.start, self.end)
            {
                Some(rm) => regex_match = rm,
                None => return,
            }
        };
    }
}

impl<T> Iterator for HintPostProcessor<'_, T> {
    type Item = alacritty_terminal::term::search::Match;

    fn next(&mut self) -> Option<Self::Item> {
        let next_match = self.next_match.take()?;

        if self.start <= self.end
            && let Some(rm) = self
                .term
                .regex_search_right(self.regex, self.start, self.end)
        {
            self.next_processed_match(rm);
        }

        Some(next_match)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Ensure shutdown on terminal drop
        if let Err(err) = self.notifier.0.send(Msg::Shutdown) {
            log::warn!("Failed to send shutdown message on dropped terminal: {err}");
        }
    }
}

#[cfg(test)]
mod tests;
