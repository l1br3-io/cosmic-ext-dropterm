// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::Event as TermEvent;
use cosmic::app::{Core, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Length;
use cosmic::iced::core::Shell;
use cosmic::iced::core::clipboard::Clipboard;
use cosmic::iced::core::layout::{self, Layout};
use cosmic::iced::core::overlay;
use cosmic::iced::core::renderer::{self, Quad, Renderer as _};
use cosmic::iced::core::widget::{Operation, Tree, Widget, tree};
use cosmic::iced::core::{Border, Color, Rectangle, Size, Vector};
use cosmic::iced::event::listen_raw;
use cosmic::iced::keyboard::key::Named;
use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers};
use cosmic::iced::mouse;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    destroy_layer_surface, set_exclusive_zone, set_keyboard_interactivity, set_layer,
};
use cosmic::iced::runtime::core::event::PlatformSpecific;
use cosmic::iced::runtime::core::event::wayland::LayerEvent;
use cosmic::iced::{Event, Subscription, clipboard, window};
use cosmic::surface::Action as SurfaceAction;
use cosmic::theme::Theme;
use cosmic::widget;
use cosmic::{Application, Element, Renderer};
use enumflags2::BitFlags;
use tokio::sync::mpsc;

use crate::config::{
    CONFIG_VERSION, DropTermConfig, Edge, TabPosition, TabTitleMode, ThemePreference,
};
use crate::dbus_state;
use crate::geometry::{self, Geometry};
use crate::grab_bar::GrabBar;
use crate::keybinds;
use crate::profile;
use crate::session;
use crate::settings::{self, SettingsTab};
use crate::slide::{self, SlideBox};
use crate::status_bar::{self, StatusBar, Strip};
use crate::surface;
use crate::tab_bar::{TabBar, TabMenuPick, TooltipSide};
use crate::tabs::{self, Tab, TabId};
use crate::terminal::{Action, TerminalBox, TerminalOptions};

pub(crate) mod remote;
mod spawn;
mod surface_lifecycle;
mod tab_actions;
mod theme_glue;

use self::remote::{Flags, ToggleAction, parse_forwarded_action, startup_action};
use self::surface_lifecycle::{
    Drag, SurfaceState, config_edit_reshapes, grab_bar_acts_on, layer_unfocused_acts_on,
    resize_acts_on,
};
use self::tab_actions::{RenameEdit, open_trees};
use self::theme_glue::{effective_is_dark, theme_override, theme_override_task};

/// The app's identity — the single string behind the single-instance
/// key, the bus name, the config namespace, and the wayland app id.
/// The lib's `cosmic_ext_dropterm::APP_ID` owns the value so the
/// applet consumes the same one; the `dev-identity` feature swaps in
/// the `.Dev` variant that keeps a dev build off an installed
/// release's instance slot.
pub const APP_ID: &str = cosmic_ext_dropterm::APP_ID;

/// Debounce window for toggle requests, guarding against the double
/// key-press events a compositor occasionally delivers for one physical
/// press (see the README's known-risks section).
const TOGGLE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

#[derive(Debug, Clone)]
pub enum Message {
    ToggleAction(ToggleAction),
    ConfigChanged(DropTermConfig),
    /// The `set_theme` task from a theme-preference change landed:
    /// re-run the color apply so the `WINDOW_BG_COLOR` derivation —
    /// which reads the process-global theme — sees the new mode. Cell
    /// colors were already resolved correctly in the same pass via
    /// `effective_is_dark` when the change arrived.
    ReapplyColors,
    TermEventTx(mpsc::UnboundedSender<(TabId, TermEvent)>),
    TermEvent(TabId, TermEvent),
    SurfaceOpened(window::Id, cosmic::iced::Size),
    /// The granted size of the surface with `id` changed (compositor
    /// configure after a `set_size`, or the initial configure): the new
    /// height in logical pixels, used to pin the terminal's height so the
    /// tab bar keeps its strip. The id gates the stream to the tracked
    /// layer surface (`resize_acts_on`): a foreign window's heights
    /// must never feed the pin.
    WindowResized(window::Id, f32),
    LayerFocused(window::Id),
    LayerUnfocused(window::Id),
    KeyPressed(Modifiers, Key, cosmic::iced::keyboard::key::Physical),
    WindowFocused,
    WindowUnfocused,
    MiddleClick,
    /// A tab bar button was left-clicked.
    TabSelected(TabId),
    /// A tab bar button was closed via its `×` or a middle-click.
    TabClosed(TabId),
    /// A status-band segment was click-copied — the widget already
    /// wrote the clipboard; the app flashes the segment's `✓`.
    StatusSegmentCopied(status_bar::SegmentId),
    /// The copy flash elapsed: the `✓` yields back to the value.
    StatusCopyFlashLapse,
    /// The status band's minute tick fired: refresh the stored clock
    /// and re-arm while the gate holds.
    StatusClockTick,
    /// The pending close-confirmation modal was dismissed: cancel —
    /// the tab, its process, and its selection state stay untouched
    /// (close-confirm criterion 4).
    CloseConfirmCancel,
    /// The pending close-confirmation modal's Close Tab was pressed:
    /// the tab id the modal armed with — resolved against the live
    /// list at apply time, no-op on a gone tab (criterion 4's stale
    /// guard).
    CloseConfirmProceed(TabId),
    /// Switch a live tab's profile in place: the tab id (the bar's
    /// stable handle, like every tab message) and the config profile
    /// index — resolved at apply time, the `NewProfileTab`
    /// precedent, so a config edit between snapshot and pick can
    /// still be caught as stale. The apply respawns the tab's
    /// terminal under the profile (see [`DropTerm::switch_tab_profile`]).
    SwitchTabProfile(TabId, usize),
    /// A tab body was double-clicked: start an inline rename.
    RenameStarted(TabId),
    /// The rename input's draft changed.
    RenameDraftChanged(String),
    /// The rename should commit now: Enter was pressed, a press
    /// landed outside the input, or the surface lost focus.
    RenameCommitted,
    /// The rename input was dismissed with Escape or Tab: cancel,
    /// keeping the previous title.
    RenameCancelled,
    /// A tab was dragged to a new position in the bar: the dragged
    /// tab's id and the index it was dropped at.
    TabMoved(TabId, usize),
    /// The per-tab context menu's Duplicate Tab row: duplicate the
    /// tab with this id — the remote `duplicate-tab --tab-index` verb
    /// on that tab (cwd and rename pin read from it, after-active
    /// landing, the duplicate activated).
    DuplicateTab(TabId),
    /// The per-tab context menu's Pin/Unpin row: flip the pin of the
    /// tab with this id, resolved against the live list at apply
    /// time. A pinned tab refuses user-initiated closes and moves,
    /// and the pin persists through save/restore.
    PinToggled(TabId),
    /// The per-tab context menu's Move rows: nudge the tab with this
    /// id one slot toward the direction, clamping at the edge — the
    /// move binds' move with the clicked tab in place of the active
    /// one, resolved against the live list at apply time.
    TabMove(TabId, tabs::MoveDirection),
    /// The per-tab context menu's Close Other Tabs row: every tab but
    /// this one through the gated close front door, one confirmation
    /// at a time for the running ones.
    CloseOtherTabs(TabId),
    /// The per-tab context menu's Close Tabs to the Right row: every
    /// tab right of this one through the same gated front door.
    CloseTabsRight(TabId),
    /// The grab bar was dragged: the pointer's surface-local y
    /// (window-absolute, not a delta and not bar-local) while a press
    /// is held. Targets recompute absolutely from it, so the moving
    /// window frame of a bottom-anchored surface cannot compound
    /// drift into the drag.
    GrabBarDragged(f32),
    /// A grab bar drag ended: the surface keeps the height it settled
    /// at; persisting it is the debounced config write's job.
    GrabBarReleased,
    /// The trailing apply of a throttled live drag fired: a target
    /// dropped inside the apply interval still lands — the pointer
    /// pausing mid-drag must not leave the surface behind. A stale
    /// fire (drag ended, surface hidden) is a no-op: the apply path
    /// re-checks the cadence gate.
    DragApplyDue,
    /// The trailing adopt of a debounced terminal-pin height fired:
    /// heights the adoption debounce skipped — the double-reported
    /// granted stream's echoes — land once the stream settles. A
    /// stale fire is a no-op.
    WindowPinDue,
    /// The debounce window for a drag-release height write elapsed:
    /// the generation that scheduled it, checked against the current
    /// pending write so a newer drag's schedule cancels this stale
    /// one.
    WriteDue(u32),
    /// The session-save debounce window elapsed: the generation that
    /// armed it, checked against the armed window so a stale fire —
    /// superseded by a newer window, or a duplicate delivery after
    /// the write — writes nothing.
    SessionSaveDue(u32),
    /// Slide clock tick: redraw-only while a slide is live — the
    /// eased offset is evaluated at draw time inside `SlideBox`, so
    /// the tick exists to keep redraw requests pending (see
    /// `SLIDE_TICK`). The first tick past the duration parks the
    /// slide at its endpoint — rest for an `In` slide, the anchored
    /// edge (then destroy) for an `Out` one — and drops it (and with
    /// it the subscription).
    SlideTick,
    OpenHyperlink(String),
    ClipboardPasted(Option<String>),
    ClipboardPastedPrimary(Option<String>),
    /// An event from the settings drawer's controls. Inputs only —
    /// the update side resolves each through the pure planners and
    /// commits it as a `Bare` write through [`Self::write_config`],
    /// whose `ConfigChanged` echo is the only apply path.
    Settings(settings::view::SettingsEvent),
    /// The settings tab strip: a row was activated. The row's entity
    /// rides the message — the activation closure is `'static` and
    /// cannot borrow the stored model — and the handler resolves it
    /// to its `SettingsTab` through the model before applying the
    /// page-turn rule.
    PickSettingsTab(cosmic::widget::segmented_button::Entity),
    /// A context-menu item was picked: the item's
    /// [`terminal::Action`], dispatched through `perform_action` —
    /// the same entry point the keybind map uses, so menu and keybind
    /// behavior cannot fork.
    Action(Action),
    /// The terminal's context menu should open or close. Published
    /// by the `TerminalBox` press handler: `true` on a right-press
    /// with the menu closed, `false` on
    /// any default-mode press while open (toggle-close). The
    /// wrapper's `on_close` also sends `false` — it fires on every
    /// close path (Escape, click-away, item pick, compositor
    /// dismiss), so duplicate `false`s are the norm and harmless:
    /// the arm is a plain state set.
    ContextMenu(bool),
    /// A surface action from a widget: the context-menu popup's
    /// create/destroy requests, forwarded to libcosmic verbatim —
    /// popups are created and destroyed by the runtime, never by the
    /// app (as in cosmic-term).
    Surface(SurfaceAction<Message>),
}

