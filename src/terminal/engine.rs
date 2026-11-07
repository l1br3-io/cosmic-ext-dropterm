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
    cell::Cell,
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fs, io, mem,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
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

// TEMPORARY AUTOSUGGEST DEBUG TRACE (dropterm debugging, remove after
// diagnosis): the last printed state line, so the trace prints only on
// change and stays readable across 60 Hz updates.
static ASDBG_LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

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
    /// Whether inline images render (iterm-images spec): resolved
    /// from the app config's `images_enabled` where options are
    /// built — flips apply at the next `set_config` pass, making the
    /// 1337 arm inert end to end while off.
    pub images_enabled: bool,
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
    /// Whether the curated-specs provider joins the autosuggest
    /// merge (flag-specs spec): `autosuggestions_specs_enabled` —
    /// engine-`true` until the config key lands (the catalog's
    /// CC-3 precedent, ruled OQ 4); flips apply at the next
    /// `set_config` pass, so a showing specs ghost disappears the
    /// moment the toggle lands while history keeps suggesting.
    pub autosuggestions_specs_enabled: bool,
    /// Whether the completion menu computes (suggest-menu spec):
    /// resolved from the app config's
    /// `autosuggestions_menu_enabled` — a flip applies at the next
    /// `set_config` pass, and an off flip closes a showing menu on
    /// the spot while the ghost keeps suggesting (the ghost and the
    /// trigger are separable).
    pub autosuggestions_menu_enabled: bool,
    /// The auto-popup opt-in (suggest-menu spec, Task 6): resolved
    /// from the app config's `autosuggestions_menu_auto` — with it
    /// on, an idle update whose typed segment collects two or more
    /// candidates opens the menu without the chord. `false` (the
    /// default) is today byte-for-bit: the block never runs.
    pub autosuggestions_menu_auto: bool,
    /// The idle-delay display gate (suggest-delay spec):
    /// `autosuggestions_delay_ms` as resolved from the app config —
    /// a qualifying suggestion is withheld until the typed segment
    /// has been stable this many milliseconds. Resolved per update
    /// like the toggles: a `set_config` pass applies it live (a
    /// lowered delay can reveal a withheld suggestion at the next
    /// update). `0` is fish parity — nothing is ever withheld.
    pub autosuggestions_delay_ms: u16,
    /// The user-declared alias map (suggest-aliases spec):
    /// `[autosuggestions.aliases]` as resolved from the app config —
    /// one serialized map field (the `custom_key_binds` precedent).
    /// Re-read at every `set_config`, so a map change applies live
    /// at the next typed prefix; empty (the default) re-targets
    /// nothing, today byte-for-byte.
    pub autosuggestions_aliases: HashMap<String, String>,
    /// The specs provider's table (suggest-specs-data spec): the
    /// built-in five overlaid by `completions.toml` beside the
    /// config, app-loaded and handed down engine-side — the
    /// `Cow::Borrowed` default is the built-in fallback itself, so
    /// no file is today's table without an allocation. A changed
    /// table applies at the next `set_config` pass, the next typed
    /// prefix borrowing the fresh rows.
    pub autosuggestions_specs_table: Cow<'static, [suggest::SpecCommand]>,
    /// Whether the generator provider runs (suggest-generators
    /// spec): resolved from the app config's
    /// `autosuggestions_generators_enabled` — a flip applies at the
    /// next `set_config` pass: `false` stops every spawn and blanks
    /// the observation's cache read on the echo (the stale rows
    /// never serve), `true` re-arms the ruled triggers. The shell
    /// gate and the resolved binary are spawn truth, never re-read
    /// by `set_config` — a respawn (a fresh terminal) is the only
    /// path that changes them.
    pub autosuggestions_generators_enabled: bool,
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
            images_enabled: true,
            spawned_program: String::new(),
            spawn_home: None,
            spawn_path: None,
            autosuggestions_paths_enabled: true,
            autosuggestions_commands_enabled: true,
            autosuggestions_specs_enabled: true,
            autosuggestions_menu_enabled: true,
            autosuggestions_menu_auto: false,
            autosuggestions_delay_ms: 0,
            autosuggestions_aliases: HashMap::new(),
            autosuggestions_specs_table: Cow::Borrowed(suggest::builtin_specs()),
            autosuggestions_generators_enabled: true,
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
    /// Whether the curated-specs provider joins the merge
    /// (flag-specs spec) — the resolved options flag, re-read at
    /// every `set_config`. The table itself is static universal
    /// knowledge; only the gate is state.
    autosuggestions_specs_enabled: bool,
    /// The user-declared alias map (suggest-aliases spec): the
    /// resolved `[autosuggestions.aliases]` table re-targeting the
    /// specs provider's command word, re-read at every `set_config`
    /// so a map change applies live at the next typed prefix.
    /// Config truth, never spawn truth — unlike the catalog
    /// snapshot it follows the config, not the pty's birth.
    autosuggestions_aliases: HashMap<String, String>,
    /// The specs provider's table (suggest-specs-data spec): the
    /// built-in five overlaid by `completions.toml`, re-read at
    /// every `set_config` so an edited or dropped file applies live
    /// at the next typed prefix — config truth beside the config,
    /// never spawn truth. `Cow::Borrowed` holds the built-in
    /// fallback without owning a copy of it.
    autosuggestions_specs_table: Cow<'static, [suggest::SpecCommand]>,
    /// Whether the completion menu computes (suggest-menu spec) —
    /// the resolved options flag, re-read at every `set_config`;
    /// an off flip closes a showing menu on the spot.
    autosuggestions_menu_enabled: bool,
    /// The auto-popup opt-in (suggest-menu spec) — the resolved
    /// options flag, re-read at every `set_config`.
    autosuggestions_menu_auto: bool,
    /// Whether the open menu's session is auto-managed
    /// (suggest-menu spec): an auto-opened menu closes itself when
    /// narrowing leaves one or nothing; a chord-opened one obeys
    /// only its own rules (the press, the lifecycle moments). Reset
    /// on every open, either kind.
    menu_auto_opened: bool,
    /// The idle-delay display gate (suggest-delay spec) — the
    /// resolved options value, re-read at every `set_config`: the
    /// observation carries it per update, so a live flip applies
    /// without a respawn.
    autosuggestions_delay_ms: u16,
    /// When the typed segment last changed (suggest-delay spec) —
    /// the idle clock the observation's `typed_idle_ms` resolves
    /// from. Reset before the step whenever this update's
    /// `before_cursor` differs from the last seen (a keystroke's
    /// own update reads as just-typed — instant dismiss) and after
    /// the step whenever the model adopted a fresh anchor (a new
    /// line is new typing). An unchanged line keeps its accumulated
    /// idle: the user's typing is the fact, the viewport is not.
    suggest_typed_at: Instant,
    /// The last update's bottom-row text — the change detector the
    /// idle clock resets on.
    suggest_last_before_cursor: String,
    /// The anchor as of the last update — the adoption detector
    /// (first adoption and every re-adoption both change it).
    suggest_last_anchor: Option<String>,
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
    /// The generator runner (suggest-generators spec): spawn,
    /// deadline, and stale-while-revalidate cache engine-side —
    /// dies with the terminal, so a respawn starts its cache empty.
    generator: GeneratorRunner,
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
    /// Output landed on this tab while the surface was hidden and no
    /// interaction has acknowledged it since (status-activity spec).
    /// Set by the active tab's wakeup while hidden; cleared by
    /// terminal touch or tab activation — never by the show itself,
    /// because a mark cleared on show is never seen. A `Cell`
    /// because the touch funnels ([`Self::input_scroll`],
    /// [`Self::paste`], [`Self::scroll`]) take `&self`. Dies with
    /// the terminal, so a respawn or session restore starts clean
    /// by construction.
    missed_activity: Cell<bool>,
    /// The shell's own cwd report (osc7-cwd spec) — the path seat's
    /// authoritative source, outranking the `/proc` cwd and the ssh
    /// stand-in while it stands. Written only by the event surface
    /// ([`Self::note_current_directory`]); a malformed report is
    /// ignored whole, a newer one replaces whatever stood. Dies with
    /// the terminal, so a respawn starts clean by construction.
    reported_cwd: Option<ReportedCwd>,
    /// The running command's own progress report (osc94-progress
    /// spec) — the OSC 9;4 state and last value, written only by
    /// the event surface ([`Self::note_progress_report`]) and
    /// cleared by state 0, the foreground handoff (`update()`'s
    /// finish edge), and [`PROGRESS_DECAY`] without a refresh. Dies
    /// with the terminal, so a respawn starts clean by
    /// construction.
    reported_progress: Option<ReportedProgress>,
    /// The program's own status report (OSC 7501, osc7501-status
    /// spec) — the root record alone, `app`/`title`/`msg`/`kind`
    /// ignored by design. Written only by the event surface
    /// ([`Self::note_program_status`]); cleared by the root
    /// `state=clear`, the foreground handoff for `working`/`blocked`
    /// alone ([`Terminal::update`]'s finish edge), a full reset
    /// ([`Self::note_terminal_reset`], RIS), and the terminal's own
    /// death. Dies with the terminal, so a respawn starts clean by
    /// construction.
    program_status: Option<ProgramStatus>,
    /// The inline-image session machine's open half (iterm-images
    /// spec): one `MultipartFile` per terminal (OQ-1 — imgcat never
    /// interleaves), a new header aborting whatever stood. `None` is
    /// idle. Dies with the terminal, so a respawn starts clean by
    /// construction.
    image_session: Option<PendingImage>,
    /// The inline-image id issuer (iterm-images spec): monotonic per
    /// terminal — ids outlive eviction, and a respawn's ids restart
    /// with the fresh store beside them.
    image_seq: u64,
    /// The decoded-image store (iterm-images spec): id → pixels,
    /// oldest-evicted under the budget. Shared to the widget through
    /// the Arc — the grid carries ids, this is the pixel side. Dies
    /// with the terminal, so a respawn starts it empty by
    /// construction.
    pub images: Arc<Mutex<ImageStore>>,
    /// Whether the inline-image pipeline runs (iterm-images spec) —
    /// the resolved `images_enabled` flag, re-read at every
    /// `set_config`; false makes OSC 1337 inert end to end (the arm
    /// never places).
    images_enabled: bool,
    /// The kitty graphics transmissions in flight (kitty-images
    /// spec), keyed by the first chunk's client `i` — `None` is the
    /// anonymous slot, one at a time; a new header replaces whatever
    /// stood under its key. Dies with the terminal, so a respawn
    /// starts clean by construction.
    kitty_chunks: HashMap<Option<u32>, KittyChunk>,
    /// The kitty client-id → store-id map (kitty-images spec): `i=`
    /// remaps on every retransmit; deletion and queries by `i`
    /// resolve through it. `I=` numbers are the store ids themselves —
    /// sequential from 1 by construction (OQ-1's default).
    kitty_ids: HashMap<u32, u64>,
    /// The settle-close's fresh-prompt latch (autosuggest spec): set
    /// when the spawn settle closes, held until an anchor is adopted
    /// or a child runs — the shell's first prompt can land after the
    /// settling update.
    settle_prompt_pending: bool,
    /// The autosuggest state (autosuggest spec) — engine-owned, per
    /// the status-glance lesson: it dies with the terminal, so a
    /// respawn (a fresh terminal) resets history by construction.
    suggest: Suggest,
    /// The completion menu's state (suggest-menu spec): the pure
    /// model, owned beside `suggest` so the collection, the scoped
    /// presses, and the accept funnel share one truth — and die with
    /// the terminal, like the ring.
    menu: suggest::MenuState,
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