// App-wide UI state: each bool is an independent, named fact of the
// interface (focus, drawer, clock arming), not a state machine.
#[expect(clippy::struct_excessive_bools)]
pub struct DropTerm {
    core: Core,
    /// Config backend, consumed by the drag-resize write-back:
    /// `set("height_percent", …)` persists a settled drag. `None`
    /// when the config failed to open at init — writes are skipped,
    /// everything else works.
    config_handler: Option<cosmic_config::Config>,
    config: DropTermConfig,
    /// The overlay's terminals in tab order. Never empty while a
    /// surface is shown: closing the last tab respawns a fresh one
    /// (see `ensure_tabs`).
    tabs: Vec<Tab>,
    /// Index into `tabs` of the focused tab.
    active_tab: usize,
    terminal_id: widget::Id,
    term_event_tx: Option<mpsc::UnboundedSender<(TabId, TermEvent)>>,
    /// Source of monotonically increasing tab ids, never reused within
    /// a run.
    next_tab_id: u32,
    /// The startup session stash: the restore plan loaded from the
    /// state file in `init`, consumed by the first productive run of
    /// `ensure_tabs`.
    restore: session::RestoreStash,
    /// A remote `new-tab` request that arrived before the terminal
    /// event channel was created (first-launch startup): stashed
    /// here, applied the moment the channel bootstraps and the
    /// session restore has landed, so the requested tab never races —
    /// or is lost to — the restore.
    pending_remote_tab: Option<ToggleAction>,
    /// Debounced session-save state: which change window is armed and
    /// whether a write is owed (see `session::SessionSave`).
    session_save: session::SessionSave,
    /// Each live tab's working directory as last seen, seeded at
    /// spawn/restore with the spawn cwd: title events compare against
    /// this cache so only a real cwd change — not title churn — marks
    /// the session dirty.
    tab_cwds: HashMap<TabId, Option<PathBuf>>,
    /// The active tab's foreground-process name for the status band's
    /// disc-and-name segment (status-bar criterion 5): a snapshot
    /// refreshed only at event piggyback points — title reports,
    /// terminal wakeups, tab switches, spawns/respawns, zoom,
    /// config echo, and the surface's show transition (the show
    /// arm's landing refresh, status-glance criterion 4) — never per
    /// frame; the band widget renders plain resolved data. `None`
    /// reads idle or unreadable, and the band falls back to the
    /// recorded shell's basename.
    active_process_name: Option<String>,
    /// The active tab's git branch for the band's branch segment —
    /// the same snapshot discipline as the process name: the lone
    /// `.git` read, refreshed at the same piggyback points, gated on
    /// the band's master toggle. `None` reads outside a repo,
    /// unreadable, or hidden.
    active_branch: Option<String>,
    /// The band segment currently flashing its `✓` after a
    /// click-to-copy — set on the copied message, cleared by the
    /// flash-lapse task (`status_bar::COPY_FLASH`).
    copied_segment: Option<status_bar::SegmentId>,
    /// The formatted wall clock (`HH:MM`) for the band's clock
    /// segment — set at init, refreshed by the minute-aligned tick,
    /// and cleared with the gate: hidden means no tick at all.
    status_clock: Option<String>,
    /// Whether a minute tick sleeps in the task queue — the arm
    /// guard that keeps repeated config echoes from stacking tick
    /// loops (each fired tick clears it before re-syncing).
    clock_armed: bool,
    /// The active inline rename, if any: which tab is being renamed
    /// and the current draft, mirrored into the input the tab bar
    /// overlays on the renamed button.
    rename: Option<RenameEdit>,
    /// The single-slot pending close confirmation (close-confirm
    /// spec): the tab a user-initiated close is waiting on, armed
    /// only when the toggle is on and the tab runs a foreground
    /// child. While `Some`, the modal owns the surface; a new
    /// request replaces the slot, any removal of the tab clears it,
    /// and it survives hide/show like the drawer does.
    pending_close: Option<TabId>,
    /// An in-progress grab-bar height drag, if any: the absolute
    /// anchor every target recomputes from, plus the runtime state of
    /// the apply cadence (see `Drag`).
    drag: Option<Drag>,
    /// Debounced drag-release config-write state: which release's
    /// persist is still pending.
    write_back: geometry::WriteBack,
    /// The geometry of our last config write, kept to recognize and
    /// absorb its `ConfigChanged` echo. Cleared on absorption.
    last_written: Option<Geometry>,
    /// Widget id of the rename input, so focus tasks can target it.
    rename_input_id: widget::Id,
    surface: SurfaceState,
    /// The output size measured by the last `SurfaceOpened`, kept so
    /// later reshapes (config edits, drag) can recompute pixels without
    /// a new measuring pass (see `geometry.rs`).
    output_size: Option<(u32, u32)>,
    /// Granted height of the shown surface, from `WindowResized` events.
    /// The basis for pinning the terminal widget's height so the tab bar
    /// gets its strip inside the fixed-size surface.
    window_height: Option<f32>,
    /// The last pin adoption (when, which height), for the adoption
    /// debounce (see `geometry::window_pin_due`).
    pin_applied: Option<(std::time::Instant, f32)>,
    /// A height the debounce skipped, landing via the trailing adopt
    /// (`WindowPinDue`) once the granted stream settles.
    pending_height: Option<f32>,
    /// The height the app last asked the compositor for via
    /// `set_size` (show pre-arm, drag applies and release, config
    /// reshapes). With `window_height` it forms the stale-echo
    /// gate (`geometry::offer_is_stale_echo`): an offered height
    /// matching neither is the synthetic pre-request echo every
    /// `set_size` double-reports, and must drop whatever the
    /// debounce clock says — adopting it by timing luck grows the
    /// grid once per toggle (toggle-grid-stability, round 2).
    requested_height: Option<f32>,
    /// Whether the surface has received keyboard focus at least once since it
    /// was shown. Guards `hide_on_focus_loss` so a stray `Unfocused` delivered
    /// before any `Focused` (possible with `keyboard_mode: OnDemand`) can't
    /// make the surface open-then-instantly-close.
    surface_focused: bool,
    key_binds: HashMap<cosmic::widget::menu::key_bind::KeyBind, Action>,
    /// The armed keybinding capture (keybinds-editor): the action whose
    /// editor row was clicked. While `Some` the `KeyPressed` arm routes
    /// every press to the capture — see [`capture_verdict_for`] — and
    /// nothing else in the app sees a press.
    keybind_capture: Option<Action>,
    /// The Keybindings tab's search box (keybinds-editor):
    /// display-only filter state following the drawer's buffer rule —
    /// an explicit close clears it, a focus-loss hide preserves it.
    keybind_search: String,
    /// A capture that ended in a conflict (keybinds-editor): the
    /// action whose row was captured and the action already holding
    /// the chord — the inline error row's payload. Cleared by the
    /// next arm, commit, revert, page turn, or close.
    keybind_conflict: Option<(Action, Action)>,
    /// The scheme picker whose popover is open (scheme-picker): the
    /// two global controls or the profile form's picker, `None` =
    /// closed. One at most — the flag is transient view state the
    /// drawer's rules clear (an explicit close, a page turn away
    /// from the hosting page); focus-loss hides preserve it with the
    /// drawer. An open picker owns the first Escape (the popover
    /// first, then the draft, then the drawer — the ordering the
    /// profile-manager spec pinned).
    scheme_picker: Option<settings::SchemePickerId>,
    last_toggle: Option<std::time::Instant>,
    /// The live slide: the eased content offset while the surface
    /// animates — in from the anchored edge after a show, out toward
    /// it before a hide's destroy. `None` at rest — renders
    /// byte-identically to the unwrapped column — and cleared when
    /// the parking tick completes the slide (`SlideTick`).
    slide: Option<slide::Playing>,
    /// Whether the settings drawer is open over the surface (spec:
    /// settings-drawer). Overlay state only — the terminal keeps its
    /// size and the pty its grid (criterion 1); the drawer survives
    /// focus-loss hides with its drafts intact (criterion 8), and an
    /// explicit close clears `settings_buffer`.
    settings_open: bool,
    /// Which page the open drawer shows: a tab's page (General,
    /// Advanced) or the Profiles page the General tab's entry row
    /// flips to (the cosmic-term `ContextPage` pattern,
    /// self-implemented at the one composition site). An explicit
    /// close resets it to General — a reopened drawer starts fresh
    /// from config — while a focus-loss hide preserves it with the
    /// drafts.
    settings_page: SettingsTab,
    /// The tab strip's rows: built once at init
    /// ([`settings::view::tab_strip_model`]) and never rebuilt per frame —
    /// the strip renders from it, each row carrying its `SettingsTab`
    /// as data. The selection is kept in lockstep with
    /// `settings_page` ([`Self::sync_settings_tab_strip`]); the rows
    /// themselves never change.
    settings_tab_model: settings::view::TabStripModel,
    /// The Profiles page's selected row: the index into
    /// `config.profiles` driving the edit form. Cleared whenever the
    /// page is left (back or close) — a reopened page starts from
    /// config.
    profile_selection: Option<usize>,
    /// The Profiles page's form draft, one at a time: an Add/Clone
    /// draft a button opened, or the Edit draft the row selection
    /// seeded. Focus-loss hides preserve it; an explicit close and a
    /// page leave clear it (`ProfileDraftTransition`).
    profile_draft: Option<settings::ProfileDraft>,
    /// The Profiles page's armed delete row (profile-manager): the
    /// row rendering its inline Confirm/Keep pair, `None` = no armed
    /// row. The state machine is [`settings::armed_profile_delete_row`];
    /// the draft lock hides the pair while a draft is open (the arm
    /// waits out the draft). Cleared with the page's other state on
    /// a leave or close; focus-loss hides preserve it.
    profile_delete_armed: Option<usize>,
    /// The settings drawer's in-flight drafts: display-only text and
    /// slider state, cleared on an explicit drawer close so a
    /// reopened drawer renders purely from config (spec criterion 8).
    settings_buffer: settings::SettingsBuffer,
    /// The settings drawer's live drag width in px (drawer-resize
    /// spec criterion 2): the buffer-draft pattern — display-only
    /// while the handle's press is held, winning over the stored
    /// preference at render through the same clamp. One write lands
    /// at release (criterion 3); the double-click reset and drawer
    /// close clear it.
    drawer_drag: Option<u16>,
    /// Whether the terminal's context menu is open: the `TerminalBox`
    /// press handler's latch, read by the view through
    /// `open_trees` and fed back to the box so any
    /// press while open toggle-closes it. Cleared by `hide()` — the
    /// popup dies with the surface and `on_close` never fires there.
    context_menu_open: bool,
    /// Queryable state on the owned bus name: the connection the
    /// `dbus_connection` hook handed over, the shared snapshot the
    /// served V1 interface reads, and the last-served snapshot the
    /// sync step dedupes against. Inert until the hook fires —
    /// headless, all three stay exactly as initialized.
    dbus_conn: Option<zbus::Connection>,
    dbus_shared: Arc<Mutex<dbus_state::State>>,
    dbus_last: dbus_state::State,
}

impl DropTerm {
    /// The active tab, when any tab exists.
    fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab)
    }

    /// The terminal options a tab under `profile` renders with: the
    /// profile's font override, else the global pick —
    /// [`profile::effective_font_size`], the `profile ?? global`
    /// rule's font arm — plus the global autosuggest flag (the
    /// config→options flow's one flat addition, autosuggest spec).
    /// `None` (plain tabs, dangling ids) is the pure global
    /// resolution.
    fn terminal_options_for(&self, profile: Option<&profile::Profile>) -> TerminalOptions {
        TerminalOptions {
            font_size: profile::effective_font_size(profile, self.config.font_size),
            autosuggestions_enabled: self.config.autosuggestions_enabled,
            ..TerminalOptions::default()
        }
    }

    /// Refreshes the active tab's status snapshot — the foreground
    /// process name and the cwd's git branch — the lone caller of
    /// the engine's comm read and of the `.git` walk (the
    /// construction pin: [`status_bar::StatusBar`] takes plain
    /// resolved data and cannot read `/proc` or `.git` itself, so
    /// the render path performs zero filesystem reads by
    /// construction). The branch read stops before the filesystem
    /// while the band's master toggle is off.
    fn refresh_status_snapshot(&mut self) {
        let Some(tab) = self.active() else {
            self.active_process_name = None;
            self.active_branch = None;
            return;
        };
        let cwd = self.tab_cwds.get(&tab.id).and_then(Option::as_deref);
        let process = tab.terminal.lock().unwrap().foreground_process_name();
        // Hidden means never gathered: the `.git` walk stops at the
        // gate, before the filesystem.
        let branch = (self.config.status_bar_enabled && self.config.status_bar_show_branch)
            .then(|| status_bar::git_branch(cwd))
            .flatten();
        self.active_process_name = process;
        self.active_branch = branch;
    }

    /// Sync the stored clock to the gate: set the text and arm the
    /// minute-aligned tick when the gate turns on (once — arming
    /// again while a tick sleeps would stack loops), stand down when
    /// it turns off. A fired tick clears the arm and re-syncs: the
    /// loop survives while the gate holds and dies within one minute
    /// of it dropping.
    fn sync_status_clock(&mut self) -> Task<Message> {
        if !status_bar::clock_due(
            self.config.status_bar_enabled,
            self.config.status_bar_show_clock,
        ) {
            self.status_clock = None;
            self.clock_armed = false;
            return Task::none();
        }
        let now = jiff::Zoned::now();
        self.status_clock = Some(status_bar::clock_text(&now));
        if self.clock_armed {
            return Task::none();
        }
        self.clock_armed = true;
        Task::perform(
            tokio::time::sleep(status_bar::next_tick_delay(&now)),
            |()| cosmic::Action::App(Message::StatusClockTick),
        )
    }

    /// The active tab's resolved status-band segments: pure assembly
    /// over existing in-app state — the cwd cache, the recorded
    /// shell, the id-resolved profile, the live zoom adjustment —
    /// plus the process snapshot (see
    /// [`status_bar::resolve_segments`]). No tabs (startup, or
    /// momentarily mid-close) resolves the empty default.
    fn active_status_segments(&self) -> status_bar::StatusSegments {
        let Some(tab) = self.active() else {
            return status_bar::StatusSegments::default();
        };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = self.tab_cwds.get(&tab.id).and_then(Option::as_deref);
        let resolved = tab
            .profile
            .as_deref()
            .and_then(|id| profile::find(&self.config.profiles, id));
        // The grid and its spawn record ride the same in-memory
        // lock as the zoom — resize-fresh by construction, no
        // snapshot of its own; the record is the engine-owned
        // resting grid (status-glance criterion 1).
        let (zoom_adj, grid, spawn_grid) = {
            let terminal = tab.terminal.lock().unwrap();
            (terminal.zoom_adj(), terminal.grid(), terminal.spawn_grid())
        };
        // The step the tab's terminal actually zooms by: the same
        // options construction every spawn builds from.
        let zoom_step = self
            .terminal_options_for(resolved)
            .font_size_zoom_step_mul_100;
        let inputs = status_bar::StatusInputs {
            home: home.as_deref(),
            cwd,
            shell: &tab.shell,
            profile_name: resolved.map(|preset| preset.name.as_str()),
            zoom_adj,
            zoom_step_mul_100: zoom_step,
            foreground: self.active_process_name.as_deref(),
            branch: self.active_branch.as_deref(),
            grid: Some(grid),
            spawn_grid: Some(spawn_grid),
            tab_position: self.active_tab_position(),
            clock: self.status_clock.as_deref(),
        };
        status_bar::resolve_segments(&inputs, self.config.status_toggles())
    }

    /// The active tab's (index, total) for the band's tab segment —
    /// saturated into `u16`; a session beyond 65k tabs has larger
    /// problems than the band's arithmetic.
    fn active_tab_position(&self) -> Option<(u16, u16)> {
        let total = self.tabs.len();
        (total > 0).then(|| {
            (
                u16::try_from(self.active_tab + 1).unwrap_or(u16::MAX),
                u16::try_from(total).unwrap_or(u16::MAX),
            )
        })
    }

    /// The one path every config write goes through: the drag release
    /// persists here as `PreApplied` — the set plus the `last_written`
    /// record whose `ConfigChanged` echo the update loop absorbs — and
    /// every settings control persists here as `Bare`, recording
    /// nothing so its echo flows into the live-apply path.
    /// Newest intent wins: a bare write of the drag's key first
    /// cancels any pending debounced write-back, so the stale
    /// `WriteDue` finds nothing and the bare write's value is what
    /// lands — while a drag release after it simply schedules fresh
    /// and lands last. Values clamp before persisting; a missing
    /// config backend (it failed to open at init) logs and skips —
    /// the drag itself still resized live.
    fn write_config(&mut self, kind: settings::WriteKind, write: &settings::PlannedWrite) {
        if kind == settings::WriteKind::Bare
            && settings::bare_write_cancels_pending_drag(write.key(), self.write_back.has_pending())
        {
            self.write_back.cancel_pending();
        }
        let write = write.clamped();
        let key = write.key();
        let Some(handler) = self.config_handler.as_ref() else {
            log::warn!("no config handler; skipping {} write", key.key_name());
            return;
        };
        match write.persist(handler) {
            Ok(()) => {
                // Only a pre-applied write absorbs its echo — geometry
                // was already applied at release, so the echo must not
                // reshape. The pre-applied writer is exactly the
                // height-only drag release. A bare write records
                // nothing: its echo *is* the apply.
                if let (
                    settings::WriteKind::PreApplied,
                    settings::PlannedWrite::HeightPercent(height),
                ) = (kind, &write)
                {
                    let mut written = Geometry::from_config(&self.config);
                    written.height_percent = *height;
                    self.last_written = Some(written);
                }
            }
            Err(err) => log::warn!("failed to persist {}: {err}", key.key_name()),
        }
    }

    /// The one path every profile write takes — `write_config`'s
    /// discipline verbatim (pure planner in, log-and-skip on a
    /// missing handler, the `ConfigChanged` echo drives every apply)
    /// but bypassing the scalar-keyed [`settings::PlannedWrite`]:
    /// profile keys carry list- and Option-shaped values. Nothing is
    /// recorded and nothing applies here — a bare write's echo *is*
    /// the apply, so the in-memory config the next view build reads
    /// is the echo-updated one.
    fn write_profile_config(&mut self, write: &settings::ProfileWrite) {
        let Some(handler) = self.config_handler.as_ref() else {
            log::warn!("no config handler; skipping profile write");
            return;
        };
        if let Err(err) = write.persist(handler) {
            log::warn!("failed to persist profile write: {err}");
        }
    }

    /// The one path every keybinds write takes — `write_profile_config`'s
    /// discipline for the map-shaped key: persist the whole
    /// `custom_key_binds` map, and the `ConfigChanged` echo drives the
    /// apply — config update plus the live-map rebuild in the echo
    /// handler — so the very next press matches the new map.
    fn write_keybinds_config(&mut self, write: &settings::KeybindsWrite) {
        let Some(handler) = self.config_handler.as_ref() else {
            log::warn!("no config handler; skipping keybinds write");
            return;
        };
        if let Err(err) = write.persist(handler) {
            log::warn!("failed to persist keybinds write: {err}");
        }
    }

    /// The one settings trigger (spec criteria 5 and 7): the
    /// keybind, the context-menu "Settings…" item, the tab-bar
    /// settings button, and the drawer's own Close button all route
    /// through `Action::Settings` into here. Opening flips the
    /// drawer over the live terminal — the surface never hides or
    /// resizes (criterion 1); a repeat press closes (criterion 5).
    fn toggle_settings(&mut self) -> Task<Message> {
        if self.settings_open {
            self.close_settings_drawer()
        } else {
            self.settings_open = true;
            Task::none()
        }
    }

    /// Closes the settings drawer: the explicit close path (Escape,
    /// the toggle's repeat press, the drawer's Close button), so the
    /// buffer clears per the close-clears-vs-hide-preserves rule
    /// (spec criterion 8 — a focus-loss hide preserves everything for
    /// the next show; hides touch nothing) and keyboard focus returns
    /// to the terminal the drawer had borrowed (cosmic-term's
    /// `on_context_drawer` refocus). The drawer reopens on the
    /// landing tab with no selection — fresh from config, whatever
    /// page it closed on.
    fn close_settings_drawer(&mut self) -> Task<Message> {
        self.settings_open = false;
        // The scheme popover dies with the drawer: the explicit close
        // clears transient UI state (the keybind search and capture
        // rule below); the landing turn stays on Appearance, so the
        // page-turn clear would not catch it.
        self.scheme_picker = None;
        self.turn_settings_page(home_settings_page());
        // The Keybindings tab's search and capture state are drawer
        // buffer state: cleared on the explicit close, preserved
        // across focus-loss hides.
        self.keybind_search.clear();
        self.keybind_capture = None;
        self.keybind_conflict = None;
        self.settings_buffer
            .apply_drawer_transition(settings::DrawerTransition::ExplicitClose);
        widget::text_input::focus(self.terminal_id.clone())
    }

    /// Cancels the pending close confirmation (close-confirm
    /// criterion 4): the slot clears and keyboard focus returns to
    /// the terminal the modal had borrowed — the tab, its process,
    /// and its selection state stay untouched. Escape and the scrim's
    /// press-cancels meet here, the `RenameCancelled` refocus shape.
    fn cancel_pending_close(&mut self) -> Task<Message> {
        self.pending_close = None;
        widget::text_input::focus(self.terminal_id.clone())
    }

    /// Composes the pending close-confirmation modal over `content`
    /// (close-confirm criterion 4), outermost — above the settings
    /// drawer when both exist, surviving hide/show with the surface —
    /// and armed-only: with no pending close the content passes
    /// through byte-identically. The scrim is a full-surface
    /// `mouse_area` whose press cancels (the card's buttons capture
    /// their own presses first, so the dialog stays clickable)
    /// carrying a centered `widget::dialog` card naming the tab by
    /// its title only — never the process name.
    fn close_confirm_modal<'a>(&self, content: Element<'a, Message>) -> Element<'a, Message> {
        let Some(pending) = self.pending_close else {
            return content;
        };
        let title = self
            .tabs
            .iter()
            .find(|tab| tab.id == pending)
            .map(|tab| tab.title.clone())
            .unwrap_or_default();
        let card = widget::dialog()
            .title("Close tab?")
            .body(format!("“{title}” is running a process. Close it anyway?"))
            .secondary_action(
                widget::button::standard("Cancel").on_press(Message::CloseConfirmCancel),
            )
            .primary_action(
                widget::button::destructive("Close Tab")
                    .on_press(Message::CloseConfirmProceed(pending)),
            );
        let scrim = widget::mouse_area(
            widget::container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(cosmic::iced::alignment::Horizontal::Center)
                .align_y(cosmic::iced::alignment::Vertical::Center)
                .class(cosmic::theme::Container::custom(scrim_style(
                    MODAL_SCRIM_ALPHA,
                ))),
        )
        .on_press(Message::CloseConfirmCancel);
        cosmic::iced::widget::Stack::with_children([content, scrim.into()])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Turn the drawer's page to `tab` — the strip pick's and the
    /// explicit close's shared page-turn path. A turn away from the
    /// Profiles tab drops that page's state — selection, the form
    /// draft, the armed delete — so a reopened Profiles page starts
    /// from config; any other turn keeps every draft. Focus-loss
    /// hides never route here: preservation is their rule.
    fn turn_settings_page(&mut self, tab: SettingsTab) {
        if leaves_profiles_tab(self.settings_page, tab) {
            self.profile_selection = None;
            self.profile_delete_armed = None;
            settings::apply_profile_draft_transition(
                &mut self.profile_draft,
                settings::ProfileDraftTransition::Clear,
            );
        }
        // A turn away from the hosting page closes the scheme popover
        // with its anchor — a stale open flag must not resurrect it on
        // return (the keybind-capture page-turn rule). Both hosting
        // pages turn through here, so every non-self turn clears.
        if tab != self.settings_page {
            self.scheme_picker = None;
        }
        self.settings_page = tab;
        self.sync_settings_tab_strip();
    }

    /// Re-sync the strip's selection to `settings_page` (criterion
    /// 7): the row of the showing tab becomes the model's selection.
    /// Applied after every page mutation: a pick or the explicit
    /// close's reset. The rows themselves never change.
    fn sync_settings_tab_strip(&mut self) {
        // A page turn away from the Keybindings tab disarms any armed
        // capture and clears its conflict — an invisible capture must
        // never eat presses on another page (the view can't show its
        // hint row there).
        if self.settings_page != SettingsTab::Keybindings {
            self.keybind_capture = None;
            self.keybind_conflict = None;
        }
        settings::view::activate_tab_row(&mut self.settings_tab_model, self.settings_page);
    }
}