// The generator contract (suggest-generators spec): the program
// list is this crate's own reviewed argv — data files never carry
// scripts, the record's permanent rejection. The env is not
// reproduced; it is reduced to two reproducible inputs: the
// spawn-time PATH's absolute binary and the tab's `/proc` cwd.
// Five seconds, then kill, then silence (Task 3) — a failed
// generator is DropTerm's problem, never the prompt's.

/// The branch generator's argv (VS Code's set, the record's
/// verbatim pinning): local heads, newest commit first, the short
/// name the template's first `|`-field carries.
pub(crate) const GIT_BRANCHES: &[&str] = &[
    "git",
    "--no-optional-locks",
    "for-each-ref",
    "--sort=-committerdate",
    "--format=%(refname:short)|%(objectname:short)|%(subject)",
    "refs/heads/",
];

/// The tag generator's argv: every tag, newest first.
pub(crate) const GIT_TAGS: &[&str] = &[
    "git",
    "--no-optional-locks",
    "tag",
    "--list",
    "--sort=-committerdate",
];

/// The working-tree files generator's argv: the porcelain short
/// status — staged, unstaged, and untracked alike.
pub(crate) const GIT_FILES: &[&str] = &["git", "--no-optional-locks", "status", "--short"];

/// One `for-each-ref --format` line's short ref name: the
/// template's first `|`-field (the subject may carry its own
/// pipes; the name never runs past the first).
pub(crate) fn branch_name(line: &str) -> Option<&str> {
    line.split('|').next()
}

/// One `tag --list` line is the whole name.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the Option is the post-filter's uniform extract signature — every set's carve answers the same question, None dropping its line"
)]
pub(crate) fn tag_name(line: &str) -> Option<&str> {
    Some(line)
}

/// One `status --short` line's changed path: past the porcelain
/// `XY ` prefix, a rename's new path after the ` -> ` arrow. Git
/// C-quotes paths carrying non-ASCII or quotes (core.quotePath's
/// default): the quoted form is not the name, and unquoting git's
/// escapes is a parser this contract refuses to ship — `None`
/// drops the line alone, never poisons the set.
pub(crate) fn status_path(line: &str) -> Option<&str> {
    let rest = line.get(3..)?;
    let path = match rest.rsplit_once(" -> ") {
        Some((_, new)) => new,
        None => rest,
    };
    if path.is_empty() || path.starts_with('"') {
        return None;
    }
    Some(path)
}

/// One generator output's post-filter (VS Code's discipline, the
/// record's pinning): `warning:`/`error:` lines are reality noise
/// and drop, any `fatal:` line means the command itself failed —
/// the whole set is silence — names dedupe through a `seen` set,
/// and each survivor rides `escape_for_shell` (an unwritable name
/// drops alone, never poisons the set). The result is the
/// already-escaped, deduped shape the model's cache holds.
pub(crate) fn post_filter_names(
    output: &str,
    extract: impl Fn(&str) -> Option<&str>,
) -> Vec<String> {
    if output.lines().any(|line| line.starts_with("fatal:")) {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for line in output.lines() {
        if line.starts_with("warning:") || line.starts_with("error:") {
            continue;
        }
        let Some(raw) = extract(line).filter(|raw| !raw.is_empty()) else {
            continue;
        };
        let Some(escaped) = suggest::escape_for_shell(raw) else {
            continue;
        };
        if seen.insert(escaped.clone()) {
            names.push(escaped);
        }
    }
    names
}

/// One generator spawn under the contract: the DropTerm-owned argv
/// run behind the absolutely-resolved binary with the tab's
/// `/proc` cwd and an env scrubbed to `HOME` (the spawn-time truth
/// the pty exec inherited — unset when the tab carried none) and
/// `LANG` — pinned to `C` so git's noise markers (`fatal:`,
/// `warning:`, `error:`) are the English bytes the post-filter
/// pins, whatever the user's desktop locale would have localized
/// them into. Nothing else inherits: no ambient PATH lookup at
/// exec, no `DropTerm` env leakage, no shell between `DropTerm`
/// and the argv.
fn generator_command(
    argv: &[&str],
    binary: &Path,
    cwd: &Path,
    home: Option<&str>,
) -> std::process::Command {
    let mut command = std::process::Command::new(binary);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .env_clear()
        .env("LANG", "C");
    if let Some(home) = home {
        command.env("HOME", home);
    }
    command
}

/// The generator binary: the first spawn-time `$PATH` directory
/// carrying an executable `git` — the catalog walk's own check
/// (regular file, execute bit, symlinks followed), resolved
/// against truth a mid-session PATH re-export cannot move. `None`
/// is silence: no git, no generators.
fn resolve_generator_binary(spawn_path: Option<&str>, program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let path = spawn_path?;
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(program);
        let Ok(meta) = fs::metadata(&candidate) else {
            continue;
        };
        if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
            return Some(candidate);
        }
    }
    None
}

/// The generator deadline (suggest-generators spec, ruled): five
/// seconds, then kill, then silence — a hung generator is never the
/// prompt's problem and never a user-visible error.
const GENERATOR_DEADLINE: Duration = Duration::from_secs(5);

/// One cache key: which DropTerm-owned argv, in which cwd — the
/// (argv, cwd) the record's stale-while-revalidate rules key on.
#[derive(Clone, PartialEq, Eq, Hash)]
struct GenKey {
    argv: &'static [&'static str],
    cwd: PathBuf,
}

/// The engine-side mirror of the model's sets: `Arc` handles out of
/// the shared cache, cheap to hand the observation without holding
/// the lock.
struct GenCached {
    branches: Arc<[String]>,
    tags: Arc<[String]>,
    files: Arc<[String]>,
}

/// The runner's thread-shared state: the cache, the in-flight
/// revalidations, and the spawn counter — the no-spawn-on-typing
/// contract's own bookkeeping, test-read.
#[derive(Default)]
struct GenShared {
    cache: HashMap<GenKey, Arc<[String]>>,
    in_flight: HashSet<GenKey>,
    spawns: u64,
}

/// The generator runner (suggest-generators spec): engine-side
/// spawn, deadline, and stale-while-revalidate cache for the
/// model's generator sets. The cache serves whatever it holds the
/// instant an update asks — stale included — and revalidation
/// happens only on the ruled triggers (the menu's open moment and
/// the eligibility onset at spawn), never on the damage-driven
/// update path. Every failure is cached silence: a missing binary,
/// a non-repo cwd, a failing command, a hung command — all degrade
/// to the empty set (no TTL, no wall clock; a re-trigger refreshes,
/// serve-forever-until-revalidation being the ruled horizon).
struct GeneratorRunner {
    /// The config gate (suggest-generators spec): the resolved
    /// `autosuggestions_generators_enabled`, re-read at every
    /// `set_config` — `false` stops every spawn and blanks the
    /// cache read (a warm cache never serves), `true` re-arms the
    /// ruled triggers. Config truth, never spawn truth.
    enabled: bool,
    /// The standing ssh gate, spawn truth: the tab's program is an
    /// allowlisted shell. An ineligible tab never spawns.
    eligible: bool,
    /// The spawn-time-resolved absolute git binary; `None` silences
    /// every spawn (no git on the tab's PATH).
    binary: Option<PathBuf>,
    /// The spawn-time HOME — the scrubbed env's one optional input.
    home: Option<String>,
    /// The wakeup channel: a landed fetch sends one `Wakeup` so the
    /// app drives the update that serves the fresh cache — without
    /// it, a landed fetch would wait on the user's next keystroke.
    wake: Option<(TabId, mpsc::UnboundedSender<(TabId, Event)>)>,
    /// The shared cache + in-flight set, written through by the
    /// worker threads and read by every update.
    shared: Arc<Mutex<GenShared>>,
}

impl GeneratorRunner {
    /// The runner for a fresh terminal: spawn truth only — the
    /// shell gate, the resolved binary, the spawn HOME, and the
    /// wakeup channel — plus the resolved config gate. The cache
    /// starts empty; nothing spawns until a trigger asks.
    fn new(
        spawned_program: &str,
        spawn_path: Option<&str>,
        spawn_home: Option<&str>,
        enabled: bool,
        tab_id: TabId,
        event_tx: mpsc::UnboundedSender<(TabId, Event)>,
    ) -> Self {
        Self {
            enabled,
            eligible: suggest::is_allowlisted_shell(spawned_program),
            binary: resolve_generator_binary(spawn_path, "git"),
            home: spawn_home.map(str::to_owned),
            wake: Some((tab_id, event_tx)),
            shared: Arc::new(Mutex::new(GenShared::default())),
        }
    }

    /// Locks the shared state, poison-recovered — a panicked worker
    /// must not take the tab's suggestions with it.
    fn lock(&self) -> std::sync::MutexGuard<'_, GenShared> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The cached sets for `cwd` — the observation's spawn-free
    /// read. `None` until the runner has cached anything for this
    /// cwd (no generator context), on an ineligible tab, with no
    /// resolvable git, or under a live `false` flip (a warm cache
    /// never serves); once any set is cached the three arrive
    /// whole, a set the cache lacks reading empty — an honest
    /// silence, not a missing context.
    fn sets_for(&self, cwd: Option<&Path>) -> Option<GenCached> {
        if !self.enabled || !self.eligible || self.binary.is_none() {
            return None;
        }
        let cwd = cwd?;
        let key = |argv: &'static [&'static str]| GenKey {
            argv,
            cwd: cwd.to_path_buf(),
        };
        let state = self.lock();
        let branches = state.cache.get(&key(GIT_BRANCHES)).cloned();
        let tags = state.cache.get(&key(GIT_TAGS)).cloned();
        let files = state.cache.get(&key(GIT_FILES)).cloned();
        if branches.is_none() && tags.is_none() && files.is_none() {
            return None;
        }
        let empty = || Arc::from(Vec::new());
        Some(GenCached {
            branches: branches.unwrap_or_else(empty),
            tags: tags.unwrap_or_else(empty),
            files: files.unwrap_or_else(empty),
        })
    }

    /// The revalidation trigger (suggest-generators spec): stale
    /// keeps serving while a worker per argv revalidates — skipped
    /// where one is already in flight (the SWR dedup). Menu-open
    /// and the eligibility onset call this; the damage-driven
    /// update path never does. A live `false` flip blocks the
    /// trigger with everything else.
    fn revalidate(&self, cwd: Option<&Path>) {
        if !self.enabled || !self.eligible || self.binary.is_none() {
            return;
        }
        let Some(cwd) = cwd else {
            return;
        };
        for argv in [GIT_BRANCHES, GIT_TAGS, GIT_FILES] {
            let key = GenKey {
                argv,
                cwd: cwd.to_path_buf(),
            };
            let mut state = self.lock();
            if !state.in_flight.insert(key.clone()) {
                continue;
            }
            state.spawns += 1;
            drop(state);
            self.spawn_worker(key);
        }
    }

    /// One background revalidation: run the argv under the
    /// deadline, post-filter whatever came back (silence on any
    /// failure), cache it, retire the in-flight mark, and wake the
    /// app — the fresh set serves at the next update.
    fn spawn_worker(&self, key: GenKey) {
        let Some(binary) = self.binary.clone() else {
            return;
        };
        let command = generator_command(key.argv, &binary, &key.cwd, self.home.as_deref());
        let shared = Arc::clone(&self.shared);
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let names = run_under_deadline(command, GENERATOR_DEADLINE)
                .map(|output| post_filter_names(&output, generator_extract(key.argv)))
                .unwrap_or_default();
            let mut state = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.cache.insert(key.clone(), Arc::from(names));
            state.in_flight.remove(&key);
            drop(state);
            // The landed-fetch wakeup: one nudge so the app drives
            // the update that serves the fresh cache (a dying tab's
            // closed channel drops it silently).
            if let Some((tab_id, tx)) = wake {
                let _ = tx.send((tab_id, Event::Wakeup));
            }
        });
    }

    /// The shared state, for the engine pins — test-only reads of
    /// the spawn counter and the cache bookkeeping.
    #[cfg(test)]
    fn lock_generator_state(&self) -> std::sync::MutexGuard<'_, GenShared> {
        self.lock()
    }
}

/// Which post-filter carve an argv's output takes: the branch
/// template's `|`-field, the bare tag name, or the porcelain path.
fn generator_extract(argv: &[&str]) -> fn(&str) -> Option<&str> {
    if argv == GIT_BRANCHES {
        branch_name
    } else if argv == GIT_TAGS {
        tag_name
    } else {
        status_path
    }
}

/// One spawn under the deadline (suggest-generators spec): a worker
/// thread drains the child's stdout and waits it while the caller
/// holds the `recv_timeout`; expiry SIGKILLs the child and discards
/// everything. Every failure is `None` — a missing binary, a spawn
/// error, a non-zero exit, a timeout, a non-UTF-8 pipe — never an
/// error surface.
fn run_under_deadline(mut command: std::process::Command, deadline: Duration) -> Option<String> {
    use std::io::Read;

    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().ok()?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        // Drain before waiting: a chatty child's full pipe would
        // otherwise deadlock the wait itself.
        let mut stdout = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            let _ = pipe.read_to_string(&mut stdout);
        }
        let status = child.wait();
        let _ = tx.send((stdout, status.ok()));
    });
    match rx.recv_timeout(deadline) {
        Ok((stdout, Some(status))) if status.success() => Some(stdout),
        _ => {
            // The hard timeout: kill, reap, discard — silence.
            // SAFETY: libc::kill signals a pid this process owns.
            // The waiter sends only after wait() reaps the child, so
            // a timed-out recv means the child was unreaped when the
            // window opened; the microsecond gap between a reaping
            // wait() and its send would additionally require
            // immediate pid reuse, which Linux's sequential pid
            // allocation makes effectively unreachable.
            unsafe { libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGKILL) };
            let _ = waiter.join();
            None
        }
    }
}