/// The tab the drawer resets to on an explicit close, so a reopened
/// drawer always starts on the everyday controls.
const fn home_settings_page() -> SettingsTab {
    SettingsTab::Appearance
}

/// Whether a page turn from `from` to `to` leaves the Profiles tab —
/// the rule that clears the Profiles page's draft state (selection
/// and form draft). A re-pick of the Profiles row itself is not a
/// leave. Pure so the rule is headless-testable (constructing
/// `DropTerm` needs a live `Core`).
const fn leaves_profiles_tab(from: SettingsTab, to: SettingsTab) -> bool {
    matches!(from, SettingsTab::Profiles) && !matches!(to, SettingsTab::Profiles)
}

/// What the app does with a key press before any other key role
/// while the settings drawer is open (spec criterion 6): Escape
/// closes the drawer — and only that. The surface does not hide,
/// and the press is never forwarded to the terminal (which is
/// disabled while the drawer is open anyway, criterion 4). One
/// rule sits ahead of this one in the key funnel — the pending
/// close confirmation's modal gate ([`modal_key_routing`]) answers
/// first — and one sits inside it: with a profile draft open,
/// Escape cancels the draft and the drawer stays open beneath (the
/// draft-first rule, a pure decision). Every other key — including
/// the Ctrl+Shift+I toggle, which resolves through the normal bind
/// match into [`DropTerm::toggle_settings`] — keeps the normal
/// routing; with the drawer closed the rule never fires and Escape
/// is terminal input exactly as before.
/// Pure so the drawer-first rule is headless-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawerKeyRouting {
    /// The drawer is open with a profile draft in it and this
    /// Escape cancels the draft: swallow the press here — the
    /// drawer stays open.
    CancelDraft,
    /// The drawer is open and this press closes it: swallow it here.
    CloseDrawer,
    /// No drawer-first role: the press takes the normal routing.
    FallThrough,
}

fn drawer_escape_routing(settings_open: bool, draft_open: bool, key: &Key) -> DrawerKeyRouting {
    if settings_open && matches!(key, Key::Named(Named::Escape)) {
        if draft_open {
            DrawerKeyRouting::CancelDraft
        } else {
            DrawerKeyRouting::CloseDrawer
        }
    } else {
        DrawerKeyRouting::FallThrough
    }
}

/// The close-gate decision (close-confirm criterion 3): confirm only
/// when the toggle is on AND a foreground child is running — either
/// input false closes immediately, exactly today's behavior
/// (criterion 5). Pure so the truth table is headless-testable.
fn confirm_needed(enabled: bool, foreground_running: bool) -> bool {
    enabled && foreground_running
}

/// What the app does with a key press while a close confirmation is
/// pending (close-confirm criterion 4): Escape cancels the modal —
/// with precedence over the settings drawer's own Escape handling,
/// which sees a later press only after the slot clears — and every
/// other press is swallowed while the decision waits. Transparent
/// with no pending close: routing is exactly as before. Pure so the
/// rule is headless-testable, the `drawer_escape_routing` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalKeyRouting {
    /// A close confirmation is pending and this Escape cancels it:
    /// swallow the press here.
    Cancel,
    /// A close confirmation is pending; any other press is consumed
    /// by the modal, never the terminal or a bind.
    Swallow,
    /// No pending close: the press takes the normal routing.
    FallThrough,
}

fn modal_key_routing(pending_close: bool, key: &Key) -> ModalKeyRouting {
    if !pending_close {
        return ModalKeyRouting::FallThrough;
    }
    if matches!(key, Key::Named(Named::Escape)) {
        ModalKeyRouting::Cancel
    } else {
        ModalKeyRouting::Swallow
    }
}

/// The confirmation modal's scrim style: a translucent black wash
/// over the whole surface (50%), the standard modal dim — the
/// `danger_text_style` hand-composed precedent for theme styles this
/// drawer-less layer surface cannot get from a preset.
fn scrim_style(
    alpha: f32,
) -> impl Fn(&cosmic::theme::Theme) -> cosmic::iced::widget::container::Style {
    move |_theme| cosmic::iced::widget::container::Style {
        background: Some(cosmic::iced::Background::Color(
            cosmic::iced::Color::from_rgba8(0, 0, 0, alpha),
        )),
        ..Default::default()
    }
}

/// How hard each scrim dims: the close-confirm modal owns the
/// surface for a moment's decision, while the settings drawer stays
/// open for reading — its backdrop recedes the content beneath
/// without blacking it out.
const MODAL_SCRIM_ALPHA: f32 = 0.5;
const DRAWER_SCRIM_ALPHA: f32 = 0.4;

/// Composes the settings drawer's backdrop scrim over `content`
/// (drawer-scrim criterion 1): the whole base tree — terminal, tab
/// strip, grab bar — under one dim veil, so the drawer's opaque pane
/// reads as the surface's one live layer. Paint-only by design: the
/// open drawer's `context_drawer` overlay captures every surface
/// mouse event before the base tree updates (the parked
/// drawer-click-close finding), so the veil carries no `mouse_area`
/// — nothing base-tree-side can observe a press anyway. The veil is
/// a plain quad painted after the child in the base render layer —
/// deliberately NOT a `Stack` overlay child: iced draws those inside
/// a dedicated `with_layer(viewport)` clip group, and on a
/// translucent surface (terminal `opacity` < 100) that extra layer
/// composited as a light washed-out smear over the tab bar and
/// terminal while the resize drag re-laid-out the pane every frame
/// (drawer-resize hardware walk, 2026-10-08 — gone at opacity 100,
/// gone without the veil, present only mid-drag). The close-confirm
/// scrim keeps its Stack: it never animates, so the moving-layer
/// interplay cannot arise there.
fn drawer_backdrop(content: Element<'_, Message>) -> Element<'_, Message> {
    Backdrop::new(content, Color::from_rgba8(0, 0, 0, DRAWER_SCRIM_ALPHA)).into()
}

/// A passthrough wrapper that paints one translucent quad over its
/// child's bounds after the child draws — the veil half of
/// [`drawer_backdrop`]. Transparent in the state tree and in layout
/// (the `SlideBox` forwarding pattern): the wrapper adds nothing but
/// the quad, so the child renders exactly as it would unwrapped and
/// the veil lands in the same render layer, above the child, below
/// the drawer pane's overlay.
struct Backdrop<'a, Message> {
    child: Element<'a, Message>,
    color: Color,
}

impl<'a, Message> Backdrop<'a, Message> {
    fn new(child: Element<'a, Message>, color: Color) -> Self {
        Self { child, color }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Backdrop<'_, Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        self.child.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.child.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.child.as_widget().children()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.child.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.child.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.child.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.child
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // The child first, then the veil quad in the same layer: the
        // veil sits above the base tree without introducing the clip
        // group a `Stack` overlay child would.
        self.child
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
        renderer.fill_quad(
            Quad {
                bounds: layout.bounds(),
                border: Border {
                    radius: 0.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                snap: true,
                ..Default::default()
            },
            self.color,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.child
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message> From<Backdrop<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(backdrop: Backdrop<'a, Message>) -> Self {
        Self::new(backdrop)
    }
}

/// The `KeyPressed` arm's first gate (keybinds-editor criterion 3):
/// while a capture is armed it owns every press — its verdict replaces
/// the rename guard, the drawer's Escape rule, and bind matching.
/// `None` (disarmed) is transparent: the press keeps exactly today's
/// routing. Pure so the ordering rule is headless-testable.
fn capture_verdict_for(
    capture: Option<Action>,
    modifiers: Modifiers,
    key: &Key,
) -> Option<keybinds::CaptureVerdict> {
    capture.map(|_| keybinds::capture_verdict(modifiers, key))
}

/// The profile a recorded tab id resolves to right now — `None` for
/// plain tabs and dangling ids (a deleted profile, a hand-edited
/// config): the lookup every per-tab appearance resolution (colors,
/// font, scrollback) shares. Free over the config so the chain is
/// headless-pinnable (constructing `DropTerm` needs a live `Core`).
fn resolved_profile<'a>(
    config: &'a DropTermConfig,
    name: Option<&str>,
) -> Option<&'a profile::Profile> {
    name.and_then(|name| profile::find(&config.profiles, name))
}

/// Whether a config echo must retitle every live tab: a title-mode
/// flip always does (every mode reads different inputs), and while
/// the (new) mode is `Profile` a profiles-list change does too — a
/// profile renamed or deleted under live tabs is a stale name the
/// moment it happens, not a future concern. The guard is anti-churn:
/// an echo that moves neither — an opacity slide, a keybind, our own
/// write's echo — retitles nothing. Pure beside
/// [`theme_override`], its neighbor in the config-echo decisions.
fn echo_retitles_tabs(
    old_mode: TabTitleMode,
    new_mode: TabTitleMode,
    old_profiles: &[crate::profile::Profile],
    new_profiles: &[crate::profile::Profile],
) -> bool {
    old_mode != new_mode || (new_mode == TabTitleMode::Profile && old_profiles != new_profiles)
}

/// Whether the tab strip sits at the surface's bottom in this
/// config. `Auto` places it at the leading edge beside the grab
/// bar, i.e. opposite the screen edge; `Top`/`Bottom` pin it
/// regardless of anchor.
fn effective_bar_at_bottom(edge: Edge, tab_position: TabPosition) -> bool {
    match tab_position {
        TabPosition::Auto => matches!(edge, Edge::Top),
        TabPosition::Top => false,
        TabPosition::Bottom => true,
    }
}

/// Which side the tab strip's tooltips pop to: the terminal's side of the
/// bar, wherever the bar sits — the bar widget itself stays
/// edge-agnostic.
fn bar_tooltip_side(bar_at_bottom: bool) -> TooltipSide {
    if bar_at_bottom {
        TooltipSide::Above
    } else {
        TooltipSide::Below
    }
}

/// Which side the status band's context menu pops to: the
/// terminal's side of the band. The band hugs the free edge beside
/// the grab bar, so its terminal side is opposite the tab strip's
/// for the same edge — the strip's own helper, inverted.
fn band_menu_side(edge: Edge) -> TooltipSide {
    match edge {
        Edge::Top => TooltipSide::Above,
        Edge::Bottom => TooltipSide::Below,
    }
}

/// The config's font family as the `&'static str` iced's `Font`
/// requires — interned per distinct name, libcosmic's own
/// `FontConfig → Font` conversion leaks-and-caches the same way, so
/// config echo's constant re-renders never grow the intern set.
fn interned_font_family(name: &str) -> &'static str {
    static FAMILY_MAP: std::sync::LazyLock<
        std::sync::RwLock<std::collections::BTreeSet<&'static str>>,
    > = std::sync::LazyLock::new(std::sync::RwLock::default);
    if let Some(name) = FAMILY_MAP.read().unwrap().get(name) {
        return name;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    FAMILY_MAP.write().unwrap().insert(leaked);
    leaked
}

/// Settings for the clipboard-anchor window.
///
/// libcosmic binds its single runtime [`Clipboard`] to the first
/// window created, and when that surface is destroyed the binding is
/// dropped — taking the smithay-clipboard connection, and with it
/// any selection this app copied, down with it. `DropTerm`'s only
/// other surface is the layer shell, which every hide destroys, so
/// copying out could never survive the focus switch to the paste
/// target. This window — never closed, created before any layer
/// surface — owns the binding for the whole process lifetime instead,
/// exempt from close requests, so nothing the user or the runtime
/// does can drop the connection mid-session.
///
/// Wayland cannot honor `visible: false` (winit's `set_visible` is a
/// no-op there), so the anchor is mapped after all and the shell's
/// window switcher lists it. Its `application_id` is the only name
/// that toplevel carries — the runtime does not derive it from
/// `Application::APP_ID` — and with the empty default the switcher
/// matches no desktop file and labels the window "unknown". Pinned
/// to the app id, it resolves to `DropTerm`'s name and icon.
fn clipboard_anchor_settings() -> window::Settings {
    let mut settings = window::Settings {
        visible: false,
        exit_on_close_request: false,
        resizable: false,
        decorations: false,
        size: cosmic::iced::Size::new(1.0, 1.0),
        ..window::Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = APP_ID.to_string();
    }
    settings
}

impl Application for DropTerm {
    type Executor = cosmic::executor::single::Executor;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        // This app owns its translucency: the `opacity` setting is the
        // single dial, applied by the terminal's own background quad.
        // libcosmic's frosted-glass machinery is a second, hidden dial:
        // when the COSMIC dark theme enables frosted glass, a
        // settings-time `set_theme` (the Application theme flip) makes
        // the runtime mark the global theme `transparent` and restyle
        // every container onto the theme's translucent slot — the
        // chrome turns see-through and blurred while the startup path
        // (which races the compositor's late blur-support event) stays
        // opaque, so the app looks inconsistent until restart. Opting
        // out of auto-blur pins `frosted()` false everywhere in the
        // runtime, making the appearance independent of the user's
        // frosted-glass preference in both paths.
        core.set_auto_blur(BitFlags::default());
        let (config_handler, config) =
            match cosmic_config::Config::new(Self::APP_ID, CONFIG_VERSION) {
                Ok(handler) => {
                    let config = match DropTermConfig::get_entry(&handler) {
                        Ok(config) => config,
                        Err((errs, config)) => {
                            log::warn!("errors loading config: {errs:?}");
                            config
                        }
                    }
                    .clamped();
                    (Some(handler), config)
                }
                Err(err) => {
                    log::warn!("failed to open config: {err}");
                    (None, DropTermConfig::default().clamped())
                }
            };

        // The session file is one small synchronous read: stashed
        // here — before any message can spawn a tab — so restore
        // structurally precedes the first `ensure_tabs`, whichever
        // startup message reaches it first. A missing or invalid
        // file stashes nothing (and was already quarantined with a
        // warning inside `load`), keeping first-run startup silent.
        // The session toggle gates the whole entry: disabled, the
        // file is not even read — nothing restores, and the file on
        // disk waits untouched for a future launch with the key on.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let restore = session::restore_entry(
            &session::state_file_path(),
            home.as_deref(),
            config.session_persistence_enabled,
            &config.profiles,
        );

        // The clipboard anchor opens here, before
        // `debounced_toggle` can request the first layer surface: the
        // runtime binds its clipboard to the first window created, so
        // the anchor — not the terminal's layer surface — must win
        // that binding, or every hide would destroy the connection
        // owning whatever the user copied. Its id is deliberately
        // not kept: nothing references the window again (view events
        // for it fall through the foreign-window guards), and it is
        // never closed.
        let (_anchor_window, open_anchor) = window::open(clipboard_anchor_settings());

        // The tab strip's model, built once (criterion 7); its
        // selection starts in lockstep with the initial page.
        let mut settings_tab_model = settings::view::tab_strip_model();
        settings::view::activate_tab_row(&mut settings_tab_model, home_settings_page());

        // The live bind map starts from the loaded config — defaults
        // overlaid with any persisted custom bindings — so a restart
        // keeps customizations from the first press (keybinds-editor
        // criterion 5).
        let key_binds = keybinds::effective_key_binds(&config.custom_key_binds);

        let mut app = Self {
            core,
            config_handler,
            config,
            tabs: Vec::new(),
            active_tab: 0,
            terminal_id: widget::Id::unique(),
            term_event_tx: None,
            next_tab_id: 0,
            restore,
            pending_remote_tab: None,
            session_save: session::SessionSave::default(),
            tab_cwds: HashMap::new(),
            active_process_name: None,
            active_branch: None,
            copied_segment: None,
            status_clock: None,
            clock_armed: false,
            rename: None,
            pending_close: None,
            drag: None,
            write_back: geometry::WriteBack::default(),
            last_written: None,
            rename_input_id: widget::Id::unique(),
            surface: SurfaceState::Hidden,
            output_size: None,
            window_height: None,
            pin_applied: None,
            pending_height: None,
            requested_height: None,
            surface_focused: false,
            key_binds,
            keybind_capture: None,
            keybind_search: String::new(),
            scheme_picker: None,
            keybind_conflict: None,
            last_toggle: None,
            slide: None,
            settings_open: false,
            settings_page: home_settings_page(),
            settings_tab_model,
            profile_selection: None,
            profile_draft: None,
            profile_delete_armed: None,
            settings_buffer: settings::SettingsBuffer::default(),
            drawer_drag: None,
            context_menu_open: false,
            dbus_conn: None,
            dbus_shared: Arc::new(Mutex::new(dbus_state::State {
                visible: false,
                tab_count: 0,
            })),
            dbus_last: dbus_state::State {
                visible: false,
                tab_count: 0,
            },
        };
        // No tabs exist yet, so this resolves the global pick — same
        // value the pre-initiative init computed inline; every later
        // refresh follows the active tab's resolution.
        app.refresh_active_window_bg();

        // A second launch never reaches this: `run_single_instance` forwards
        // its action over D-Bus to the already-running instance instead
        // (see `Application::dbus_activation` below), so the first launch's
        // own action has to be applied here. The anchor window opens first
        // in the batch: the runtime binds its clipboard to the first window
        // created, and the anchor must win that race before any layer
        // surface exists — afterwards the binding never moves again.
        // A preference pinned on disk applies at startup: the
        // runtime's pre-init state is "following the system", so the
        // transition input is `(System, loaded)` — one matrix serves
        // startup and `ConfigChanged`. `System → System` is `None`,
        // so default configs never call `set_theme`. This is a
        // `no_main_window` app: nothing is visible to flash — the
        // surface opens only on the first toggle, after the task
        // lands. Startup colors above already resolved through
        // `effective_is_dark`, so they are correct even before it
        // does.
        let theme_task = theme_override(ThemePreference::System, app.config.theme_preference)
            .map_or_else(Task::none, theme_override_task);
        // First launch has no running state to act on remotely: the
        // tab verbs degrade to a plain start — the default `Toggle` —
        // through `startup_action`, never a fabricated close, rename,
        // duplicate, or switch on a restored-or-fresh session; only
        // new-tab keeps startup application.
        let startup = startup_action(&flags.action);
        if startup != flags.action {
            log::info!("{} with no running instance: starting plain", flags.action);
        }
        let task = Task::batch([
            open_anchor.then(|_| Task::none()),
            app.debounced_toggle(startup),
            theme_task,
            // The clock starts with the config's gate — armed here,
            // kept alive or stood down at every gate re-evaluation.
            app.sync_status_clock(),
        ]);
        (app, task)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct TermEventSubscription;
        struct ConfigSubscription;