/// The history seed's one-time read (history-seed spec): the shell's
/// own persistent history file under the spawn-time HOME — the
/// per-shell default location, read once at spawn and never written,
/// any IO failure degrading to an empty seed (silence, never a
/// guess). The allowlist is implicit in the file mapping: a program
/// with no standard history file seeds nothing.
fn shell_history_seed(program: &str, home: Option<&str>) -> Vec<String> {
    let Some(file) = suggest::shell_history_file(program) else {
        return Vec::new();
    };
    let Some(home) = home else {
        return Vec::new();
    };
    fs::read_to_string(Path::new(home).join(file))
        .map(|contents| suggest::parse_shell_history(program, &contents))
        .unwrap_or_default()
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
        // The generator runner (suggest-generators spec): spawn
        // truth only — the shell gate, the spawn-PATH git binary,
        // the spawn HOME — plus the resolved config gate and the
        // wakeup channel the landed fetches nudge (the clone; the
        // proxy owns the original).
        let generator = GeneratorRunner::new(
            &options.spawned_program,
            options.spawn_path.as_deref(),
            options.spawn_home.as_deref(),
            options.autosuggestions_generators_enabled,
            tab_id,
            event_tx.clone(),
        );
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

        let terminal = Self {
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
            images_enabled: options.images_enabled,
            autosuggestions_paths_enabled: options.autosuggestions_paths_enabled,
            autosuggestions_commands_enabled: options.autosuggestions_commands_enabled,
            autosuggestions_specs_enabled: options.autosuggestions_specs_enabled,
            autosuggestions_aliases: options.autosuggestions_aliases.clone(),
            autosuggestions_specs_table: options.autosuggestions_specs_table.clone(),
            autosuggestions_menu_enabled: options.autosuggestions_menu_enabled,
            autosuggestions_menu_auto: options.autosuggestions_menu_auto,
            menu_auto_opened: false,
            autosuggestions_delay_ms: options.autosuggestions_delay_ms,
            suggest_typed_at: Instant::now(),
            suggest_last_before_cursor: String::new(),
            suggest_last_anchor: None,
            spawned_program: options.spawned_program.clone(),
            spawn_home: options.spawn_home.clone(),
            command_catalog: command_catalog_from_path(options.spawn_path.as_deref()),
            path_cache: PathListingCache::default(),
            generator,
            ghost_color: suggest::ghost_color(colors),
            last_foreground_child: false,
            reported_progress: None,
            program_status: None,
            image_session: None,
            image_seq: 0,
            images: Arc::new(Mutex::new(ImageStore::default())),
            kitty_chunks: HashMap::new(),
            kitty_ids: HashMap::new(),
            timer: CommandTimer::default(),
            command_finished: None,
            missed_activity: Cell::new(false),
            reported_cwd: None,
            settle_prompt_pending: false,
            suggest: {
                // The history seed (history-seed spec): read once at
                // spawn, only on a tab whose autosuggestions are on —
                // a disabled feature never touches the file — and
                // only via the shell-gated default location.
                let mut suggest = Suggest::new();
                if options.autosuggestions_enabled {
                    suggest.seed_history(shell_history_seed(
                        &options.spawned_program,
                        options.spawn_home.as_deref(),
                    ));
                }
                suggest
            },
            menu: suggest::MenuState::new(),
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
        };
        // The generators' eligibility onset (suggest-generators
        // spec): an eligible tab's first revalidation pre-warms the
        // cache at the spawn cwd, so the first Ctrl+Space serves
        // real rows — background workers under the deadline, never
        // blocking construction, never per keystroke.
        terminal
            .generator
            .revalidate(terminal.working_directory().as_deref());
        Ok(terminal)
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

    /// Closes the spawn-grid settle at the grid the terminal already
    /// holds — records the resting grid, freezes the record, and
    /// marks the settle's close a fresh-prompt moment (autosuggest
    /// spec) so the next update may adopt the shell's first prompt.
    /// For the adoption-after-apply shape the pin funnel runs: the
    /// granted size applies first (the draw resized before the
    /// granted-size echo arrived), and `resize` no-ops on unchanged
    /// sizes — so an arm at that point would wait for a changed
    /// resize that never comes, the settle would never close, and
    /// the fresh-prompt latch would never arm (the 2026-10-24
    /// fresh-start trace: no tab ever adopted its anchor). A no-op
    /// on settled records, so a re-show's adoption can never
    /// re-snapshot a frozen record.
    pub fn settle_spawn_grid_now(&mut self) {
        if matches!(self.spawn_settle, SpawnSettle::Settled) {
            return;
        }
        self.spawn_grid = self.grid();
        self.spawn_settle = SpawnSettle::Settled;
        // The settle's close is a fresh-prompt moment (autosuggest
        // spec): the update that follows may adopt the shell's
        // first prompt.
        self.settle_prompt_pending = true;
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
            // The completion menu closes on focus loss
            // (suggest-menu spec): an unfocused surface must not
            // hold an interaction overlay over a grid it is not
            // attending to.
            if !is_focused {
                self.menu.close();
            }
            // The caret rides the buffer build — the cursor cell's
            // fg/bg swap keys on this flag — so a real focus change
            // must arm a rebuild, or the swap only lands when
            // unrelated pty output next rebuilds the lines and a
            // re-shown surface paints no caret at all until then
            // (a non-focus-reporting shell like bash never emits
            // any).
            self.needs_update = true;

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

    /// Records the shell's own verdict for the finished command
    /// (status-exit spec): the OSC 133;D report forwarded from the
    /// event surface, held by the command timer on the frozen pair's
    /// lifecycle — cleared by the next witnessed start, never guessed.
    pub fn note_prompt_exit(&mut self, code: u32) {
        self.timer.note_exit(code);
    }

    /// The last finished command's exit code (status-exit spec): the
    /// shell's own 133;D report — success and absent facts both read
    /// `None` — served until the next command starts.
    pub fn command_exit(&self) -> Option<u32> {
        self.timer.last_exit()
    }

    /// Records the shell's own cwd report (osc7-cwd spec): the OSC 7
    /// payload forwarded from the event surface. A malformed report
    /// is ignored whole — it never disturbs a standing one; a valid
    /// report replaces whatever stood.
    pub fn note_current_directory(&mut self, url: &str) {
        if let Some(report) = ReportedCwd::parse(url) {
            self.reported_cwd = Some(report);
        }
    }

    /// The shell's own cwd report (osc7-cwd spec), served until the
    /// next report or the terminal's death — the path seat's
    /// authoritative source, never a guess.
    pub fn reported_cwd(&self) -> Option<ReportedCwd> {
        self.reported_cwd.clone()
    }

    /// Records the running command's own progress report
    /// (osc94-progress spec): the OSC 9;4 payload forwarded from
    /// the event surface. State 0 clears; a report that omits its
    /// value keeps the standing one; every report re-anchors the
    /// decay clock.
    pub fn note_progress_report(&mut self, state: u8, value: Option<u8>) {
        self.reported_progress =
            ReportedProgress::apply(self.reported_progress, state, value, Instant::now());
    }

    /// The standing progress report (osc94-progress spec), until
    /// state 0, the foreground handoff, or the decay window clears
    /// it — never a guess.
    pub fn reported_progress(&self) -> Option<ReportedProgress> {
        self.reported_progress
    }

    /// The decay deadline of the standing progress report
    /// (osc94-progress spec): the instant it expires without a
    /// refresh — `None` when nothing stands. The app sleeps until
    /// this instant and then expires, so a dead reporter's last
    /// number never outlives its window.
    pub fn progress_decay_deadline(&self) -> Option<Instant> {
        self.reported_progress
            .map(|report| report.refreshed_at + PROGRESS_DECAY)
    }

    /// Expires the standing progress report if its decay window
    /// has passed (osc94-progress spec); a no-op before the
    /// deadline — an early fire caught a refreshed anchor.
    pub fn expire_progress(&mut self) {
        if self
            .reported_progress
            .is_some_and(|report| report.expired(Instant::now()))
        {
            self.reported_progress = None;
        }
    }

    /// Records the program's own status report (osc7501-status
    /// spec): the OSC 7501 body forwarded from the event surface.
    /// The protocol's grammar holds — the root record alone stands
    /// (id'd reports ignored), the root `state=clear` removes, each
    /// report replacing its record completely — and a mangled body
    /// never disturbs a standing one.
    pub fn note_program_status(&mut self, body: &str) {
        self.program_status = ProgramStatus::apply(self.program_status, body);
    }

    /// The standing program status report (osc7501-status spec),
    /// until the root `clear`, the foreground handoff (for
    /// `working`/`blocked` alone), a full reset, or the terminal's
    /// own death — never a guess.
    pub fn program_status(&self) -> Option<ProgramStatus> {
        self.program_status
    }

    /// The terminal was fully reset (osc7501-status spec): RIS
    /// removes every record — the fork's `FullReset` event's seat,
    /// clearing even the persistent `done`/`error`.
    pub fn note_terminal_reset(&mut self) {
        self.program_status = None;
    }

    /// Records one OSC 1337 arrival (iterm-images spec): the fork's
    /// raw parameter slices, forwarded from the event surface. The
    /// session machine runs here — the legacy `File=` single-shot
    /// decodes and places at once; the chunked session accumulates
    /// `FilePart`s under one open `MultipartFile` header until
    /// `FileEnd` (a new header aborts whatever stood; a part or end
    /// without a session is ignored). A kill-switched pipeline, a
    /// malformed payload, or a blown cap drops whole — nothing
    /// half-places, nothing logs.
    pub fn note_inline_image(&mut self, params: &[Vec<u8>]) {
        if !self.images_enabled {
            return;
        }
        let Some(first) = params.first() else { return };

        if first.starts_with(b"File=") {
            // Legacy single-shot: args and payload in one sequence —
            // the payload still base64 on the wire, decoded here, a
            // malformed one dropped whole.
            let (args, encoded) = parse_image_args(params);
            if let Some(encoded) = encoded
                && let Some(payload) = decode_base64(&encoded)
            {
                self.place_inline_image(args, &payload);
            }
        } else if first.starts_with(b"MultipartFile=") {
            // A second header aborts the first by replacement (the
            // first session's bytes are simply dropped).
            let (args, _) = parse_image_args(params);
            self.image_session = Some(PendingImage {
                args,
                payload: Vec::new(),
            });
        } else if let Some(chunk) = first.strip_prefix(b"FilePart=") {
            if let Some(session) = self.image_session.as_mut() {
                match decode_base64(chunk) {
                    Some(bytes) => {
                        session.payload.extend_from_slice(&bytes);
                        if session.payload.len() > IMAGE_PAYLOAD_CAP {
                            // The cap drops the session whole, never a
                            // half-buffered image.
                            self.image_session = None;
                        }
                    }
                    None => self.image_session = None,
                }
            }
        } else if first == b"FileEnd"
            && let Some(session) = self.image_session.take()
        {
            self.place_inline_image(session.args, &session.payload);
        }
        // Anything else — a bare or unknown opcode — is silence.
    }

    /// Decode one completed payload and place it (iterm-images
    /// spec): `inline=0` renders nothing (no download UI — a map-gate
    /// never), the caps drop whole before any decode, the decoded
    /// pixels enter the store under a fresh id, and the grid's cells
    /// are diced through the fork's `Term::place_image` — the
    /// placement rides the grid like text from there.
    fn place_inline_image(&mut self, args: ImageArgs, payload: &[u8]) {
        if !args.inline || payload.is_empty() || payload.len() > IMAGE_PAYLOAD_CAP {
            return;
        }
        let Some(rgba) = decode_image(payload) else {
            return;
        };
        let (width, height) = rgba.dimensions();
        #[expect(
            clippy::cast_precision_loss,
            reason = "decode-capped dimensions (8192) are exact in f32"
        )]
        let inherent = (width as f32, height as f32);
        self.image_seq += 1;
        let id = self.image_seq;
        self.images.lock().unwrap().insert(id, rgba);
        let (cols, rows) = resolve_placement(&args, inherent, &self.size);
        self.term.lock().place_image(id, cols, rows);
        self.needs_update = true;
    }

    /// One APC payload from the fork (kitty-images spec): the leading
    /// `G` gates the protocol — SOS/PM strings share the fork's APC
    /// state and arrive here too — then the control data parses under
    /// the subset's grammar and the transmission machine runs: chunks
    /// assemble keyed by the first chunk's `i` until a final chunk
    /// commits through decode and (for `a=T`) placement. A
    /// kill-switched pipeline, a forbidden medium, or malformed
    /// control data drops whole; the forbidden media answer the one
    /// uniform error on the PTY write path (`q=2` silences it).
    pub fn note_apc_payload(&mut self, payload: &[u8]) {
        if !self.images_enabled {
            return;
        }
        let Some(body) = payload.strip_prefix(b"G") else {
            return;
        };
        let (control, encoded) = match body.iter().position(|&byte| byte == b';') {
            Some(sep) => (&body[..sep], Some(&body[sep + 1..])),
            None => (body, None),
        };
        let Some(keys) = parse_kitty_keys(control) else {
            return;
        };

        // The medium is direct in-band or the sequence is refused —
        // file, temp-file, and shared-memory transmission never cross
        // the seam, and every refusal answers identically so nothing
        // about the filesystem is probeable.
        if keys.transmission.is_some_and(|medium| medium != b'd') {
            if keys.quiet < 2 {
                self.input_no_scroll(kitty_error_reply(keys.image_id));
            }
            return;
        }

        // The command arms (query, delete, place) are standalone —
        // never chunk payloads.
        if matches!(keys.action, Some(b'q' | b'd' | b'p')) {
            return;
        }

        let mut bytes = Vec::new();
        if let Some(encoded) = encoded {
            // A malformed base64 slice drops the sequence whole,
            // never half-carried.
            match decode_base64(encoded) {
                Some(decoded) => bytes = decoded,
                None => return,
            }
        }

        if keys.opens() {
            // The first chunk carries the control keys; a new header
            // replaces whatever stood under its key.
            if keys.more {
                if bytes.len() <= IMAGE_PAYLOAD_CAP {
                    self.kitty_chunks.insert(
                        keys.image_id,
                        KittyChunk {
                            keys,
                            payload: bytes,
                        },
                    );
                }
            } else {
                self.kitty_commit(keys, &bytes);
            }
        } else if let Some(session) = self.kitty_chunks.get_mut(&keys.image_id) {
            // A continuation joins its key's session — the anonymous
            // slot when no `i` rides the wire.
            session.payload.extend_from_slice(&bytes);
            if session.payload.len() > IMAGE_PAYLOAD_CAP {
                // The cap drops the session whole, never a
                // half-buffered image.
                self.kitty_chunks.remove(&keys.image_id);
            } else if !keys.more
                && let Some(finished) = self.kitty_chunks.remove(&keys.image_id)
            {
                self.kitty_commit(finished.keys, &finished.payload);
            }
        }
        // A continuation without a session is silence.
    }

    /// Decode and seat one completed kitty transmission (kitty-images
    /// spec): the iterm module's own ceilings drop whole before any
    /// decode, the pixels enter the shared store under a fresh id
    /// (the client `i`, when sent, maps to it), `x`/`y` crop the
    /// displayed region to its far edges, and `a=T` alone places —
    /// `a=t` and the default transmit-only store.
    fn kitty_commit(&mut self, keys: KittyKeys, payload: &[u8]) {
        if payload.is_empty() || payload.len() > IMAGE_PAYLOAD_CAP {
            return;
        }
        let Some(mut rgba) = decode_kitty_payload(&keys, payload) else {
            return;
        };

        // The displayed region's source offset: a crop extending to
        // the image's far edges, so only what shows is seated.
        if let (Some(x), Some(y)) = (keys.offset_x, keys.offset_y) {
            let (width, height) = rgba.dimensions();
            if x >= width || y >= height {
                return;
            }
            rgba = image::DynamicImage::ImageRgba8(rgba)
                .crop_imm(x, y, width - x, height - y)
                .to_rgba8();
        }

        self.image_seq += 1;
        let id = self.image_seq;
        if let Some(client) = keys.image_id {
            self.kitty_ids.insert(client, id);
        }
        let dims = rgba.dimensions();
        self.images.lock().unwrap().insert(id, rgba);
        if keys.action == Some(b'T') {
            let (cols, rows) = kitty_cells(&keys, dims, &self.size);
            self.term.lock().place_image(id, cols, rows);
        }
        self.needs_update = true;
    }

    /// Drains the finish latch once (command-lifecycle spec): the
    /// frozen duration of the most recently finished command,
    /// returned exactly once per finish — `command-notify`'s seam.
    /// The band's frozen display reads [`Self::command_frozen`] and
    /// is unaffected by the drain.
    pub fn take_command_finished(&mut self) -> Option<Duration> {
        self.command_finished.take()
    }

    /// Arms the missed-activity latch (status-activity spec): output
    /// landed on this tab while the surface was hidden. Called by
    /// the app's Wakeup handler for the active tab, gated on the
    /// hidden surface — never by anything inside the engine, whose
    /// own output writes must not arm their own mark.
    pub fn note_missed_activity(&mut self) {
        self.missed_activity.set(true);
    }

    /// Whether output landed while the surface was hidden and no
    /// interaction has acknowledged it since (status-activity spec)
    /// — the band's attention halo's fact. A pure read: the band
    /// samples it every refresh without draining it.
    pub fn missed_activity(&self) -> bool {
        self.missed_activity.get()
    }

    /// Clears the missed-activity latch — the acknowledgment itself
    /// (status-activity spec). The shared head of the three user
    /// touch funnels ([`Self::input_scroll`], [`Self::paste`],
    /// [`Self::scroll`]) and the widget's pane-press site: any input
    /// routed into the terminal pane is the user saying "seen".
    /// Internal engine writes ride [`Self::input_no_scroll`]
    /// directly, so they never masquerade as touches.
    pub(crate) fn clear_missed_activity(&self) {
        self.missed_activity.set(false);
    }

    pub fn input_scroll<I: Into<Cow<'static, [u8]>>>(&self, input: I) {
        self.clear_missed_activity();
        self.input_no_scroll(input);
        self.scroll(TerminalScroll::Bottom);
    }

    pub fn paste(&self, value: &str) {
        self.clear_missed_activity();
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

    /// The showing suggestion's visible string (autosuggest +
    /// ghost-fit specs) — the clipped display the widget shapes;
    /// `None` renders nothing, and an empty visible (below the
    /// three-column draw threshold) renders nothing while the showing
    /// still accepts. The accept path carries the full suffix.
    pub fn suggestion(&self) -> Option<&str> {
        self.suggest
            .showing()
            .map(|showing| showing.visible.as_str())
    }

    /// The completion menu's open attempt (suggest-menu spec, the
    /// soft trigger): the ghost's own eligibility chain minus its
    /// single-choice display gates, a fresh observation, and the
    /// merged providers' full candidate sets. `false` is the
    /// decline — ineligible, or no candidate answered — and the
    /// caller forwards the chord to the shell verbatim.
    pub fn menu_open(&mut self) -> bool {
        self.menu_auto_opened = false;
        // The generators' ruled trigger (suggest-generators spec):
        // the menu's open moment revalidates the tab's cwd — stale
        // serves in the rows this open collects, the fresh sets
        // landing for the updates that follow.
        self.generator
            .revalidate(self.working_directory().as_deref());
        let rows = self.menu_candidates();
        self.menu.open(rows)
    }

    /// Whether the completion menu shows.
    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// The menu's displayed rows (suggest-menu spec) — the
    /// renderer's input, in display order.
    pub fn menu_rows(&self) -> &[suggest::MenuCandidate] {
        self.menu.rows()
    }

    /// The menu's selected row index (suggest-menu spec) — the
    /// renderer's highlight.
    pub fn menu_selected(&self) -> usize {
        self.menu.selected()
    }

    /// The menu's scroll offset (suggest-menu spec) — the renderer's
    /// visible window starts here.
    pub fn menu_scroll(&self) -> usize {
        self.menu.scroll()
    }

    /// Closes the menu: Escape, the chord's toggle, any press the
    /// menu does not claim, and the lifecycle moments (resize,
    /// focus loss, scroll-away) the renderer's contract names.
    pub fn menu_close(&mut self) {
        self.menu.close();
    }

    /// One menu-scoped press (suggest-menu spec): the pure
    /// transition, with an accept's suffix written straight through
    /// the one funnel — typed characters via `input_no_scroll`,
    /// never a paste.
    pub fn menu_press(&mut self, key: suggest::MenuKey) {
        if let Some(suffix) = self.menu.press(key)
            && !suffix.is_empty()
        {
            self.input_no_scroll(suffix.into_bytes());
        }
    }

    /// The menu's candidate collection (suggest-menu spec): a fresh
    /// observation under the ghost's eligibility chain minus its
    /// single-choice display gates — the same reduced facts
    /// `update()` builds each damage cycle, mirrored here as a
    /// second cursor-line scrape at key-press frequency (never per
    /// frame) so the ghost's own path stays untouched. Ineligible is
    /// the empty set: the soft trigger's decline.
    fn menu_candidates(&mut self) -> Vec<suggest::MenuCandidate> {
        if !self.autosuggestions_menu_enabled || !self.autosuggestions_enabled || !self.is_focused {
            return Vec::new();
        }
        if self.foreground_child_running() {
            return Vec::new();
        }
        let (before_cursor, after_cursor_blank, display_offset, columns, cursor_column) = {
            let term = self.term.lock();
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
                        && !cell
                            .flags
                            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                        && cell.zerowidth().is_none();
                    if !blank {
                        after_cursor_blank = false;
                    }
                }
            }
            (
                before_cursor,
                after_cursor_blank,
                grid.display_offset(),
                columns,
                cursor.column.0,
            )
        };
        if display_offset != 0 {
            return Vec::new();
        }
        // The anchor carve doubles as two of the ghost's guards:
        // `None` is unanchored or a line that left the anchor.
        let Some(typed) = self.suggest.typed_segment(&before_cursor) else {
            return Vec::new();
        };
        if typed.is_empty() {
            return Vec::new();
        }
        let paths_eligible = self.paths_eligible();
        let working_directory = self.working_directory();
        // The generator provider's cached sets (suggest-generators
        // spec), served spawn-free beside the cwd truth: the fetch
        // happens only on the ruled triggers, never on this path.
        let generator_sets = self.generator.sets_for(working_directory.as_deref());
        let path_listing = self.path_cache.listing_for(
            self.autosuggestions_paths_enabled && paths_eligible,
            suggest::path_token(Some(typed)),
            self.spawn_home.as_deref(),
            working_directory.as_deref(),
        );
        let observation = suggest::Observation {
            enabled: self.autosuggestions_enabled,
            paths_eligible,
            paths_enabled: self.autosuggestions_paths_enabled,
            path_listing,
            commands_enabled: self.autosuggestions_commands_enabled,
            specs_enabled: self.autosuggestions_specs_enabled,
            aliases: &self.autosuggestions_aliases,
            specs_table: &self.autosuggestions_specs_table,
            command_catalog: &self.command_catalog,
            generator_candidates: generator_sets.as_ref().map(|sets| suggest::GeneratorSets {
                branches: &sets.branches,
                tags: &sets.tags,
                files: &sets.files,
            }),
            focused: self.is_focused,
            foreground_child: false,
            display_offset: 0,
            cursor_on_bottom_row: false,
            columns,
            cursor_column,
            before_cursor: &before_cursor,
            after_cursor_blank,
            fresh_prompt: false,
            delay_ms: self.autosuggestions_delay_ms,
            typed_idle_ms: 0,
        };
        suggest::candidates(typed, &self.suggest, &observation)
    }

    /// The active reveal deadline (suggest-delay spec): the instant
    /// a withheld suggestion may show — the typed segment's last
    /// change plus the resolved delay. `None` when nothing waits (no
    /// qualifying candidate is withheld) or the delay is zero
    /// (nothing is ever withheld). The app sleeps until this instant
    /// and then drives one update, so the ghost appears at the pause
    /// with no new keystroke.
    pub fn suggest_reveal_deadline(&self) -> Option<Instant> {
        if !self.suggest.withheld() || self.autosuggestions_delay_ms == 0 {
            return None;
        }
        Some(
            self.suggest_typed_at + Duration::from_millis(u64::from(self.autosuggestions_delay_ms)),
        )
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

    /// The scheme's foreground as a cosmic-text color
    /// (suggest-menu spec): the completion menu's row text — the
    /// live terminal scheme's own fg, the same conversion the
    /// default attrs ride, never a palette index.
    pub fn foreground_color(&self) -> cosmic_text::Color {
        convert_color(&self.colors, Color::Named(NamedColor::Foreground))
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width != self.size.width || height != self.size.height {
            let instant = Instant::now();

            // The completion menu closes on resize (suggest-menu
            // spec): the panel is anchored to the cursor's cell, and
            // a reflow moves that anchor under a panel sized for the
            // old geometry.
            self.menu.close();

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
        self.clear_missed_activity();
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

        if self.images_enabled != options.images_enabled {
            // The images kill switch (iterm-images spec): an off flip
            // makes the 1337 arm inert at the next arrival — nothing
            // decodes, nothing places; an on flip renders from the
            // next sequence. Placed cells and stored pixels stand
            // either way (the grid is the bookkeeping, not the
            // toggle).
            self.images_enabled = options.images_enabled;
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

        if self.autosuggestions_specs_enabled != options.autosuggestions_specs_enabled {
            self.autosuggestions_specs_enabled = options.autosuggestions_specs_enabled;
            update = true;
        }

        if self.autosuggestions_menu_enabled != options.autosuggestions_menu_enabled {
            self.autosuggestions_menu_enabled = options.autosuggestions_menu_enabled;
            // The live close (suggest-menu spec): an off flip spends
            // a showing menu the moment the switch lands; an on flip
            // only arms the chord (opens stay press-driven).
            self.menu.close();
            update = true;
        }

        if self.autosuggestions_menu_auto != options.autosuggestions_menu_auto {
            self.autosuggestions_menu_auto = options.autosuggestions_menu_auto;
            update = true;
        }

        if self.autosuggestions_delay_ms != options.autosuggestions_delay_ms {
            self.autosuggestions_delay_ms = options.autosuggestions_delay_ms;
            update = true;
        }

        if self.autosuggestions_aliases != options.autosuggestions_aliases {
            // The alias map's live flip (suggest-aliases spec): one
            // whole-map swap — a declared, edited, or removed alias
            // applies at the next typed prefix, the next observation
            // borrowing the fresh table.
            self.autosuggestions_aliases
                .clone_from(&options.autosuggestions_aliases);
            update = true;
        }

        if self.autosuggestions_specs_table != options.autosuggestions_specs_table {
            // The spec table's live flip (suggest-specs-data spec):
            // one whole-table swap — an edited, extended, or removed
            // `completions.toml` applies at the next typed prefix,
            // the next observation borrowing the fresh rows.
            // `Cow::clone_from` keeps the borrowed fallback a
            // pointer swap, never a table copy.
            self.autosuggestions_specs_table
                .clone_from(&options.autosuggestions_specs_table);
            update = true;
        }

        if self.generator.enabled != options.autosuggestions_generators_enabled {
            // The generators' live flip (suggest-generators spec):
            // `false` stops every spawn and blanks the cache read on
            // this very pass (a warm cache never serves the stale
            // rows); `true` re-arms the ruled triggers — the next
            // menu-open revalidates.
            self.generator.enabled = options.autosuggestions_generators_enabled;
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
        // The generator provider's cached sets (suggest-generators
        // spec), served spawn-free beside the cwd truth — the
        // stale-while-revalidate serving half. The fetch itself
        // happens only on the ruled triggers (menu-open, the
        // eligibility onset), never on this damage-driven path.
        let generator_sets = self.generator.sets_for(working_directory.as_deref());
        let fresh_by_finish = self.last_foreground_child && !foreground_child;
        self.last_foreground_child = foreground_child;
        // The command timer rides the engine's own diff
        // (command-lifecycle spec) — timestamps always run, one
        // `Option` write per update; never suggest's, which lives
        // behind the autosuggest toggle. The finish edge arms the
        // notify latch with the just-frozen duration (unwitnessed
        // starts freeze nothing, so they latch nothing either).
        self.timer.step(foreground_child, instant);
        if fresh_by_finish {
            // The command that owned any standing progress report
            // finished at this handoff — the report dies with it
            // (osc94-progress spec), on the same edge the timer
            // freezes its duration.
            self.reported_progress = None;
            // The program behind a 7501 report ended here too, but
            // only its live states die with it (osc7501-status
            // spec): `working`/`blocked` describe a program that no
            // longer runs, while `done`/`error` persist until
            // replaced, cleared, or respawned — finishes are already
            // narrated by their own honest channels, and the
            // protocol says both survive.
            if self.program_status.is_some_and(|status| {
                matches!(status.state, ProgramState::Working | ProgramState::Blocked)
            }) {
                self.program_status = None;
            }
        }
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
                    // The idle-delay gate's engine side
                    // (suggest-delay spec): the typed segment's
                    // stability. A changed line restarts the clock
                    // BEFORE the observation resolves its idle fact —
                    // the keystroke's own update must read as
                    // just-typed (instant dismiss, no grace) — while
                    // a fresh-prompt anchor adoption (detected after
                    // the step that adopts) restarts it for the
                    // updates that follow: a new line is new typing.
                    // An unchanged line (scroll away and back) keeps
                    // its accumulated idle: the user's typing is the
                    // fact, the viewport is not.
                    let now = Instant::now();
                    if before_cursor != self.suggest_last_before_cursor {
                        self.suggest_typed_at = now;
                        self.suggest_last_before_cursor.clone_from(&before_cursor);
                    }
                    let typed_idle_ms =
                        u16::try_from(now.duration_since(self.suggest_typed_at).as_millis())
                            .unwrap_or(u16::MAX);
                    let observation = suggest::Observation {
                        enabled: self.autosuggestions_enabled,
                        paths_eligible,
                        paths_enabled: self.autosuggestions_paths_enabled,
                        path_listing,
                        commands_enabled: self.autosuggestions_commands_enabled,
                        specs_enabled: self.autosuggestions_specs_enabled,
                        aliases: &self.autosuggestions_aliases,
                        specs_table: &self.autosuggestions_specs_table,
                        command_catalog: &self.command_catalog,
                        generator_candidates: generator_sets.as_ref().map(|sets| {
                            suggest::GeneratorSets {
                                branches: &sets.branches,
                                tags: &sets.tags,
                                files: &sets.files,
                            }
                        }),
                        focused: self.is_focused,
                        foreground_child,
                        display_offset: grid.display_offset(),
                        cursor_on_bottom_row: cursor.line == grid.bottommost_line(),
                        columns,
                        cursor_column: cursor.column.0,
                        before_cursor: &before_cursor,
                        after_cursor_blank,
                        fresh_prompt: fresh_by_finish || self.settle_prompt_pending,
                        delay_ms: self.autosuggestions_delay_ms,
                        typed_idle_ms,
                    };
                    suggest::step(&observation, &mut self.suggest);
                    // The completion menu closes on scroll-away
                    // (suggest-menu spec, the record's lifecycle
                    // ruling): the panel anchors to the cursor's
                    // viewport cell, and scrolling moves the cursor
                    // out from under it.
                    if observation.display_offset != 0 && self.menu.is_open() {
                        self.menu.close();
                    }
                    // The auto-popup opt-in (suggest-menu spec, the
                    // OQ-1 condition decided in the module's final
                    // task): with the key on, this idle update's
                    // typed segment collects the providers' full
                    // sets — two or more candidates pops the menu
                    // without the chord, narrowing past the boundary
                    // (or any ineligibility, which collects nothing)
                    // closes an auto-opened one. A chord-opened
                    // session is the user's: only its own rules
                    // close it. Default off never runs a byte of
                    // this — the chord is today's only opener.
                    if self.autosuggestions_menu_auto
                        && self.autosuggestions_menu_enabled
                        && self.autosuggestions_enabled
                        && observation.focused
                        && !observation.foreground_child
                        && observation.display_offset == 0
                        && let Some(typed) = self.suggest.typed_segment(observation.before_cursor)
                        && !typed.is_empty()
                    {
                        let collected = suggest::candidates(typed, &self.suggest, &observation);
                        if collected.len() >= 2 {
                            self.menu_auto_opened = true;
                            self.menu.open(collected);
                        } else if self.menu.is_open() && self.menu_auto_opened {
                            self.menu.close();
                        }
                    }
                    // The adoption reset (suggest-delay spec): a
                    // fresh anchor — the first adoption or a
                    // post-finish re-adoption — is a new line, so
                    // the updates that follow count their idle from
                    // this moment.
                    if self.suggest.anchor() != self.suggest_last_anchor.as_deref() {
                        self.suggest_last_anchor = self.suggest.anchor().map(str::to_owned);
                        self.suggest_typed_at = now;
                    }
                    // TEMPORARY AUTOSUGGEST DEBUG TRACE: one line per
                    // changed state, gated on the env var, appended to
                    // /tmp/dropterm-asdbg.log and echoed to stderr.
                    if std::env::var_os("DROPTERM_AUTOSUGGEST_DEBUG").is_some() {
                        let line = format!(
                            "en={} foc={} fg={} off={} bottom={} cl={} cc={} cols={} fresh={} anch={} ab={} b4={:?} show={:?}",
                            observation.enabled,
                            observation.focused,
                            observation.foreground_child,
                            observation.display_offset,
                            observation.cursor_on_bottom_row,
                            cursor.line.0,
                            cursor.column.0,
                            columns,
                            observation.fresh_prompt,
                            self.suggest.anchored(),
                            observation.after_cursor_blank,
                            &observation.before_cursor[observation
                                .before_cursor
                                .char_indices()
                                .rev()
                                .nth(40)
                                .map_or(0, |(i, _)| i)..],
                            self.suggest
                                .showing()
                                .map(|s| (s.visible.as_str(), s.source)),
                        );
                        if let Ok(mut last) = ASDBG_LAST.lock()
                            && *last != Some(line.clone())
                        {
                            *last = Some(line.clone());
                            eprintln!("ASDBG {line}");
                            let _ = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open("/tmp/dropterm-asdbg.log")
                                .and_then(|mut f| {
                                    use std::io::Write;
                                    writeln!(f, "{line}")
                                });
                        }
                    }
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

/// The shell's own cwd report (osc7-cwd spec): an OSC 7 payload,
/// parsed. `host` is `None` for an empty host (the local case);
/// the path is percent-decoded and keeps its leading slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedCwd {
    pub host: Option<String>,
    pub path: String,
}

/// The standing states of an OSC 9;4 progress report
/// (osc94-progress spec). State 0 is not one of them — it clears
/// the report; the fork's parse guard already refuses anything
/// past 4, so the enum needs no more members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    InProgress,
    Error,
    Indeterminate,
    Paused,
}

/// The progress decay window (osc94-progress spec): a standing
/// OSC 9;4 report expires this long after its last refresh, so a
/// reporter that dies mid-command cannot leave its last number
/// lying — Ghostty's own hardcoding, the record's inherited rule.
/// Fixed, no knob (OQ-2).
const PROGRESS_DECAY: Duration = Duration::from_secs(15);

/// A standing OSC 9;4 progress report (osc94-progress spec): the
/// reporter's state, the last value it gave (a percent 0–100,
/// inherited when a report omits its own — the protocol's
/// leave-unchanged rule), and the instant of the last refresh —
/// the decay clock's anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedProgress {
    pub state: ProgressState,
    pub value: Option<u8>,
    /// The decay anchor — engine policy, read through
    /// [`Terminal::progress_decay_deadline`]; crate-visible only so
    /// the band's headless fixtures can stand a report.
    pub(crate) refreshed_at: Instant,
}

impl ReportedProgress {
    /// One report applied to the standing state: state 0 clears
    /// (the protocol's own invalidation, whatever value rides it);
    /// any other state stands, a report that omits its value keeps
    /// the standing one, and every report re-anchors the decay
    /// clock. States past the fork's 0–4 guard are ignored whole —
    /// unreachable here, and still not a guess.
    fn apply(standing: Option<Self>, state: u8, value: Option<u8>, now: Instant) -> Option<Self> {
        let state = match state {
            0 => return None,
            1 => ProgressState::InProgress,
            2 => ProgressState::Error,
            3 => ProgressState::Indeterminate,
            4 => ProgressState::Paused,
            _ => return standing,
        };
        Some(Self {
            state,
            value: value.or(standing.and_then(|standing| standing.value)),
            refreshed_at: now,
        })
    }

    /// Whether [`PROGRESS_DECAY`] has passed since the last
    /// refresh — the decay death's truth.
    fn expired(&self, now: Instant) -> bool {
        now >= self.refreshed_at + PROGRESS_DECAY
    }
}

/// The program's own status states (OSC 7501, osc7501-status spec):
/// the protocol's five reportable states. `clear` is not one of
/// them — it clears the record rather than standing as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

/// The OSC 7501 body cap (osc7501-status spec, the `REPORTED_CWD_MAX`
/// hardening precedent): a body past 4 KiB is not a status worth
/// parsing — reject whole, never truncate, and never disturb a
/// standing record with one.
const PROGRAM_STATUS_MAX: usize = 4096;

/// A standing OSC 7501 root-record report (osc7501-status spec): the
/// program's own status — `blocked` and `error` tint the disc
/// `attention` — with its optional progress percent 0–100. Child
/// records never reach the seat (root-only: one disc, one name seat,
/// the program's own summary); `app`, `title`, `msg`, and `kind` are
/// consumed by nothing — ignored by design, named in the docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramStatus {
    pub state: ProgramState,
    pub progress: Option<u8>,
}

impl ProgramStatus {
    /// One report applied to the standing record, the protocol's own
    /// grammar: pairs split on `:`; unknown keys ignored (the
    /// protocol's extension path); malformed pairs skipped; a
    /// repeated key's last value winning. Each report replaces its
    /// record completely — an omitted key vanishes, no inheritance.
    /// A report carrying an `id` is a child record and is ignored
    /// whole (root-only), including an id'd `clear`; the root
    /// `state=clear` removes the standing record. A missing or
    /// unrecognized `state` discards the report whole — a mangled
    /// report never disturbs a standing truth.
    fn apply(standing: Option<Self>, body: &str) -> Option<Self> {
        if body.len() > PROGRAM_STATUS_MAX {
            return standing;
        }
        let mut state: Option<ProgramState> = None;
        let mut clear = false;
        let mut progress: Option<Option<u8>> = None;
        let mut id = false;
        for pair in body.split(':') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "state" => match value {
                    "idle" => {
                        state = Some(ProgramState::Idle);
                        clear = false;
                    }
                    "working" => {
                        state = Some(ProgramState::Working);
                        clear = false;
                    }
                    "done" => {
                        state = Some(ProgramState::Done);
                        clear = false;
                    }
                    "blocked" => {
                        state = Some(ProgramState::Blocked);
                        clear = false;
                    }
                    "error" => {
                        state = Some(ProgramState::Error);
                        clear = false;
                    }
                    "clear" => {
                        clear = true;
                        state = None;
                    }
                    // The protocol's one required key, unrecognized:
                    // the whole report discards.
                    _ => return standing,
                },
                "progress" => {
                    progress = Some(value.parse::<u8>().ok().filter(|&percent| percent <= 100));
                }
                // An `id` marks a child record — not the band's
                // narrator, whatever else the report carries.
                "id" => id = true,
                // Unknown keys MUST be ignored: the protocol's
                // extension path, and the seat for the
                // ignored-by-design set (`app`, `kind`, `title`,
                // `msg`).
                _ => {}
            }
        }
        if id {
            return standing;
        }
        if clear {
            return None;
        }
        let Some(state) = state else {
            return standing;
        };
        Some(Self {
            state,
            progress: progress.flatten(),
        })
    }
}