        let mut subs = vec![listen_raw(|event, _status, id| match event {
            Event::PlatformSpecific(PlatformSpecific::Wayland(
                cosmic::iced::runtime::core::event::wayland::Event::Layer(
                    LayerEvent::Focused,
                    _,
                    layer_id,
                ),
            )) => Some(Message::LayerFocused(layer_id)),
            Event::PlatformSpecific(PlatformSpecific::Wayland(
                cosmic::iced::runtime::core::event::wayland::Event::Layer(
                    LayerEvent::Unfocused,
                    _,
                    layer_id,
                ),
            )) => Some(Message::LayerUnfocused(layer_id)),
            Event::Window(cosmic::iced::window::Event::Opened { size, .. }) => {
                Some(Message::SurfaceOpened(id, size))
            }
            Event::Window(cosmic::iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(id, size.height))
            }
            Event::Keyboard(KeyEvent::KeyPressed {
                key,
                modifiers,
                physical_key,
                ..
            }) => Some(Message::KeyPressed(modifiers, key, physical_key)),
            _ => None,
        })];

        // Always kept alive (same key every recompute, so iced never
        // restarts the underlying stream): it both bootstraps
        // `term_event_tx` on first run and keeps draining terminal events
        // for as long as the app lives, independent of whether a surface -
        // or even a terminal - currently exists, so PTY output, title and
        // clipboard requests keep being processed while hidden.
        subs.push(Subscription::run_with(
            std::any::TypeId::of::<TermEventSubscription>(),
            |_| {
                cosmic::iced::stream::channel(
                    100,
                    |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                        use cosmic::iced::futures::SinkExt;

                        let (tx, mut rx) = mpsc::unbounded_channel();
                        if output.send(Message::TermEventTx(tx)).await.is_err() {
                            return;
                        }
                        while let Some((tab_id, event)) = rx.recv().await {
                            if output
                                .send(Message::TermEvent(tab_id, event))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    },
                )
            },
        ));

        subs.push(
            cosmic_config::config_subscription::<_, DropTermConfig>(
                std::any::TypeId::of::<ConfigSubscription>(),
                Self::APP_ID.into(),
                CONFIG_VERSION,
            )
            .map(|update| {
                if !update.errors.is_empty() {
                    log::warn!("config errors: {:?}", update.errors);
                }
                Message::ConfigChanged(update.config.clamped())
            }),
        );

        // Slide clock: redraw-only ticks while a slide is live —
        // 1 ms saturation so every compositor frame window contains
        // a pending redraw request (see `SLIDE_TICK`). The stream is
        // dropped the moment the slide parks — idle time costs no
        // ticks.
        if self.slide.is_some() {
            subs.push(cosmic::iced::time::every(slide::SLIDE_TICK).map(|_| Message::SlideTick));
        }

        Subscription::batch(subs)
    }

    fn system_theme_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic::cosmic_theme::Theme,
    ) -> Task<Self::Message> {
        self.apply_config_to_tabs();
        Task::none()
    }

    fn system_theme_mode_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic::cosmic_theme::ThemeMode,
    ) -> Task<Self::Message> {
        self.apply_config_to_tabs();
        Task::none()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        let task = self.update_inner(message);
        // Every surface and tab transition flows through this wrapper,
        // so one sync after the arm — not per-arm calls — keeps the
        // served V1 properties current; an arm's early returns are
        // irrelevant to it. A no-op message dedupes to no emission.
        self.sync_dbus_state();
        task
    }

    fn view(&self) -> Element<'_, Self::Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        let Some(tab) = self.active() else {
            return widget::text("").into();
        };
        if !matches!(
            &self.surface,
            SurfaceState::Measuring(shown)
                | SurfaceState::Shown(shown)
                | SurfaceState::Hiding(shown)
                if *shown == id
        ) {
            return widget::text("").into();
        }

        // No grid without a settled size (toggle-grid-stability):
        // the Measuring surface is the full-output probe, and a
        // `Fill`-height TerminalBox rendered into it would realize
        // the full-output grid the pass exists only to read. With
        // a settled pin (every re-show) the box renders at that
        // height inside the stretched window, byte-identical to
        // the settled frame; without one (first launch) the
        // placeholder holds the measuring round-trip, so the
        // terminal's first realized size is the configured
        // geometry — one opening resize, never the probe.
        if matches!(self.surface, SurfaceState::Measuring(_)) && self.window_height.is_none() {
            return widget::text("").into();
        }

        let opacity = f32::from(self.config.opacity) / 100.0;
        // Pin the terminal's height to the granted window minus the bars'
        // strips — both always reserved, so the 1↔2 tab crossing never
        // resizes the terminal (see `surface::terminal_view_height`).
        let terminal_height = self.window_height.map_or(Length::Fill, |h| {
            Length::Fixed(surface::terminal_view_height(
                h,
                self.config.status_bar_enabled,
            ))
        });
        // The height pin's mirror (toggle-grid-stability, OQ-2):
        // while an output is known the box renders at the settled
        // surface width — during Measuring that keeps a
        // `width_percent < 100` config from bouncing the grid's
        // columns through the full-output probe width. At rest the
        // pin equals the grant, so Shown rendering is
        // byte-identical to Fill.
        let terminal_width = if self.output_size.is_some() {
            Length::Fixed(self.current_surface_width())
        } else {
            Length::Fill
        };
        let terminal: Element<'_, Message> = TerminalBox::new(&tab.terminal, &self.key_binds)
            .id(self.terminal_id.clone())
            .height(terminal_height)
            .width(terminal_width)
            .opacity(opacity)
            .show_headerbar(false)
            // Inert while the settings drawer is open (spec
            // criterion 4) or a close confirmation is pending
            // (close-confirm criterion 4): cosmic-term's discipline —
            // the overlays already capture pointer events, and
            // `disabled` keeps keyboard (raw key events reach widgets
            // whatever the app-level routing swallows) away from the
            // pty.
            .disabled(self.settings_open || self.pending_close.is_some())
            .on_middle_click(|| Message::MiddleClick)
            .on_open_hyperlink(Some(Box::new(Message::OpenHyperlink)))
            .on_window_focused(|| Message::WindowFocused)
            .on_window_unfocused(|| Message::WindowUnfocused)
            .context_menu_open(self.context_menu_open)
            .on_context_menu(Message::ContextMenu)
            .into();

        // The context menu wraps ONLY the terminal element — the
        // wrapper triggers anywhere over its bounds, so the tab bar
        // and grab bar must stay outside it. Trees are supplied only
        // while open (`open_trees`): closed, the
        // wrapper is a plain pass-through with a pixel-identical
        // view. The popup plumbing is the cosmic-term pattern:
        // `on_surface_action` is mandatory — `create_popup` is a
        // no-op without it — and `window_id` must be this layer
        // surface's id (the `view_window` parameter), never the main
        // window's.
        let terminal: Element<'_, Message> = widget::context_menu(
            terminal,
            open_trees(self.context_menu_open, &self.config.profiles),
        )
        .on_close(Message::ContextMenu(false))
        .on_surface_action(Message::Surface)
        .window_id(id)
        .into();

        // NOTE: no pointer-input wrapper for the open settings drawer
        // may sit here — while the drawer is open, its overlay
        // (`widget::context_drawer`) captures every mouse event over
        // the whole surface before this base tree ever updates, so a
        // base-tree mouse_area cannot observe a press. (A base-tree
        // scrim can still *paint* — `drawer_backdrop` does exactly
        // that — it just never sees events.) The drawer-click-close
        // module parked on the input half of this finding
        // (2026-10-08); see SPEC-drawer-click-close.md's fallback
        // clause for the revisit options.

        // Stacking from the screen edge: content, tab strip, grab
        // bar on the free edge. Both bars take their strips out of
        // the terminal's pinned height (see
        // `surface::terminal_view_height`), so neither pushes the
        // other off the surface. Fill dimensions keep iced's flex
        // from deferring the bars past the terminal's allocation: a
        // Shrink cross-axis makes a fixed-height, fill-width bar a
        // fourth-pass child, so the Fill terminal would take the
        // whole surface and push the bars below its bottom edge.
        let grab_bar = GrabBar::new(Message::GrabBarDragged, || Message::GrabBarReleased);

        // The tab strip mounts unconditionally and its side comes
        // from `effective_bar_at_bottom`: `Auto` keeps it between
        // the terminal and the grab bar — on the surface edge
        // opposite the screen edge — while a pin can move it to the
        // screen edge itself. With fewer than two tabs the strip
        // carries only its settings button (the bar's own visibility
        // rule). The inline rename input overlays the renamed tab's
        // button — a rename can only start from a tab-button press,
        // i.e. with 2+ tabs, so the wiring is unchanged by the
        // unconditional mount. The tooltip pops to the terminal's
        // side of the strip, derived from the bar's side, not the
        // anchor edge.
        let bar_at_bottom = effective_bar_at_bottom(self.config.edge, self.config.tab_position);
        let tooltip_side = bar_tooltip_side(bar_at_bottom);
        let mut bar = TabBar::new(&self.tabs, self.active_tab)
            .on_select(Message::TabSelected)
            .on_close(Message::TabClosed)
            .on_move(Message::TabMoved)
            .on_rename(Message::RenameStarted)
            .on_rename_end(|| Message::RenameCommitted)
            // The same handler the context menu's `Settings…` row
            // uses: one click away in every tab state.
            .on_settings(|| Message::Action(Action::Settings))
            // The same handler `Ctrl+Shift+T` uses: the click rides
            // the default-profile/cwd-inheritance/placement funnel
            // unchanged.
            .on_new_tab(|| Message::Action(Action::NewTab))
            // The dropdown rides the same profile list the context
            // menu reads, and its picks the same funnels: the plain
            // row is the default spawn, a profile row the submenu's
            // apply-time-resolved `NewProfileTab` pick.
            .profiles(&self.config.profiles)
            .on_profile_pick(|index| Message::Action(Action::NewProfileTab(index)))
            // The per-tab menu's picks ride the existing funnels —
            // spawn, rename, profile switch, close — with the clicked
            // tab's id in place of "active"; the four id-addressed
            // verbs that had no message gain theirs here.
            .on_tab_menu_pick(|tab, pick| match pick {
                TabMenuPick::NewTab => Message::Action(Action::NewTab),
                TabMenuPick::Rename => Message::RenameStarted(tab),
                TabMenuPick::Duplicate => Message::DuplicateTab(tab),
                TabMenuPick::Pin => Message::PinToggled(tab),
                TabMenuPick::SwitchProfile(index) => Message::SwitchTabProfile(tab, index),
                TabMenuPick::Move(direction) => Message::TabMove(tab, direction),
                TabMenuPick::Close => Message::TabClosed(tab),
                TabMenuPick::CloseOthers => Message::CloseOtherTabs(tab),
                TabMenuPick::CloseRight => Message::CloseTabsRight(tab),
            })
            .tooltip_side(tooltip_side)
            // The border hairline rides the same side the tooltips
            // pop to — the terminal's side of the strip — off the
            // `tab_bar_border` key (bar-borders spec).
            .show_border(self.config.tab_bar_border);
        if let Some(edit) = &self.rename
            && self.tabs.iter().any(|tab| tab.id == edit.tab)
        {
            // The inline rename input, overlaid by the bar on the
            // renamed tab's button. Its value mirrors the app's
            // draft so it survives every re-render.
            bar = bar.renaming(
                edit.tab,
                widget::text_input("", edit.draft.clone())
                    .id(self.rename_input_id.clone())
                    .size(14.0)
                    .on_input(Message::RenameDraftChanged)
                    .on_submit(|_| Message::RenameCommitted)
                    .on_unfocus(Message::RenameCancelled),
            );
        }
        // The status band (status-bar spec): the free-edge-most strip,
        // adjacent to the grab bar, narrating the active terminal in
        // its own configured font at 12 px (OQ-2 — the sanctioned
        // fallback if the family does not resolve at the hardware
        // pass is the default UI font with every other token
        // standing). Constructed from the resolved segments only —
        // the process name comes from the app-side snapshot, never a
        // render-path `/proc` read.
        let band: Element<'_, Message> = StatusBar::new(
            self.active_status_segments(),
            cosmic::iced::Font {
                family: cosmic::iced::font::Family::Name(interned_font_family(
                    &self.config.font_name,
                )),
                ..cosmic::iced::Font::default()
            },
            self.copied_segment,
            Message::StatusSegmentCopied,
        )
        // The menu's action rows ride the `perform_action` funnel
        // every keybind uses — the same `Message::Action` route the
        // tab menu's New Tab row takes; the band's menu introduces
        // no behavior of its own. The bubble pops to the terminal's
        // side of the band (the band hugs the free edge, so that
        // side is the strip's own tooltip side inverted).
        .on_menu_action(Message::Action)
        .menu_side(band_menu_side(self.config.edge))
        // The border hairline rides the same side the menu pops to
        // — the band's side away from the grab bar — off the
        // `status_bar_border` key (bar-borders spec).
        .show_border(self.config.status_bar_border)
        .into();

        // Stacking: `status_bar::column_stacking` owns the orders —
        // the grab bar fixed at the free edge (opposite the screen
        // edge, where all of `geometry.rs`'s drag/anchor math expects
        // it), the status band beside it, the terminal filling the
        // middle, the tab strip wherever `effective_bar_at_bottom`
        // picks. All three strips take theirs out of the terminal's
        // pinned height (see `surface::terminal_view_height` —
        // subtracted order-agnostically), so no layout changes the
        // terminal's pixel height; a disabled band simply drops out
        // of the column and the pin reclaims its strip. Fill
        // dimensions keep iced's flex from deferring the strips past
        // the terminal's allocation: a Shrink cross-axis makes a
        // fixed-height, fill-width strip a fourth-pass child, so the
        // Fill terminal would take the whole surface and push the
        // strips below its edge.
        let stacking = status_bar::column_stacking(self.config.edge, self.config.tab_position);
        let slot_of = |strip: Strip| {
            stacking
                .iter()
                .position(|candidate| *candidate == strip)
                .expect("every strip stacks exactly once (pinned)")
        };
        let mut strips: Vec<Option<Element<'_, Message>>> = (0..4).map(|_| None).collect();
        strips[slot_of(Strip::Tabs)] = Some(bar.into());
        strips[slot_of(Strip::StatusBand)] = self.config.status_bar_enabled.then_some(band);
        strips[slot_of(Strip::Terminal)] = Some(terminal);
        strips[slot_of(Strip::GrabBar)] = Some(grab_bar.into());
        let column: Element<'_, Message> =
            widget::column::with_children(strips.into_iter().flatten().collect::<Vec<_>>())
                .width(Length::Fill)
                .height(Length::Fill)
                .into();

        // The slide: while the surface animates — ease-in from the
        // anchored edge after a show, ease-out back to it before a
        // hide's destroy — the content column's pose is NOT computed
        // here: the view runs in the runtime's AboutToWait pass but
        // the frame presents on the compositor's frame callback, so
        // a view-baked offset is up to one frame stale at
        // presentation — random per-frame error that read as judder
        // on hardware. Instead the live slide state and its true
        // start instant are handed to `SlideBox`, which evaluates
        // the easing inside `draw` against the frame's own clock. No
        // animation in flight → offset 0 → the column renders
        // byte-identically to the unwrapped one.
        let mut slide_box = SlideBox::new(column, 0.0);
        if let Some(playing) = self.slide {
            slide_box = slide_box.playing(self.config.edge, playing);
        }
        let content: Element<'_, Message> = slide_box.into();

        // The settings drawer rides as an overlay on the same layer
        // surface (spec criterion 1): `widget::context_drawer` wraps
        // the whole content column — terminal, tab strip, grab bar —
        // and pins its pane to the right edge without resizing
        // anything beneath (overlay mode, never the side-by-side
        // layout that would reflow the pty grid). Composed by hand
        // because the framework's `view_main` serves only the main
        // window — this surface is a layer-shell window whose view
        // comes from `view_window`, so the `context_drawer()` trait
        // hook cannot reach it (spike decision, settings-drawer
        // Task 0). The Close button routes through the same
        // `Action::Settings` toggle as the keybind (criterion 7).
        // The pending-close modal composes over whichever of the two
        // trees is showing — outermost (close-confirm criterion 4).
        if !self.settings_open {
            return self.close_confirm_modal(content);
        }
        // The backdrop goes on here, outside the SlideBox and inside
        // the drawer: at surface coordinates like the pane itself
        // (which the slide does not move either), veiling the whole
        // base tree while the drawer is open.
        let content = drawer_backdrop(content);
        // The drawer pane's width comes from the drawer-resize width
        // policy (spec criterion 4): a live drag value (criterion 2)
        // wins while the handle's press is held, else the stored
        // preference — `None` until the user first drags — against
        // the live surface width, clamped to [344px, 50% of
        // surface].
        let surface_width = self.current_surface_width();
        // The drawer hosts the tab pages — Profiles among them, a
        // first-class tab since the drill-down and its entry row
        // retired. The page switch is this match at the one
        // hand-composed site; the Close button keeps routing through
        // the same `Action::Settings` toggle from any page, so the
        // keybind/Close/gear funnel never changes under a page turn.
        // The title stays "Settings" on every page — the strip
        // disambiguates.
        let drawer_content = match self.settings_page {
            SettingsTab::Appearance => settings::view::appearance_view(
                &self.config,
                &self.settings_buffer,
                self.scheme_picker,
            ),
            SettingsTab::Behavior => {
                settings::view::behavior_view(&self.config, &self.settings_buffer)
            }
            SettingsTab::Profiles => {
                // `is_dark` resolves the Inherit row's swatch the way
                // the terminal renders — the preference over the
                // system mode, the `effective_is_dark` decision.
                let is_dark =
                    effective_is_dark(self.config.theme_preference, cosmic::theme::is_dark());
                settings::view::profiles_view(
                    &self.config,
                    self.profile_selection,
                    self.profile_draft.as_ref(),
                    self.profile_delete_armed,
                    self.scheme_picker == Some(settings::SchemePickerId::ProfileForm),
                    is_dark,
                )
            }
            SettingsTab::Keybindings => settings::view::keybindings_view(
                &self.config.custom_key_binds,
                &self.keybind_search,
                self.keybind_capture,
                self.keybind_conflict,
            ),
            SettingsTab::Advanced => {
                settings::view::advanced_view(&self.config, &self.settings_buffer)
            }
        };
        // The tab strip rides the drawer's header slot — pinned under
        // the title, above the pane's scrollable — on every page.
        // Rendered from the stored model (never rebuilt per frame);
        // activation carries the row's entity back as a message —
        // the closure is `'static` and cannot borrow the model — and
        // the handler resolves it (criterion 7). 32px like
        // cosmic-edit's doc tabs; the strip-fit ruling at the 344px
        // drawer floor is OQ-1, taken at the hardware pass.
        let tab_strip = widget::tab_bar::horizontal(&self.settings_tab_model)
            .button_height(32)
            .on_activate(Message::PickSettingsTab);
        let drawer: Element<'_, Message> = widget::context_drawer(
            Some("Settings".into()),
            None,
            Some(tab_strip.into()),
            None,
            Message::Action(Action::Settings),
            content,
            settings::view::drawer_with_handle(drawer_content).map(Message::Settings),
            settings::drawer_width(
                self.drawer_drag.or(self.config.settings_drawer_width),
                surface_width,
            ),
        )
        .into();
        self.close_confirm_modal(drawer)
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;
        match msg.msg {
            Details::Activate => Task::done(cosmic::Action::App(Message::ToggleAction(
                ToggleAction::Toggle,
            ))),
            Details::ActivateAction { action, args } => {
                // The forwarded name plus its args re-parse through
                // the clap definitions — a single source of truth for
                // the action surface. A malformed forward degrades to
                // a plain toggle, never an error surface.
                let mut words = vec![action];
                words.extend(args);
                let parsed = parse_forwarded_action(&words).unwrap_or_default();
                Task::done(cosmic::Action::App(Message::ToggleAction(parsed)))
            }
            Details::Open { .. } => Task::none(),
        }
    }

    /// Serves the V1 state interface on the bus name single-instance
    /// activation already owns. The snapshot is seeded from the
    /// app's current fields, so a first read is correct even before
    /// any later sync lands.
    fn dbus_connection(&mut self, conn: zbus::Connection) -> Task<Self::Message> {
        let state = self.derive_dbus_state();
        *self
            .dbus_shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
        self.dbus_last = state;
        self.dbus_conn = Some(conn.clone());
        let shared = self.dbus_shared.clone();
        // Serving is async and nothing waits on it: property reads
        // through the object server simply don't find the interface
        // until it lands, and a sync raced in front of it fails its
        // emission lookup, logs, and is otherwise harmless.
        tokio::spawn(async move {
            match conn
                .object_server()
                .at(dbus_state::OBJECT_PATH, dbus_state::V1::new(shared))
                .await
            {
                Ok(true) => {}
                Ok(false) => log::warn!("V1 state interface already served"),
                Err(err) => log::warn!("failed to serve V1 state interface: {err}"),
            }
        });
        Task::none()
    }
}

impl DropTerm {
    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleAction(action) => return self.debounced_toggle(action),
            Message::ConfigChanged(config) => {
                let old_geom = Geometry::from_config(&self.config);
                let new_geom = Geometry::from_config(&config);
                let old_preference = self.config.theme_preference;
                let old_title_mode = self.config.tab_title_mode;
                let old_profiles = self.config.profiles.clone();
                self.config = config;
                // The live bind map follows every config change — our
                // own write's echo and an external edit land here alike
                // (keybinds-editor criterion 2: a hand-edited config
                // while running stays consistent).
                self.key_binds = keybinds::effective_key_binds(&self.config.custom_key_binds);
                self.apply_config_to_tabs();
                // The config echo is the last listed piggyback point
                // for the band's process snapshot (criterion 5) — the
                // band's own visibility/font ride this same echo.
                self.refresh_status_snapshot();
                // The clock's gate can flip on any echo; sync (arm or
                // stand down) before the echo's own exits, chaining
                // the task onto whichever return it takes.
                let clock_task = self.sync_status_clock();
                // A tab-title-mode flip retitles every live tab from
                // stored state, renamed ones excepted; while in
                // Profile mode a profiles-list change does too (a
                // stale name after a rename is a bug, not a nicety).
                // The delta guard keeps every unrelated echo — our
                // own writes included — from churning titles.
                if echo_retitles_tabs(
                    old_title_mode,
                    self.config.tab_title_mode,
                    &old_profiles,
                    &self.config.profiles,
                ) {
                    self.retitle_all_tabs();
                }
                // A changed theme preference overrides (or restores)
                // the process theme. Sits after
                // `apply_config_to_tabs` — cells are already correct
                // in this pass through `effective_is_dark` — and
                // before the geometry exits below: a preference echo
                // can never be a geometry own-write (`last_written`
                // records geometry writes only), so the two returns
                // cannot collide. The task chains a `ReapplyColors`
                // for the one global-reading consumer, the
                // window-background derivation.
                if let Some(kind) = theme_override(old_preference, self.config.theme_preference) {
                    return theme_override_task(kind).chain(clock_task);
                }
                // Our own write's echo: geometry matching the last
                // write is absorbed — the in-memory config already
                // agrees (updated at release), so no rewrite and no
                // reshape; only the record is cleared. A stale
                // record that never matches is harmless: an echo
                // matching all four fields needs no reshape anyway.
                if let Some(written) = self.last_written
                    && geometry::is_own_write(written, new_geom)
                {
                    self.last_written = None;
                    return clock_task;
                }
                // Geometry edits reshape a shown surface in place —
                // no destroy/recreate; a hidden surface just opens
                // with the new geometry. Non-geometry edits leave
                // the surface alone. A geometry edit that arrives
                // mid-drag is deferred until release, so config
                // reshapes and drag deltas never interleave
                // `set_size` calls on one surface; the release's
                // apply picks the config's other fields up (see
                // `GrabBarReleased`). A geometry edit arriving
                // mid-slide skips the in-place reshape too —
                // geometry stays frozen while animating; the
                // in-memory config above already updated, so the
                // next show opens with the edit applied (a 130 ms
                // exposure window, cheaper than a deferred-reshape
                // queue). `config_edit_reshapes` keys on the slide's
                // existence, not its direction, so this holds
                // mid-slide-out and mid-reversal alike.
                if config_edit_reshapes(
                    geometry::geometry_changed(old_geom, new_geom),
                    self.drag.is_some(),
                    self.slide.is_some(),
                ) && let (SurfaceState::Shown(id), Some(output)) =
                    (&self.surface, self.output_size)
                {
                    let id = *id;
                    // Declare the reshape's request for the
                    // stale-echo gate before issuing it.
                    self.request_geometry_height(output, new_geom);
                    return geometry::apply_geometry(id, new_geom, output).chain(clock_task);
                }
                return clock_task;
            }
            Message::ReapplyColors => self.reapply_colors(),
            Message::TermEventTx(tx) => {
                if self.term_event_tx.is_none() {
                    self.term_event_tx = Some(tx);
                    self.ensure_tabs();
                    // A first-launch remote new-tab stashed while the
                    // channel was missing applies now, after the
                    // session restore has landed — with the placement
                    // the config holds at replay time.
                    if let Some(request) = self.pending_remote_tab.take() {
                        return self.spawn_remote_tab(request, self.config.new_tab_after_current);
                    }
                }
            }
            Message::TermEvent(tab_id, event) => return self.handle_term_event(tab_id, event),
            Message::SurfaceOpened(id, size) => {
                if self.surface == SurfaceState::Measuring(id) {
                    // The measuring pass stretched across the output, so
                    // its first `Opened` size *is* the output size. Keep
                    // it: every later reshape recomputes pixels from it
                    // without a new measuring pass.
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "rounded surface px are non-negative and far below u32::MAX"
                    )]
                    let output = (size.width.round() as u32, size.height.round() as u32);
                    self.output_size = Some(output);
                    let geom = Geometry::from_config(&self.config);
                    self.surface = SurfaceState::Shown(id);
                    // Arm the pin at the height the reshape below
                    // requests — before the tasks run, so their
                    // granted-size echoes meet an armed pin. The
                    // stale echo carries the measuring pass's
                    // full-output height; onto an empty pin it
                    // adopts immediately and grows the grid once
                    // per toggle (toggle-grid-stability).
                    self.prearm_show_pin(output, geom);
                    // The show arm's refresh (status-glance criterion
                    // 4): a landing surface narrates the present, not
                    // whatever the last piggyback point saw — the
                    // snapshot re-reads the lone `/proc` + `.git`
                    // pair and the clock re-stores, before the first
                    // frame's view assembly. One read pair per
                    // landing, the sanctioned cadence; the tick's own
                    // arming is unchanged (assumption 7 — the loop
                    // runs while hidden too, so `sync_status_clock`
                    // only re-stores the text).
                    self.refresh_status_snapshot();
                    let clock_task = self.sync_status_clock();
                    // The slide-in starts here, with the shown
                    // surface: the full surface height is both the
                    // from-offset and the distance — deterministic
                    // and known at open time, unlike the granted
                    // window height, which arrives later through the
                    // double-reported `WindowResized` stream. The
                    // slide toggle gates this entry: disabled, the
                    // surface opens at rest with a byte-equal
                    // render.
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "output px are < 2^24, exact in f32"
                    )]
                    let distance_px =
                        geometry::surface_height(output.1 as f32, geom.height_percent) as f32;
                    self.slide =
                        slide::show_slide(self.config.slide_animation_enabled, distance_px).map(
                            |active| slide::Playing {
                                started: std::time::Instant::now(),
                                active,
                            },
                        );
                    return Task::batch([
                        geometry::apply_geometry(id, geom, output),
                        set_layer(id, surface::wlr_layer(self.config.layer)),
                        set_keyboard_interactivity(
                            id,
                            surface::keyboard_interactivity(self.config.keyboard_mode),
                        ),
                        set_exclusive_zone(id, surface::exclusive_zone(self.config.respect_panel)),
                        widget::text_input::focus(self.terminal_id.clone()),
                        clock_task,
                    ]);
                }
            }
            Message::WindowResized(id, height) => {
                // Only the tracked layer surface's granted stream
                // feeds the pin (`resize_acts_on`): the settings
                // window resizes too and its heights must never pin
                // the terminal's.
                //
                // While a drag is armed, the granted size only echoes
                // the applies the drag itself made — each one
                // double-reported, once with the previous height (the
                // runtime synthesizes a configure per set_size whose
                // height slot is stale), so following the echoes
                // would bounce the pin stale/new on every move: the
                // applies offer their targets directly instead.
                // Outside a drag, every granted height is offered
                // through the adoption debounce — each size arrives
                // twice and the applies arrive at cadence rate, and
                // reflowing the terminal on every echo made the drag
                // sluggish on hardware.
                if resize_acts_on(&self.surface, id)
                    && self.drag.is_none()
                    && let Some(task) = self.offer_window_height(height)
                {
                    return task;
                }
            }
            Message::WindowPinDue => {
                // Trailing adopt: a height the debounce skipped lands
                // once the granted stream settles. The timer waited
                // out the interval, so the gate passes; a stale fire
                // with nothing pending does nothing.
                if let Some(height) = self.pending_height
                    && let Some(task) = self.offer_window_height(height)
                {
                    return task;
                }
            }
            Message::LayerFocused(id) => {
                if self.surface == SurfaceState::Shown(id) {
                    self.surface_focused = true;
                    if let Some(tab) = self.active() {
                        tab.terminal.lock().unwrap().set_focused(true);
                    }
                    return widget::text_input::focus(self.terminal_id.clone());
                }
            }
            Message::LayerUnfocused(id) => {
                if layer_unfocused_acts_on(&self.surface, id) {
                    return self.focus_lost();
                }
            }
            Message::KeyPressed(modifiers, key, physical_key) => {
                // An armed capture owns the keyboard ahead of everything
                // else — the rename guard, the drawer's Escape rule, bind
                // matching — so while armed, capture consumes every press
                // (keybinds-editor criterion 3): Escape cancels, bare
                // modifiers stay armed, any other chord commits through
                // the whole-map write. Disarmed the gate is transparent
                // and routing is exactly as before.
                if let Some(verdict) = capture_verdict_for(self.keybind_capture, modifiers, &key) {
                    match verdict {
                        keybinds::CaptureVerdict::Cancel => self.keybind_capture = None,
                        keybinds::CaptureVerdict::Ignore => {}
                        keybinds::CaptureVerdict::Commit(binding) => {
                            let action = self
                                .keybind_capture
                                .expect("the gate only fires on an armed capture");
                            self.keybind_capture = None;
                            // The conflict guard before any write
                            // (keybinds-editor criterion 4): only a
                            // free chord persists; the action's own
                            // chord disarms silently, a held chord
                            // shows the inline error and writes
                            // nothing.
                            match keybinds::commit_verdict(
                                action,
                                &binding,
                                &self.config.custom_key_binds,
                            ) {
                                keybinds::CommitVerdict::Commit => {
                                    let write =
                                        settings::KeybindsWrite(keybinds::with_custom_binding(
                                            &self.config.custom_key_binds,
                                            action,
                                            binding,
                                        ));
                                    self.write_keybinds_config(&write);
                                }
                                keybinds::CommitVerdict::Own => {}
                                keybinds::CommitVerdict::Conflict(holder) => {
                                    self.keybind_conflict = Some((action, holder));
                                }
                                keybinds::CommitVerdict::Unsupported => {
                                    log::warn!("captured chord cannot be stored; dropping capture");
                                }
                            }
                        }
                    }
                    return Task::none();
                }
                // While an inline rename is active the rename input
                // owns the keyboard: without this guard a bound combo
                // like Ctrl+Shift+V would act on the terminal in
                // addition to the input. The input handles Enter and
                // Escape itself; the binds resume once the rename
                // ends.
                if self.rename.is_some() {
                    return Task::none();
                }
                // The pending close confirmation owns the keyboard
                // ahead of the drawer's Escape rule (close-confirm
                // criterion 4): Escape cancels the modal — only it,
                // the drawer beneath stays open — and every other
                // press is swallowed while the decision waits
                // (pointer blocking comes free from the scrim).
                match modal_key_routing(self.pending_close.is_some(), &key) {
                    ModalKeyRouting::Cancel => {
                        return self.cancel_pending_close();
                    }
                    ModalKeyRouting::Swallow => return Task::none(),
                    ModalKeyRouting::FallThrough => {}
                }
                // The drawer-first Escape rule (spec criterion 6):
                // with the drawer open, Escape closes it — only
                // that. The press never reaches the terminal and the
                // surface does not hide; with the drawer closed the
                // pure rule returns `FallThrough` and the bind match
                // below runs exactly as today. An open scheme picker
                // owns the first Escape (the keybind-capture rule):
                // its overlay publishes the close, and this routing
                // must not also close the drawer — a later Escape
                // does. An open profile draft owns the next Escape
                // (the draft-first rule): the draft cancels — no
                // write, the selection kept — and the drawer stays
                // open for the plain close on a later press.
                let drawer_escapable = self.settings_open && self.scheme_picker.is_none();
                match drawer_escape_routing(drawer_escapable, self.profile_draft.is_some(), &key) {
                    DrawerKeyRouting::CancelDraft => {
                        self.profile_draft = None;
                        return Task::none();
                    }
                    DrawerKeyRouting::CloseDrawer => return self.close_settings_drawer(),
                    DrawerKeyRouting::FallThrough => {}
                }
                if let Some(action) =
                    keybinds::matched_action(&self.key_binds, modifiers, &key, Some(&physical_key))
                {
                    return self.perform_action(action);
                }
            }
            Message::Action(action) => {
                // Context-menu picks reuse the keybind entry point
                // above, so an item behaves exactly like its keybind.
                return self.perform_action(action);
            }
            Message::ContextMenu(open) => {
                // The open-state latch: the press handler arms it,
                // every close path clears it, and the view reads it
                // through `open_trees`.
                self.context_menu_open = open;
            }
            Message::Surface(action) => {
                // Popup plumbing: the wrapper's popup create/destroy
                // requests, handed to libcosmic untouched — the
                // runtime owns popup surfaces.
                return cosmic::task::message(cosmic::Action::Surface(action));
            }
            Message::WindowFocused => {
                self.surface_focused = true;
                if let Some(tab) = self.active() {
                    tab.terminal.lock().unwrap().set_focused(true);
                }
            }
            Message::WindowUnfocused => return self.focus_lost(),
            Message::MiddleClick => {
                return clipboard::read_primary()
                    .map(|text| cosmic::Action::App(Message::ClipboardPastedPrimary(text)));
            }
            Message::TabSelected(id) => {
                // The bar reports the stable tab id; resolve it against
                // the current list so a list change between redraw and
                // click cannot act on the wrong tab.
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    return self.set_active_tab(index);
                }
            }
            Message::TabClosed(id) => {
                // The tab-bar `×` and bar middle-click ride the
                // gated front door like every user-initiated close
                // (close-confirm criterion 3); `TermEvent::Exit`
                // keeps calling `close_tab_at` directly and never
                // confirms.
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    return self.request_close_tab(index);
                }
            }
            Message::StatusSegmentCopied(id) => {
                // The widget already wrote the clipboard; the app owns
                // the flash's life — show the check, clear on lapse.
                self.copied_segment = Some(id);
                return Task::perform(tokio::time::sleep(status_bar::COPY_FLASH), |()| {
                    cosmic::Action::App(Message::StatusCopyFlashLapse)
                });
            }
            Message::StatusCopyFlashLapse => {
                self.copied_segment = None;
            }
            Message::StatusClockTick => {
                // The fired tick consumed its sleep; the re-sync arms
                // the next minute while the gate holds and lets the
                // loop die within one minute of it dropping.
                self.clock_armed = false;
                return self.sync_status_clock();
            }
            Message::CloseConfirmCancel => {
                return self.cancel_pending_close();
            }
            Message::CloseConfirmProceed(id) => {
                // Stale guard (criterion 4): the modal can outlive
                // its tab — the shell exited on its own, a faster
                // close landed — so resolve by id and no-op on a
                // gone tab. A live id closes through today's
                // unchanged path.
                self.pending_close = None;
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    return self.close_tab_at(index);
                }
            }
            Message::SwitchTabProfile(id, profile_index) => {
                return self.switch_tab_profile(id, profile_index);
            }
            Message::RenameStarted(id) => {
                return self.start_rename(id);
            }
            Message::RenameDraftChanged(value) => {
                if let Some(edit) = &mut self.rename {
                    edit.draft = value;
                }
            }
            Message::RenameCommitted => return self.commit_pending_rename(),
            Message::RenameCancelled => {
                self.rename = None;
                return widget::text_input::focus(self.terminal_id.clone());
            }
            Message::TabMoved(id, to) => {
                // The bar reports the drop as (stable id, target index),
                // resolved against the current list so a list change
                // between redraw and release cannot move the wrong tab.
                // Reordering never changes which tab is active, so no
                // focus handoff is needed.
                if let Some(from) = self.tabs.iter().position(|tab| tab.id == id) {
                    self.active_tab = tabs::reorder_tabs(&mut self.tabs, self.active_tab, from, to);
                    // The active tab's identity survives a reorder; the
                    // refresh is the uniform every-assignment rule, cheap
                    // and idempotent here.
                    self.refresh_active_window_bg();
                    // The saved tab order is tab-bar order, so a
                    // reorder is a save trigger.
                    return self.mark_session_dirty();
                }
            }
            Message::DuplicateTab(id) => {
                // The per-tab menu's duplicate rides the shared source
                // read; an unresolvable id — a tab closed between open
                // and pick — is a quiet no-op (the TabSelected rule).
                return self.duplicate_tab(id);
            }
            Message::PinToggled(id) => {
                return self.toggle_pin(id);
            }
            Message::TabMove(id, direction) => {
                return self.move_tab_by(id, direction);
            }
            Message::CloseOtherTabs(id) => {
                return self.close_tabs_sweep(id, tabs::CloseSweep::Others);
            }
            Message::CloseTabsRight(id) => {
                return self.close_tabs_sweep(id, tabs::CloseSweep::Right);
            }
            Message::GrabBarDragged(pointer_y) => {
                // The drag handlers (`GrabBarDragged`, `DragApplyDue`,
                // `GrabBarReleased`) all match `Shown` exclusively,
                // so a `Hiding` surface ignores every drag message by
                // construction — no drag can interleave with an
                // out-slide or its reversal. A slide-in, though,
                // rides a `Shown` surface, so `Shown` alone cannot
                // gate it: `grab_bar_acts_on` additionally requires
                // the slide to be idle (a press inside the 130 ms
                // window neither arms a drag nor resizes the
                // animating surface; the pointer-to-content mapping
                // is offset mid-slide anyway).
                //
                // Live drag: arm on the first move — anchored at the
                // height the surface has right now — then retarget
                // absolutely per move in pixels: one pixel of pointer
                // travel is one pixel of surface, and the target
                // recomputes from the height actually applied, so the
                // moving window frame of a bottom-anchored surface
                // cannot eat travel. Width and alignment always come
                // from the config (a drag can never snap a narrow
                // surface to fill). Applies pass a deadbanded,
                // interval-gated cadence (geometry::drag_apply_due) —
                // every apply reaches the app twice (the runtime
                // double-reports each set_size) and reflows the
                // terminal, so a fast drag cannot be allowed to storm
                // the pipeline; skipped targets land via a trailing
                // apply, and the release settle is exact and
                // immediate. Config is never touched while dragging;
                // persistence is the release path.
                if grab_bar_acts_on(&self.surface, self.slide.is_some())
                    && let (SurfaceState::Shown(id), Some(output)) =
                        (&self.surface, self.output_size)
                {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "output px are < 2^24, exact in f32"
                    )]
                    let anchored_px = geometry::surface_height(
                        output.1 as f32,
                        Geometry::from_config(&self.config).height_percent,
                    );
                    let drag = self.drag.get_or_insert(Drag {
                        anchor: geometry::DragAnchor {
                            anchored_px,
                            applied_px: anchored_px,
                            press_y: pointer_y,
                        },
                        pointer_y,
                        applied: None,
                    });
                    drag.pointer_y = pointer_y;
                    let target = geometry::drag_target_px(
                        self.config.edge,
                        output.1,
                        drag.anchor,
                        pointer_y,
                    );
                    if geometry::drag_apply_due(drag.applied, std::time::Instant::now(), target) {
                        return self.apply_drag_target(*id, output, target);
                    }
                    // Throttled: land the latest target even if the
                    // pointer pauses inside the interval. A stale fire
                    // is a no-op — the apply path re-checks the gate.
                    return Task::perform(
                        tokio::time::sleep(geometry::DRAG_APPLY_INTERVAL),
                        |()| cosmic::Action::App(Message::DragApplyDue),
                    );
                }
            }
            Message::DragApplyDue => {
                // Trailing apply of a throttled live drag: recompute
                // from the latest reported pointer position and apply
                // if the gate allows; a fire after the drag ended (or
                // with nothing changed) does nothing.
                if let (SurfaceState::Shown(id), Some(output)) = (&self.surface, self.output_size)
                    && let Some(drag) = self.drag.as_ref()
                {
                    let target = geometry::drag_target_px(
                        self.config.edge,
                        output.1,
                        drag.anchor,
                        drag.pointer_y,
                    );
                    if geometry::drag_apply_due(drag.applied, std::time::Instant::now(), target) {
                        return self.apply_drag_target(*id, output, target);
                    }
                }
            }
            Message::GrabBarReleased => {
                // The settle: the dragged pixels quantize to a whole
                // percent once — the surface may snap by at most half
                // a percent, the accepted tolerance, and then matches
                // the persisted percent. The settle targets where the
                // pointer released (the latest move), not the last
                // target the cadence landed: throttling must never
                // decide the final height. The in-memory config
                // updates immediately — so a hide/show inside the
                // debounce window reopens at the new height — and one
                // write follows after the window, canceled by any
                // newer drag that settles first. Re-applying also
                // flushes a config edit that arrived mid-drag and was
                // deferred (see `ConfigChanged`): width and alignment
                // come from the current config, the height from the
                // drag — the hand outranks the file.
                if let (Some(drag), SurfaceState::Shown(id), Some(output)) =
                    (self.drag.take(), &self.surface, self.output_size)
                {
                    let id = *id;
                    let geom = self.settled_geometry(&drag, output);
                    // Declare the release's request for the
                    // stale-echo gate before issuing it.
                    self.request_geometry_height(output, geom);
                    // Write-only-if-changed: a drag that settles back
                    // on the config's current percent persists nothing.
                    if geom.height_percent != self.config.height_percent {
                        self.config.height_percent = geom.height_percent;
                        let generation = self.write_back.schedule(geom.height_percent).generation;
                        return Task::batch([
                            geometry::apply_geometry(id, geom, output),
                            Task::perform(
                                tokio::time::sleep(geometry::WRITE_BACK_DEBOUNCE),
                                move |()| cosmic::Action::App(Message::WriteDue(generation)),
                            ),
                        ]);
                    }
                    return geometry::apply_geometry(id, geom, output);
                }
            }
            Message::WriteDue(generation) => {
                // The debounce window elapsed: persist the settled
                // height once through [`Self::write_config`] —
                // unless a newer drag rescheduled or a bare settings
                // write canceled (the stale generation finds nothing
                // pending). `PreApplied` because the release already
                // applied the geometry: the write records the written
                // geometry so the resulting `ConfigChanged` echo is
                // absorbed instead of reshaping (see `ConfigChanged`).
                if let Some(pending) = self.write_back.due(generation) {
                    self.write_config(
                        settings::WriteKind::PreApplied,
                        &settings::PlannedWrite::HeightPercent(pending.height_percent),
                    );
                }
            }
            Message::SessionSaveDue(generation) => {
                // The debounce window elapsed: capture the live tabs
                // now and write atomically — cwds are read at write
                // time, so a window always persists what is true at
                // its close, not what was true when it was armed. A
                // stale generation — superseded, or a duplicate
                // delivery after the write — writes nothing. The
                // session toggle gates the write itself: a window
                // armed before a mid-run disable fires into a
                // no-write, but the machine still clears, so no
                // zombie window ever blocks the next armed one.
                if self
                    .session_save
                    .due_writes(generation, self.config.session_persistence_enabled)
                {
                    self.write_session();
                }
            }
            Message::SlideTick => {
                // Redraw-only clock: the eased offset is evaluated at
                // draw time inside `SlideBox`, so the tick's only job
                // is parking the slide once the duration has fully
                // elapsed — which also drops the tick subscription
                // until the next slide. Between completion and this
                // parking tick the draw already clamps to exactly the
                // endpoint, so there is no pop.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
                )]
                if let Some(playing) = self.slide
                    && slide::finished(
                        &playing.active,
                        playing.started.elapsed().as_millis() as u64,
                    )
                {
                    self.slide = None;
                    // The animated hide's destroy point: an `Out`
                    // slide's completion is the only path that
                    // destroys a hiding surface — the app's single
                    // `destroy_layer_surface` call for shown
                    // surfaces, reached through the state machine's
                    // completion event. Teardown note: app teardown
                    // has no graceful path — process exit destroys
                    // the surface compositor-side, inherently
                    // immediate; any future quit action must bypass
                    // the slide and destroy straight away, like this
                    // does.
                    if let SurfaceState::Hiding(id) = self.surface {
                        self.surface = SurfaceState::Hidden;
                        return destroy_layer_surface(id);
                    }
                }
            }
            Message::OpenHyperlink(url) => {
                if let Err(err) = open::that_detached(&url) {
                    log::warn!("failed to open {url:?}: {err}");
                }
            }
            Message::ClipboardPasted(text) | Message::ClipboardPastedPrimary(text) => {
                if let (Some(text), Some(tab)) = (text, self.active()) {
                    tab.terminal.lock().unwrap().paste(&text);
                }
            }
            Message::PickSettingsTab(entity) => {
                // The strip's row → its tab (entity↔tab lockstep,
                // criterion 7), then the page-turn rule: the page
                // turns, nothing else — no drawer close, no settings
                // buffer clear, no `DrawerTransition` — and the
                // model's selection follows the page. An entity the
                // model does not know (impossible while rows are only
                // `SettingsTab::ALL`) resolves to nothing: no turn,
                // no invented tab.
                if let Some(tab) = self.settings_tab_model.data::<SettingsTab>(entity) {
                    self.turn_settings_page(*tab);
                }
            }
            Message::Settings(event) => {
                // The draft lock (profile-manager): while a profile
                // draft is open, no list mutation and no draft switch
                // is reachable — the pure rule the disabled view
                // renders and this gate enforces, so a stale press (a
                // race between a render and a click) is a no-op even
                // if a control flashed enabled.
                if !settings::profile_event_reachable(&event, self.profile_draft.is_some()) {
                    return Task::none();
                }
                // The settings drawer's commit path: every event
                // resolves through a pure planner and lands as one
                // `Bare` write via [`Self::write_config`] — the
                // drawer never applies anything itself, the
                // `ConfigChanged` echo drives every apply.
                // Display-only events (drafts, slider moves) touch
                // just the buffer; commits that fail to parse are
                // no-ops that leave the draft for the user to fix.
                match event {
                    settings::view::SettingsEvent::Draft(field, text) => {
                        self.settings_buffer.set_text(field, text);
                    }
                    settings::view::SettingsEvent::Submitted(field, text) => {
                        if let Some(write) = settings::submit_write(field, &text) {
                            // A parsed submit clears its draft so the
                            // input falls back to the echoed config
                            // value; a rejected one keeps the draft.
                            self.settings_buffer.clear_text(field);
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::view::SettingsEvent::SliderMoved(key, value) => {
                        self.settings_buffer.set_slider(key, value);
                    }
                    settings::view::SettingsEvent::SliderReleased(key) => {
                        // `on_release` carries no value (the pinned
                        // rev's API), so the release commits the
                        // buffered position the readout showed.
                        let value = self.settings_buffer.slider_value(key, &self.config);
                        if let Some(write) = settings::slider_release_write(key, value) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::view::SettingsEvent::Picked(key, index) => {
                        // Shell's dropdown is dynamic — its options are
                        // built per view from `/etc/shells` — so the
                        // pick resolves through the dynamic planner,
                        // which rebuilds the list rather than trusting
                        // the open dropdown's snapshot.
                        let write = if key == settings::ConfigKey::Shell {
                            settings::shell_pick_write(index)
                        } else {
                            settings::dropdown_write(key, index)
                        };
                        if let Some(write) = write {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                        // A scheme pick closes its picker: the
                        // popover's dismissal contract, applied where
                        // the pick lands.
                        if matches!(
                            key,
                            settings::ConfigKey::ColorSchemeDark
                                | settings::ConfigKey::ColorSchemeLight
                        ) {
                            self.scheme_picker = None;
                        }
                    }
                    settings::view::SettingsEvent::ToggleSchemePicker(id) => {
                        // One picker at most: pressing another scheme
                        // control swaps, pressing the open one closes.
                        self.scheme_picker = (self.scheme_picker != Some(id)).then_some(id);
                    }
                    settings::view::SettingsEvent::CloseSchemePicker => {
                        self.scheme_picker = None;
                    }
                    settings::view::SettingsEvent::Toggled(key, checked) => {
                        if let Some(write) = settings::toggle_write(key, checked) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::view::SettingsEvent::KeybindSearchChanged(text) => {
                        // Display only: the next view build re-filters
                        // the Keybindings tab's rows.
                        self.keybind_search = text;
                    }
                    settings::view::SettingsEvent::CaptureStart(action) => {
                        // Arm the capture: from here the KeyPressed
                        // arm's first gate owns every press. A fresh
                        // arm clears any standing conflict error.
                        self.keybind_capture = Some(action);
                        self.keybind_conflict = None;
                    }
                    settings::view::SettingsEvent::CaptureCancel => {
                        self.keybind_capture = None;
                    }
                    settings::view::SettingsEvent::RevertKeybind(action) => {
                        // Revert: drop the action's custom entry — its
                        // defaults come back when the echo rebuilds the
                        // live map, effective on the next key press.
                        self.keybind_conflict = None;
                        let write = settings::KeybindsWrite(keybinds::without_custom_binding(
                            &self.config.custom_key_binds,
                            action,
                        ));
                        self.write_keybinds_config(&write);
                    }
                    settings::view::SettingsEvent::SelectProfile(index) => {
                        // The view emits live indices only; the bound
                        // guards the echo race where the list shrank
                        // between render and press. Selection seeds
                        // the edit form (one draft at a time).
                        if index < self.config.profiles.len() {
                            self.profile_selection = Some(index);
                            self.profile_draft = Some(settings::ProfileDraft::edit(
                                index,
                                &self.config.profiles[index],
                            ));
                        }
                    }
                    settings::view::SettingsEvent::PickDefaultProfile(index) => {
                        // Commit-on-change through the module's one
                        // persist path; the `ConfigChanged` echo
                        // updates the in-memory config the next view
                        // build reads. A stale index rejects inside
                        // the planner — no write, no invented value.
                        if let Some(write) =
                            settings::default_picker_write(&self.config.profiles, index)
                        {
                            self.write_profile_config(&write);
                        }
                    }
                    settings::view::SettingsEvent::OpenProfileDraft => {
                        self.profile_draft = Some(settings::ProfileDraft::add());
                    }
                    settings::view::SettingsEvent::CloneProfile(index) => {
                        // Seeding only — no write until the draft
                        // commits.
                        if let Some(source) = self.config.profiles.get(index) {
                            self.profile_draft = Some(settings::ProfileDraft::clone_of(
                                source,
                                &self.config.profiles,
                            ));
                        }
                    }
                    settings::view::SettingsEvent::ArmProfileDelete(index) => {
                        // The two-step delete's first press: the row
                        // trades its trash for the inline Confirm/Keep
                        // pair. Single-row arming lives in the pure
                        // state machine — arming another row disarms
                        // the first.
                        self.profile_delete_armed = settings::armed_profile_delete_row(
                            settings::ProfileDeletePress::Arm(index),
                        );
                    }
                    settings::view::SettingsEvent::KeepProfileDelete => {
                        // The pair's retreat: disarm, delete nothing.
                        self.profile_delete_armed =
                            settings::armed_profile_delete_row(settings::ProfileDeletePress::Keep);
                    }
                    settings::view::SettingsEvent::ConfirmProfileDelete(index) => {
                        // Confirm deletes — one whole-list write, plus
                        // the paired auto-clear when the deleted
                        // profile was the default — and the arm ends
                        // with it. Page state follows the shrunken
                        // list through the pure adjusters; a stale
                        // index plans nothing and disarms anyway.
                        if let Some((list, paired)) = settings::plan_profile_delete(
                            &self.config.profiles,
                            index,
                            self.config.default_profile.as_deref(),
                        ) {
                            self.write_profile_config(&settings::ProfileWrite::Profiles(list));
                            if let Some(clear) = paired {
                                self.write_profile_config(&clear);
                            }
                            self.profile_selection =
                                settings::selection_after_delete(self.profile_selection, index);
                            self.profile_draft =
                                settings::draft_after_delete(self.profile_draft.take(), index);
                        }
                        self.profile_delete_armed = settings::armed_profile_delete_row(
                            settings::ProfileDeletePress::Confirm(index),
                        );
                    }
                    settings::view::SettingsEvent::ReorderProfile(index, direction) => {
                        // One slot per press, one whole-list write;
                        // the selection follows the moved profile.
                        // The new order is the `+` dropdown /
                        // context-menu order on their next open.
                        if let Some(list) =
                            settings::plan_profile_reorder(&self.config.profiles, index, direction)
                        {
                            self.write_profile_config(&settings::ProfileWrite::Profiles(list));
                            self.profile_selection = settings::selection_after_move(
                                self.profile_selection,
                                index,
                                direction,
                            );
                            // An open edit draft names the same
                            // profile by slot: the swap trades its
                            // index exactly like the selection's.
                            if let Some(settings::ProfileDraftMode::Edit {
                                index: edited_index,
                            }) = self.profile_draft.as_ref().map(|draft| &draft.mode)
                            {
                                let followed = settings::selection_after_move(
                                    Some(*edited_index),
                                    index,
                                    direction,
                                );
                                if let Some(draft) = self.profile_draft.as_mut() {
                                    draft.mode = settings::ProfileDraftMode::Edit {
                                        index: followed.expect("a move never clears an edit index"),
                                    };
                                }
                            }
                        }
                    }
                    settings::view::SettingsEvent::DraftProfileField(field, text) => {
                        // Display-only buffering; the keystroke also
                        // clears any stale error.
                        if let Some(draft) = self.profile_draft.as_mut() {
                            draft.set_field_text(field, text);
                        }
                    }
                    settings::view::SettingsEvent::SaveProfileDraft => {
                        // The transactional commit: one Save, one
                        // plan, at most one whole-list write plus
                        // the paired rename cascade — then the form
                        // closes on the row it wrote. An invalid
                        // Save writes nothing and keeps the form
                        // open with every offending field erroring
                        // at once; an unchanged draft closes clean.
                        if let Some(draft) = self.profile_draft.clone() {
                            // Pre-append length: the selection the
                            // Write leaves behind (an Add/Clone
                            // selects the row its write creates).
                            let pre_append_len = self.config.profiles.len();
                            match settings::plan_profile_save(
                                &draft,
                                &self.config.profiles,
                                self.config.default_profile.as_deref(),
                            ) {
                                settings::ProfileSavePlan::Write {
                                    profiles,
                                    default_after_rename,
                                } => {
                                    // `profiles` first, then the
                                    // cascade — the spec's two-key
                                    // order; read-side tolerance
                                    // covers the crash window.
                                    self.write_profile_config(&settings::ProfileWrite::Profiles(
                                        profiles,
                                    ));
                                    if let Some(new_default) = default_after_rename {
                                        self.write_profile_config(
                                            &settings::ProfileWrite::DefaultProfile(Some(
                                                new_default,
                                            )),
                                        );
                                    }
                                    self.profile_selection = Some(settings::selection_after_save(
                                        &draft.mode,
                                        pre_append_len,
                                    ));
                                    self.profile_draft = None;
                                }
                                settings::ProfileSavePlan::Invalid { errors } => {
                                    if let Some(open) = self.profile_draft.as_mut() {
                                        open.errors = errors;
                                    }
                                }
                                settings::ProfileSavePlan::Unchanged => {
                                    // The empty transaction: clean
                                    // close, no write.
                                    self.profile_draft = None;
                                }
                                settings::ProfileSavePlan::StaleEdit => {
                                    // The edited profile is gone: the
                                    // draft (and its selection) drop.
                                    self.profile_selection = None;
                                    self.profile_draft = None;
                                }
                            }
                        }
                    }
                    settings::view::SettingsEvent::CancelProfileDraft => {
                        // Cancel drops the draft with no write; the
                        // selection persists — the row still names
                        // a profile for the next pencil press.
                        self.profile_draft = None;
                    }
                    settings::view::SettingsEvent::PickProfileScheme(pick) => {
                        // The pick buffers in every mode — the
                        // recolor lands on Save with the rest of the
                        // draft — and retires the scheme's stale error
                        // (it described a value the pick just
                        // replaced). A pick then closes its popover:
                        // the dismissal contract, applied where the
                        // pick lands. An out-of-range pick is ignored
                        // — the rows are the only writable vocabulary,
                        // never an invented name.
                        if let Some(pick) = settings::profile_scheme_write(pick)
                            && let Some(open) = self.profile_draft.as_mut()
                        {
                            open.scheme = pick.field_value();
                            open.errors
                                .retain(|(at, _)| *at != settings::ProfileField::Scheme);
                        }
                        if matches!(
                            self.scheme_picker,
                            Some(settings::SchemePickerId::ProfileForm)
                        ) {
                            self.scheme_picker = None;
                        }
                    }
                    settings::view::SettingsEvent::AddProfileEnvRow => {
                        // A buffered list edit, not a write: the row
                        // joins the draft (an Enter in any env input
                        // writes the whole map); any stale error
                        // clears — the row list it described is gone.
                        if let Some(draft) = self.profile_draft.as_mut() {
                            draft.env_rows.push(settings::ProfileEnvRow::default());
                            draft.errors.clear();
                        }
                    }
                    settings::view::SettingsEvent::RemoveProfileEnvRow(index) => {
                        // Same buffering rule: the row leaves the
                        // draft, the write rides the next env Enter.
                        // A stale index (the row already gone) is a
                        // no-op; the error always clears — removal
                        // shifts row indices, so an error pointing
                        // into the old list can no longer be trusted.
                        if let Some(draft) = self.profile_draft.as_mut()
                            && index < draft.env_rows.len()
                        {
                            draft.env_rows.remove(index);
                            draft.errors.clear();
                        }
                    }
                    settings::view::SettingsEvent::DrawerResize(pointer_x) => {
                        // Live width (criterion 2): display-only while
                        // the press is held — the same band render
                        // clamps through, so the drag can never leave
                        // it. Nothing writes mid-drag.
                        self.drawer_drag = Some(settings::drawer_drag_px(
                            pointer_x,
                            self.current_surface_width(),
                        ));
                    }
                    settings::view::SettingsEvent::DrawerResizeReleased => {
                        // One write per drag (criterion 3), and only a
                        // real one: a plain click armed no live value,
                        // and a drag landing back on the stored px
                        // rewrites nothing.
                        let write = settings::drawer_release_write(
                            self.drawer_drag,
                            self.config.settings_drawer_width,
                        );
                        self.drawer_drag = None;
                        if let Some(write) = write {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::view::SettingsEvent::DrawerReset => {
                        // Double-click (criterion 5): back to the
                        // built-in default, stored value cleared.
                        self.drawer_drag = None;
                        self.write_config(
                            settings::WriteKind::Bare,
                            &settings::PlannedWrite::SettingsDrawerWidth(None),
                        );
                    }
                }
            }
        }
        Task::none()
    }
    /// The app's surface state as the V1 interface sees it:
    /// `window::Id` payloads dropped, so no app type crosses into
    /// `dbus_state`. Exhaustive on purpose — a new surface state must
    /// pick a phase here.
    fn surface_phase(&self) -> dbus_state::SurfacePhase {
        match self.surface {
            SurfaceState::Hidden => dbus_state::SurfacePhase::Hidden,
            SurfaceState::Measuring(_) => dbus_state::SurfacePhase::Measuring,
            SurfaceState::Shown(_) => dbus_state::SurfacePhase::Shown,
            SurfaceState::Hiding(_) => dbus_state::SurfacePhase::Hiding,
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "tab count is bounded far below u32::MAX"
    )]
    fn derive_dbus_state(&self) -> dbus_state::State {
        dbus_state::derive(self.surface_phase(), self.tabs.len() as u32)
    }

    /// Re-derives the served snapshot and emits `PropertiesChanged`
    /// for the properties that actually moved. No-op without a bus
    /// connection (headless) and when nothing changed.
    fn sync_dbus_state(&mut self) {
        let Some(conn) = self.dbus_conn.clone() else {
            return;
        };
        let new = self.derive_dbus_state();
        let changed = dbus_state::changed(self.dbus_last, new);
        if !(changed.visible || changed.tab_count) {
            return;
        }
        *self
            .dbus_shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = new;
        self.dbus_last = new;
        // The emission rides a spawned task so `update` never awaits
        // the bus; the single executor serializes messages, so a
        // subscriber may at worst see older-then-newer values, never a
        // wrong final state.
        tokio::spawn(async move {
            let iface_ref = match conn
                .object_server()
                .interface::<_, dbus_state::V1>(dbus_state::OBJECT_PATH)
                .await
            {
                Ok(iface_ref) => iface_ref,
                Err(err) => {
                    log::warn!("V1 state interface lookup failed: {err}");
                    return;
                }
            };
            let iface = iface_ref.get().await;
            if changed.visible
                && let Err(err) = iface.visible_changed(iface_ref.signal_emitter()).await
            {
                log::warn!("failed to emit Visible change: {err}");
            }
            if changed.tab_count
                && let Err(err) = iface.tab_count_changed(iface_ref.signal_emitter()).await
            {
                log::warn!("failed to emit TabCount change: {err}");
            }
        });
    }
}

#[cfg(test)]
mod tests;