/// The OSC 7 payload cap (osc7-cwd spec, /ship hardening): a cwd
/// past 4 KiB is not a cwd worth narrating — and an unbounded
/// payload is a `cat`-away UI stall, its whole length text-shaped
/// on every band layout — so over-long reports reject whole, never
/// truncate.
const REPORTED_CWD_MAX: usize = 4096;

impl ReportedCwd {
    /// Parse an OSC 7 payload — `file://[host]/path`. Everything
    /// else (another scheme, a missing path, an escape that is not
    /// exactly two hex digits, a decode that is not UTF-8, a control
    /// or bidirectional character re-admitted by an escape, or a
    /// payload beyond [`REPORTED_CWD_MAX`]) is rejected: a mangled
    /// report never replaces a live truth.
    pub fn parse(url: &str) -> Option<Self> {
        if url.len() > REPORTED_CWD_MAX {
            return None;
        }
        let rest = url.strip_prefix("file://")?;
        let slash = rest.find('/')?;
        let host = percent_decode(&rest[..slash])?;
        let path = percent_decode(&rest[slash..])?;
        Some(Self {
            host: (!host.is_empty()).then_some(host),
            path,
        })
    }

    /// Whether the report names a host other than ours (osc7-cwd
    /// spec): an empty host is local by convention; a named host is
    /// local exactly when it names ours — case-insensitively, the
    /// DNS rule. Without a local name to compare against
    /// (`None` — the hostname read failed), a named host is taken
    /// at its word: remote.
    pub fn names_remote_host(&self, local: Option<&str>) -> bool {
        match (&self.host, local) {
            (None, _) => false,
            (Some(host), Some(ours)) => !host.eq_ignore_ascii_case(ours),
            (Some(_), None) => true,
        }
    }
}

/// Percent-decode a URL segment (`%41` → `A`); an escape must be
/// exactly two hex digits (a `+` sign is not one, whatever
/// `from_str_radix` says), a decode that is not UTF-8 rejects
/// whole, and so does any character that can only be there to
/// mislead — see [`is_invisible`].
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = input.get(i + 1..i + 3)?;
                let [hi, lo] = hex.as_bytes() else {
                    return None;
                };
                if !hi.is_ascii_hexdigit() || !lo.is_ascii_hexdigit() {
                    return None;
                }
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    (!decoded.chars().any(is_invisible)).then_some(decoded)
}

/// Whether a character can only be there to mislead: a control
/// character (vte's OSC layer strips these raw, so an escape
/// re-admitting one is a payload lying about being text) or a
/// bidirectional override / zero-width mark (the invisible spoof
/// class the band's plain text owes no trust to). A report carrying
/// one rejects whole.
fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{FEFF}')
}

#[cfg(test)]
mod tests;

// ===== Inline images (iterm-images spec) =====

/// The per-image payload cap (iterm-images spec): the largest
/// base64-decoded image bytes one sequence may carry, enforced while
/// chunks accumulate — a bigger sequence drops whole, never
/// half-buffered. A default; flip = one line (OQ-4).
const IMAGE_PAYLOAD_CAP: usize = 32 * 1024 * 1024;

/// The decode dimension ceiling (iterm-images spec): the `image`
/// crate's width/height limit, applied before any decode — half the
/// decompression-bomb guard. A default; flip = one line (OQ-4).
const IMAGE_MAX_DIMENSION: u32 = 8192;

/// The decode allocation ceiling (iterm-images spec): the `image`
/// crate's allocation limit, applied before any decode — the other
/// half of the guard. A default; flip = one line (OQ-4).
const IMAGE_MAX_ALLOC: u64 = 64 * 1024 * 1024;

/// The store's decoded-byte budget across all live images
/// (iterm-images spec): kitty's and `WezTerm`'s own number. A ceiling,
/// not an allocation — the oldest image is evicted to stay under it.
/// A default; flip = one line (OQ-4).
const IMAGE_STORE_BUDGET: usize = 320 * 1024 * 1024;

/// One `width`/`height` argument in iTerm2's unit grammar
/// (iterm-images spec): `N` cells, `Npx` pixels, `N%` of the session,
/// or `auto` — the image's inherent size.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SizeUnit {
    Cells(u32),
    Pixels(u32),
    Percent(u32),
    Auto,
}

impl SizeUnit {
    /// Parse `N`, `Npx`, `N%`, or `auto`; anything else is `None` —
    /// the argument reads as absent, the same silence unknown args
    /// get. Never panics: the wire is untrusted.
    fn parse(value: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(value).ok()?;
        if text.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else if let Some(number) = text.strip_suffix("px") {
            number.parse().ok().map(Self::Pixels)
        } else if let Some(number) = text.strip_suffix('%') {
            number.parse().ok().map(Self::Percent)
        } else {
            text.parse().ok().map(Self::Cells)
        }
    }

    /// Resolve to a pixel length against the cell size, the session
    /// width, and the image's inherent axis — the units' one
    /// conversion point.
    #[expect(
        clippy::cast_precision_loss,
        reason = "unit numbers are user-typed counts far below 2^24; exactness past it is meaningless"
    )]
    fn to_pixels(self, cell: f32, session: f32, inherent: f32) -> f32 {
        match self {
            Self::Cells(n) => n as f32 * cell,
            Self::Pixels(n) => n as f32,
            Self::Percent(n) => session * n as f32 / 100.0,
            Self::Auto => inherent,
        }
    }
}

/// The placement-relevant args of a `File=`/`MultipartFile=` sequence
/// (iterm-images spec): the size grammar, the aspect switch, and the
/// inline gate. `size`/`name`/`type` are hints — carried nowhere day
/// one — and unknown keys are ignored by the same silence.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageArgs {
    width: Option<SizeUnit>,
    height: Option<SizeUnit>,
    /// `preserveAspectRatio` — iTerm2's default 1; `0` alone disables.
    preserve_aspect: bool,
    /// `inline` — iTerm2's default 0 (a download there); `DropTerm`
    /// renders nothing and logs nothing when off.
    inline: bool,
}

impl Default for ImageArgs {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            preserve_aspect: true,
            inline: false,
        }
    }
}

impl ImageArgs {
    /// Apply one `key=value` pair; a malformed known value reads as
    /// absent (None), an unknown key is silence.
    fn apply(&mut self, key: &[u8], value: &[u8]) {
        match key {
            b"width" => self.width = SizeUnit::parse(value),
            b"height" => self.height = SizeUnit::parse(value),
            b"preserveAspectRatio" => self.preserve_aspect = value != b"0",
            b"inline" => self.inline = value == b"1",
            _ => {}
        }
    }
}

/// Parse a `File=`/`MultipartFile=` sequence's parameter slices into
/// args and (for the legacy single-shot form) its payload: the first
/// slice carrying a `:` splits there — the pre-colon bytes are one
/// more arg, the post-colon bytes the base64 payload. Chunked headers
/// carry no colon and therefore no payload. Pure; never panics.
fn parse_image_args(slices: &[Vec<u8>]) -> (ImageArgs, Option<Vec<u8>>) {
    let mut args = ImageArgs::default();
    let mut payload = None;
    for (index, slice) in slices.iter().enumerate() {
        // The first slice carries its opcode prefix; its remainder is
        // an arg like every other slice.
        let body = if index == 0 {
            slice
                .strip_prefix(b"File=")
                .or_else(|| slice.strip_prefix(b"MultipartFile="))
        } else {
            Some(&slice[..])
        };
        let Some(mut body) = body else { continue };
        // The legacy single-shot form carries its payload after the
        // first `:` of the slice that has one; the pre-colon bytes
        // remain an arg.
        if payload.is_none()
            && let Some(colon) = body.iter().position(|&byte| byte == b':')
        {
            payload = Some(body[colon + 1..].to_vec());
            body = &body[..colon];
        }
        if let Some(equals) = body.iter().position(|&byte| byte == b'=') {
            args.apply(&body[..equals], &body[equals + 1..]);
        }
    }
    (args, payload)
}

/// The engine's decoded-image store (iterm-images spec): id → pixels
/// plus the running decoded-byte total, oldest-evicted to stay under
/// the budget. Evicted ids draw blank — cells outlive their pixels by
/// design (OQ-3); the grid's own honesty. The renderer handles ride
/// beside the pixels: `from_rgba` mints a fresh identity per call, so
/// each id's pixels become a handle exactly once and the renderer's
/// own cache holds the upload from there.
#[derive(Default)]
pub struct ImageStore {
    /// Insertion order doubles as age: the front is the oldest.
    images: VecDeque<(u64, image::RgbaImage)>,
    /// The running decoded-byte total across live images.
    bytes: usize,
    /// Renderer handles by id — pixels-to-handle made exactly once.
    handles: HashMap<u64, cosmic::iced::widget::image::Handle>,
}

impl ImageStore {
    /// The pixels standing behind an id — `None` when the id never
    /// existed or was evicted; the blank the widget draws.
    pub fn get(&self, id: u64) -> Option<&image::RgbaImage> {
        self.images
            .iter()
            .find(|(stored, _)| *stored == id)
            .map(|(_, rgba)| rgba)
    }

    /// The renderer handle for an id, building it from the pixels on
    /// first ask and serving the clone after — the renderer's cache
    /// keys on the handle, so one build is one upload, forever.
    /// `None` when the id stands behind no pixels.
    pub fn handle_for(&mut self, id: u64) -> Option<cosmic::iced::widget::image::Handle> {
        if let Some(handle) = self.handles.get(&id) {
            return Some(handle.clone());
        }
        let rgba = self.get(id)?;
        let handle = cosmic::iced::widget::image::Handle::from_rgba(
            rgba.width(),
            rgba.height(),
            rgba.as_raw().clone(),
        );
        self.handles.insert(id, handle.clone());
        Some(handle)
    }

    /// The running decoded-byte total (test surface for the budget).
    #[cfg(test)]
    fn total_bytes(&self) -> usize {
        self.bytes
    }

    /// Insert an image under its id, evicting from the oldest until
    /// the budget holds — but never evicting the image just inserted:
    /// a single image larger than the whole budget still shows (the
    /// cap is a ceiling, and decode's own allocation limit keeps any
    /// single image far under it). An evicted id's handle dies with
    /// its pixels.
    fn insert_with_budget(&mut self, id: u64, rgba: image::RgbaImage, budget: usize) {
        self.bytes += rgba.len();
        self.images.push_back((id, rgba));
        while self.bytes > budget && self.images.len() > 1 {
            if let Some((oldest, evicted)) = self.images.pop_front() {
                self.bytes -= evicted.len();
                self.handles.remove(&oldest);
            }
        }
    }

    /// Insert under the shipped budget.
    fn insert(&mut self, id: u64, rgba: image::RgbaImage) {
        self.insert_with_budget(id, rgba, IMAGE_STORE_BUDGET);
    }
}

/// One accumulating `MultipartFile` session (iterm-images spec): the
/// header's parsed args plus the growing decoded-payload bytes. One
/// per terminal (OQ-1 — imgcat never interleaves); a new header
/// aborts whatever stood by replacement.
struct PendingImage {
    args: ImageArgs,
    payload: Vec<u8>,
}

/// Decode one image payload under the strict limits (iterm-images
/// spec): dimension and allocation ceilings applied before any decode
/// — the decompression-bomb guard — and only the four sanctioned
/// formats (png/jpeg/gif/webp; anything else fails format detection
/// and drops whole). Synchronous by design: no threads touch the
/// damage path.
fn decode_image(payload: &[u8]) -> Option<image::RgbaImage> {
    use image::ImageDecoder;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(IMAGE_MAX_DIMENSION);
    limits.max_image_height = Some(IMAGE_MAX_DIMENSION);
    limits.max_alloc = Some(IMAGE_MAX_ALLOC);
    let mut reader = image::ImageReader::new(io::Cursor::new(payload))
        .with_guessed_format()
        .ok()?;
    // The reader's limits seat covers PNG; every other decoder needs
    // them set on the decoder itself (the crate's own split), so both
    // are applied before any decode runs.
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().ok()?;
    decoder.set_limits(limits).ok()?;
    // The allocation ceiling, enforced where the decoder exposes its
    // header dimensions but not its allocations (PNG ignores
    // max_alloc — the crate's own documented gap): the pixel budget
    // the ceiling buys at four bytes a pixel, xterm.js's `pixelLimit`
    // shape. Checked before any pixel is read.
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > IMAGE_MAX_ALLOC / 4 {
        return None;
    }
    image::DynamicImage::from_decoder(decoder)
        .ok()
        .map(|dynamic| dynamic.to_rgba8())
}

/// Pixels to whole cells, never fewer than one (iterm-images spec).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the max(1.0) floors the value into usize range; rounding is the whole point"
)]
fn pixels_to_cells(pixels: f32, cell: f32) -> usize {
    (pixels / cell).round().max(1.0) as usize
}

/// Resolve a placement's cell dimensions (iterm-images spec): the unit
/// grammar against the cell pixel size and the session dims, aspect
/// preserved unless `preserveAspectRatio=0` — one given axis scales
/// the other along, both given fit within, neither leaves the image
/// inherent. Every result is at least one cell. Pure; never panics.
#[expect(
    clippy::cast_precision_loss,
    reason = "session pixel dims are window-sized, far below 2^24"
)]
fn resolve_placement(args: &ImageArgs, image: (f32, f32), size: &Size) -> (usize, usize) {
    let (inherent_w, inherent_h) = image;
    let box_w = args.width.unwrap_or(SizeUnit::Auto).to_pixels(
        size.cell_width,
        size.width as f32,
        inherent_w,
    );
    let box_h = args.height.unwrap_or(SizeUnit::Auto).to_pixels(
        size.cell_height,
        size.height as f32,
        inherent_h,
    );

    let (target_w, target_h) = if args.preserve_aspect {
        let scale = match (args.width, args.height) {
            (Some(_), Some(_)) => f32::min(box_w / inherent_w, box_h / inherent_h),
            (Some(_), None) => box_w / inherent_w,
            (None, Some(_)) => box_h / inherent_h,
            (None, None) => 1.0,
        };
        (inherent_w * scale, inherent_h * scale)
    } else {
        (box_w, box_h)
    };

    (
        pixels_to_cells(target_w, size.cell_width),
        pixels_to_cells(target_h, size.cell_height),
    )
}

/// Decode a base64 slice from the wire (iterm-images spec); a
/// malformed slice is `None` — dropped whole, never half-carried.
fn decode_base64(data: &[u8]) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

// ===== Kitty graphics (kitty-images spec) =====

/// The one error every forbidden transmission form answers with
/// (kitty-images spec): file, temp-file, and shared-memory
/// transmission never cross the seam, and the answer is byte-identical
/// for all three so nothing about the filesystem is probeable —
/// kitty's own uniform refusal wording.
const KITTY_UNIFORM_ERROR: &str = "EBADF:Failed to read image file";

/// The control keys of one APC `G` sequence (kitty-images spec) — the
/// transmission subset's grammar. Unknown keys are ignored by the
/// spec's own rule; a known key with a value outside its grammar
/// drops the sequence whole.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct KittyKeys {
    /// `a` — action: `t` transmit-only (the default), `T`
    /// transmit-and-display; `p`/`q`/`d` are the command arms.
    action: Option<u8>,
    /// `f` — format: 24 and 100 ride the container decode path, 32 is
    /// raw RGBA sized by `s`×`v`. Absent means 32 (kitty's default).
    format: Option<u32>,
    /// `t` — transmission medium: `d` direct in-band, the default and
    /// the only accepted form.
    transmission: Option<u8>,
    /// `m=1` — more chunks follow; absent or `m=0` is the final chunk
    /// (possibly empty, the observed chafa shape).
    more: bool,
    /// `o=z` — the payload arrives zlib-compressed.
    compressed: bool,
    /// `s`/`v` — the image's pixel width and height.
    width_px: Option<u32>,
    height_px: Option<u32>,
    /// `c`/`r` — the display region in cells.
    cols: Option<u32>,
    rows: Option<u32>,
    /// `x`/`y` — the displayed region's source offset; the region
    /// extends to the image's far edges.
    offset_x: Option<u32>,
    offset_y: Option<u32>,
    /// `i` — the client-chosen image id.
    image_id: Option<u32>,
    /// `p` — the placement id.
    placement_id: Option<u32>,
    /// `z` — the z-index layer (consumed at paint).
    z: Option<i32>,
    /// `q` — reply suppression: 1 silences OK replies, 2 errors too.
    quiet: u8,
}

impl KittyKeys {
    /// Whether this chunk opens a transmission: it carries any control
    /// key beyond the chunking keys `m`/`i`/`q` — kitty's own rule
    /// that all control keys ride the first chunk, inverted.
    fn opens(&self) -> bool {
        self.action.is_some()
            || self.format.is_some()
            || self.transmission.is_some()
            || self.compressed
            || self.width_px.is_some()
            || self.height_px.is_some()
            || self.cols.is_some()
            || self.rows.is_some()
            || self.offset_x.is_some()
            || self.offset_y.is_some()
            || self.placement_id.is_some()
            || self.z.is_some()
    }
}

/// Parse one `G` sequence's control data (kitty-images spec):
/// comma-separated single-character `key=value` pairs. Unknown keys
/// are ignored; a known key with a malformed value drops the sequence
/// whole. Pure; never panics.
fn parse_kitty_keys(control: &[u8]) -> Option<KittyKeys> {
    fn number(value: &[u8]) -> Option<u32> {
        std::str::from_utf8(value).ok()?.parse().ok()
    }

    let mut keys = KittyKeys::default();
    for token in control.split(|&byte| byte == b',') {
        let eq = token.iter().position(|&byte| byte == b'=')?;
        let (key, value) = token.split_at(eq);
        let value = &value[1..];
        // Kitty's keys are single characters; anything longer is
        // malformed control data.
        if key.len() != 1 {
            return None;
        }
        match key[0] {
            b'a' => {
                keys.action = match value {
                    [byte] if b"tTpqd".contains(byte) => Some(*byte),
                    _ => return None,
                };
            }
            b't' => {
                keys.transmission = match value {
                    [byte] if b"dfts".contains(byte) => Some(*byte),
                    _ => return None,
                };
            }
            b'f' => keys.format = Some(number(value)?),
            b'm' => {
                keys.more = match value {
                    b"1" => true,
                    b"0" => false,
                    _ => return None,
                };
            }
            b'o' => {
                if value != b"z" {
                    return None;
                }
                keys.compressed = true;
            }
            b's' => keys.width_px = Some(number(value)?),
            b'v' => keys.height_px = Some(number(value)?),
            b'c' => keys.cols = Some(number(value)?),
            b'r' => keys.rows = Some(number(value)?),
            b'x' => keys.offset_x = Some(number(value)?),
            b'y' => keys.offset_y = Some(number(value)?),
            b'i' => keys.image_id = Some(number(value)?),
            b'p' => keys.placement_id = Some(number(value)?),
            b'z' => keys.z = Some(std::str::from_utf8(value).ok()?.parse().ok()?),
            b'q' => {
                keys.quiet = match value {
                    b"0" => 0,
                    b"1" => 1,
                    b"2" => 2,
                    _ => return None,
                };
            }
            _ => {}
        }
    }
    Some(keys)
}

/// One kitty transmission in flight (kitty-images spec): the first
/// chunk's parsed keys plus the accumulated decoded payload, keyed by
/// the client `i` (`None` is the anonymous slot).
struct KittyChunk {
    keys: KittyKeys,
    payload: Vec<u8>,
}

/// The uniform-refusal reply bytes (kitty-images spec): `i=` rides the
/// reply only when the client sent one. Byte-exact per the spec's own
/// detection contract — a DA1 reply without a graphics reply means no
/// support, so every refusal still answers.
fn kitty_error_reply(id: Option<u32>) -> Vec<u8> {
    match id {
        Some(id) => format!("\x1b_Gi={id};{KITTY_UNIFORM_ERROR}\x1b\\"),
        None => format!("\x1b_G{KITTY_UNIFORM_ERROR}\x1b\\"),
    }
    .into_bytes()
}

/// Inflate one `o=z` payload (kitty-images spec) under the same cap
/// the wire bytes obey — a decompression bomb dies at the cap, never
/// inside a decoder. `None` on a corrupt stream or an over-cap
/// inflate; pure; never panics.
fn inflate_capped(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    // One byte past the cap, so an over-cap stream is detected rather
    // than silently truncated.
    let mut limited = ZlibDecoder::new(data).take(IMAGE_PAYLOAD_CAP as u64 + 1);
    let mut out = Vec::new();
    limited.read_to_end(&mut out).ok()?;
    if out.len() > IMAGE_PAYLOAD_CAP {
        return None;
    }
    Some(out)
}

/// Decode one completed kitty payload (kitty-images spec) under the
/// iterm module's own ceilings — the dimension and allocation limits
/// apply before any pixel exists. `f=24`/`f=100` ride the container
/// decode path (the `image` crate's format detection under the strict
/// `Limits`); `f=32` is raw RGBA whose byte count must be exactly
/// `s`×`v`×4. Anything else drops whole. Pure; never panics.
fn decode_kitty_payload(keys: &KittyKeys, payload: &[u8]) -> Option<image::RgbaImage> {
    let payload = if keys.compressed {
        &inflate_capped(payload)?
    } else {
        payload
    };
    match keys.format.unwrap_or(32) {
        24 | 100 => decode_image(payload),
        32 => {
            let width = keys.width_px?;
            let height = keys.height_px?;
            if width > IMAGE_MAX_DIMENSION
                || height > IMAGE_MAX_DIMENSION
                || u64::from(width) * u64::from(height) > IMAGE_MAX_ALLOC / 4
            {
                return None;
            }
            if payload.len() != width as usize * height as usize * 4 {
                return None;
            }
            image::RgbaImage::from_raw(width, height, payload.to_vec())
        }
        // An off-subset format never crosses the seam.
        _ => None,
    }
}

/// The display region in cells (kitty-images spec): `c`/`r` when
/// given (zero reads as absent — kitty's natural-size marker), else
/// the image's inherent pixels against the cell size.
#[expect(
    clippy::cast_precision_loss,
    reason = "decode-capped dimensions (8192) are exact in f32"
)]
fn kitty_cells(keys: &KittyKeys, image: (u32, u32), size: &Size) -> (usize, usize) {
    let cols = keys.cols.filter(|cols| *cols > 0).map_or_else(
        || pixels_to_cells(image.0 as f32, size.cell_width),
        |cols| cols as usize,
    );
    let rows = keys.rows.filter(|rows| *rows > 0).map_or_else(
        || pixels_to_cells(image.1 as f32, size.cell_height),
        |rows| rows as usize,
    );
    (cols, rows)
}
