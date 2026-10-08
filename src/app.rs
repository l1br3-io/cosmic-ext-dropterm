// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::{self as aterm};
use alacritty_terminal::vte::ansi::NamedColor;
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
    KeyboardInteractivity, destroy_layer_surface, get_layer_surface, set_exclusive_zone,
    set_keyboard_interactivity, set_layer,
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
use crate::surface;
use crate::tab_bar::{TabBar, TabMenuPick, TooltipSide};
use crate::tabs::{self, Tab, TabId};
use crate::terminal::{self, Action, TerminalBox, TerminalOptions, theme_source};

pub(crate) mod remote;
mod spawn;
mod tab_actions;

use self::remote::{Flags, ToggleAction, parse_forwarded_action, startup_action};
use self::tab_actions::{CloseSweep, RenameEdit, open_trees};

pub const APP_ID: &str = "io.github.l1br3_io.CosmicExtDropterm";

/// Debounce window for toggle requests, guarding against the double
/// key-press events a compositor occasionally delivers for one physical
/// press (see the README's known-risks section).
const TOGGLE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SurfaceState {
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(window::Id),
    Shown(window::Id),
    /// A shown surface sliding out toward its anchored edge: the
    /// content still renders — translated by the live `Out` slide —
    /// and the layer surface is destroyed only when that slide
    /// completes. Keyboard interactivity was already released at
    /// slide-out start, so focus events find no `Shown` surface to
    /// act on; the drag handlers and live reshape ignore this state
    /// by construction (they match `Shown` exclusively).
    Hiding(window::Id),
}

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
    Settings(settings::SettingsEvent),
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

/// An in-progress grab-bar drag's runtime state: the pure anchor
/// every target recomputes from (`geometry`), the latest reported
/// pointer y, and the last live apply (when, which target) for the
/// change-gated apply cadence — see `geometry::drag_apply_due`.
/// Applies are bounded because each one costs the runtime two window
/// resizes plus terminal and pty resizes (its `set_size` handling
/// double-reports each apply), so per-move applies at pointer rate
/// saturate the single runtime thread — the drag froze input on
/// hardware.
struct Drag {
    anchor: geometry::DragAnchor,
    /// The pointer's window y as of the last `GrabBarDragged`, so the
    /// trailing apply and the release settle at where the pointer
    /// actually is, not where the last apply landed.
    pointer_y: f32,
    /// The last live apply, for the cadence gate.
    applied: Option<(std::time::Instant, u32)>,
}

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
    settings_page: SettingsPage,
    /// The tab strip's rows: built once at init
    /// ([`settings::tab_strip_model`]) and never rebuilt per frame —
    /// the strip renders from it, each row carrying its `SettingsTab`
    /// as data. The selection is kept in lockstep with
    /// `settings_page` ([`Self::sync_settings_tab_strip`]); the rows
    /// themselves never change.
    settings_tab_model: settings::TabStripModel,
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

    fn terminal_options(&self) -> TerminalOptions {
        TerminalOptions {
            font_size: self.config.font_size,
            ..TerminalOptions::default()
        }
    }

    /// The global color pick, unchanged by the profile-appearance
    /// chain: plain tabs' colors, every profile tab's fallback (see
    /// [`colors_for`]), and the resolution behind the window
    /// background when no tab exists.
    fn active_colors(&self) -> aterm::color::Colors {
        // The config preference is the source of truth:
        // `effective_is_dark`, not a direct global read, decides the
        // mode, so a pick applies colors in this same pass — before
        // the async `set_theme` task lands and moves the global.
        let is_dark = effective_is_dark(self.config.theme_preference, cosmic::theme::is_dark());
        let scheme_name = if is_dark {
            &self.config.color_scheme_dark
        } else {
            &self.config.color_scheme_light
        };
        theme_source::by_name(scheme_name).unwrap_or_else(|| {
            if is_dark {
                theme_source::cosmic_dark()
            } else {
                theme_source::cosmic_light()
            }
        })
    }

    /// Applies the current config and theme to every tab's terminal.
    /// Colors resolve per tab through the [`colors_for`] chain — a
    /// profile tab runs its profile's scheme, every other tab the
    /// global pick — so one pass can paint a mixed pair differently;
    /// the window background then follows the active tab's
    /// resolution, keeping chrome and unset-bg cells in step with
    /// whichever tab is on screen.
    fn apply_config_to_tabs(&mut self) {
        let options = self.terminal_options();
        let global = self.active_colors();
        for tab in &self.tabs {
            let colors = colors_for(&self.config, &global, tab.profile.as_deref());
            tab.terminal.lock().unwrap().set_config(&options, &colors);
        }
        self.refresh_active_window_bg();
    }

    /// Refreshes the process-global window background from the
    /// ACTIVE tab's resolved colors (the profile-appearance ruling):
    /// the fallback background is one value, so it must belong to
    /// the tab actually on screen — every `active_tab` assignment
    /// and every config pass funnels through here. No tabs
    /// (startup, or momentarily mid-close) resolves the global
    /// pick, the pre-initiative behavior.
    fn refresh_active_window_bg(&self) {
        let global = self.active_colors();
        let colors = colors_for(
            &self.config,
            &global,
            self.active().and_then(|tab| tab.profile.as_deref()),
        );
        Self::refresh_window_bg_color(&colors);
    }

    /// Mirrors cosmic-term's own `update_config`: the effective window
    /// background color is a process-global (`terminal::WINDOW_BG_COLOR`)
    /// used as a fallback wherever a color scheme leaves "background"
    /// unset, so it must be refreshed whenever the active tab's
    /// resolution, the global scheme, or the COSMIC theme changes.
    fn refresh_window_bg_color(colors: &aterm::color::Colors) {
        let theme = cosmic::theme::active();
        let color = cosmic::iced::Color::from(theme.cosmic().background(theme.transparent).base);
        let bytes = color.into_rgba8();
        let system_bg = u32::from(bytes[2])
            | (u32::from(bytes[1]) << 8)
            | (u32::from(bytes[0]) << 16)
            | 0xFF00_0000;
        let data = window_bg_color(colors, system_bg);
        terminal::WINDOW_BG_COLOR.store(data, std::sync::atomic::Ordering::SeqCst);
    }

    fn show(&mut self) -> Task<Message> {
        if !matches!(self.surface, SurfaceState::Hidden) {
            return Task::none();
        }
        // The user can see the terminal again: re-arm every tab's bell
        // notification latch, so a bell from here on is fresh news
        // worth one notification per tab again.
        for tab in &mut self.tabs {
            tab.bell_notified = false;
        }
        self.ensure_tabs();
        let id = window::Id::unique();
        self.surface = SurfaceState::Measuring(id);
        self.surface_focused = false;
        get_layer_surface(surface::measuring(id))
    }

    fn hide(&mut self) -> Task<Message> {
        // Hiding the surface is a focus loss for the rename input:
        // commit before the content slides out.
        let commit = self.commit_pending_rename();
        // The drawer survives the hide (spec criterion 8): its open
        // state and in-flight drafts stay exactly as they were for
        // the next show. The named no-op documents that at the seam
        // a hide would otherwise silently forget; only an explicit
        // close ever clears (see `close_settings_drawer`).
        self.settings_buffer
            .apply_drawer_transition(settings::DrawerTransition::FocusLossHide);
        // The page-local form draft rides the same rule: a hide
        // preserves it (the named no-op; only the transitions in
        // `close_settings_drawer` and `leave_profiles_page` clear).
        settings::apply_profile_draft_transition(
            &mut self.profile_draft,
            settings::ProfileDraftTransition::Hide,
        );
        // The slide toggle feeds the decision: a disabled slide turns
        // this hide into an immediate destroy inside `toggle_step` —
        // the executor arms below then simply never runs.
        let task = match toggle_step(
            ToggleAction::Hide,
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::DestroyNow(id) => {
                // A measuring surface has no reshaped content to
                // animate: destroy immediately, no slide.
                self.slide = None;
                self.surface = SurfaceState::Hidden;
                destroy_layer_surface(id)
            }
            ToggleStep::SlideOut(id) => {
                // A shown surface slides out and its layer is
                // destroyed on completion. The out-slide starts from
                // wherever the content stands — the live eased offset
                // of an in-flight slide-in, or rest — so nothing
                // teleports. Its distance is the same deterministic
                // formula the show used, kept from the in-flight slide
                // when one exists: a config edit that arrived
                // mid-slide updates the config but not the frozen
                // geometry, so the in-flight distance is the one the
                // visible surface still has.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
                )]
                let (from_offset_px, distance_px) = match self.slide {
                    Some(playing) => (
                        slide::offset(
                            &playing.active,
                            playing.started.elapsed().as_millis() as u64,
                        ),
                        playing.active.distance_px,
                    ),
                    None => (0.0, self.slide_distance_px()),
                };
                self.surface = SurfaceState::Hiding(id);
                self.slide = Some(slide::Playing {
                    started: std::time::Instant::now(),
                    active: slide::Active {
                        direction: slide::Direction::Out,
                        from_offset_px,
                        distance_px,
                    },
                });
                // Keyboard interactivity releases at slide-out START,
                // not at destroy: the previously focused window
                // regains focus while the slide plays. The compositor
                // answers with an `Unfocused` layer event, which the
                // `Shown(id)` guard in its handler already rejects —
                // the surface is `Hiding` by the time it arrives.
                set_keyboard_interactivity(id, KeyboardInteractivity::None)
            }
            // `Hide` never decides to show, and a hiding or hidden
            // surface leaves with nothing: no restart, no double
            // destroy.
            ToggleStep::Measure | ToggleStep::ReverseShow(_) | ToggleStep::Leave => Task::none(),
        };
        // The surface is leaving on every destructive path, and the
        // cleanup is idempotent for `Leave`: any live grab-bar drag
        // is moot, the terminal pin's debounce state starts over so
        // the next show's first granted size adopts immediately, and
        // the active terminal keeps its reported focus — the
        // dropdown's own visibility toggle is not a focus event for
        // the PTY (a focus-reporting program — fish, vim, tmux —
        // would repaint its prompt per toggle), and the PTY keeps
        // running regardless, because PTYs live in tabs, not the
        // window.
        self.drag = None;
        self.pending_height = None;
        self.pin_applied = None;
        // The context menu's popup dies with the surface and its
        // `on_close` never fires — clear the latch here, or the next
        // show's first right-click would be consumed as a
        // toggle-close of a menu that is not there.
        self.context_menu_open = false;
        Task::batch([commit, task])
    }

    /// Reverses a hiding slide back to shown: the live surface is
    /// reused — no new measuring pass, `output_size` is retained (the
    /// `geometry` module's guarantee) — the slide re-targets `In`
    /// from the current offset (`slide::reverse`: no teleport, a
    /// fresh fixed 130 ms), keyboard interactivity is re-grabbed per
    /// config, and `surface_focused` resets to `false` pending the
    /// `LayerFocused` the re-grab synthesizes. No focus report
    /// returns here — none left at hide start (the toggle is not a
    /// focus event), so the PTY never noticed the hide and keeps
    /// running throughout.
    fn reverse_show(&mut self, id: window::Id) -> Task<Message> {
        self.surface = SurfaceState::Shown(id);
        self.surface_focused = false;
        // `Hiding` always carries a live `Out` slide — the parking
        // tick destroys the surface and leaves `Hidden` in the same
        // handler — so there is always something to re-target; a
        // slide-less degenerate would simply show at rest. An `Out`
        // slide that already ran past its duration reverses from the
        // endpoint (the full distance), i.e. a fresh full slide-in.
        if let Some(playing) = self.slide {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "elapsed ms of one slide; u64 range is exhausted after 58M years"
            )]
            let reversed = slide::Playing {
                started: std::time::Instant::now(),
                active: slide::reverse(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
            };
            self.slide = Some(reversed);
        }
        // The mirror of hide's focus bookkeeping: none — hide reports
        // nothing, so nothing returns here; the `LayerFocused` this
        // re-grab synthesizes repeats the terminal's standing focus
        // harmlessly (`set_focused` reports only real changes).
        Task::batch([
            set_keyboard_interactivity(
                id,
                surface::keyboard_interactivity(self.config.keyboard_mode),
            ),
            widget::text_input::focus(self.terminal_id.clone()),
        ])
    }

    /// The slide distance for the currently shown surface: the same
    /// deterministic `geometry::surface_height` formula `SurfaceOpened`
    /// used — deliberately not the granted window height, which
    /// double-reports through `WindowResized`. A shown surface always
    /// has a measured output beside it; the `0.0` fallback covers a
    /// state that cannot occur and still hides (a degenerate at-rest
    /// out-slide that parks, then destroys).
    fn slide_distance_px(&self) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        self.output_size.map_or(0.0, |output| {
            geometry::surface_height(
                output.1 as f32,
                Geometry::from_config(&self.config).height_percent,
            ) as f32
        })
    }

    /// Handles the surface losing keyboard focus: marks the terminal unfocused
    /// and, when configured, hides the overlay. The hide is guarded by the
    /// focus latch so a stray unfocus before any focus (possible with
    /// `keyboard_mode: OnDemand`) can't close a freshly-opened surface. The
    /// latch is cleared only when the hide fires; a loss that doesn't hide
    /// (the config off) leaves it armed. No settings-window case exists
    /// here: while that window is open the surface is hidden by
    /// construction, so focus-loss events cannot arrive for a shown
    /// surface in that state.
    fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename: the input's own
        // unfocus signal only covers Escape and Tab, which cancel.
        let commit = self.commit_pending_rename();
        // Only a shown surface's focus loss reaches the PTY: unfocus
        // events trailing our own hide (`Hiding`/`Hidden` by the time
        // they arrive) must not report, or a focus-reporting program
        // repaints its prompt per toggle.
        if matches!(self.surface, SurfaceState::Shown(_))
            && let Some(tab) = self.active()
        {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        if focus_loss_hides(self.config.hide_on_focus_loss) && self.surface_focused {
            self.surface_focused = false;
            return Task::batch([commit, self.hide()]);
        }
        commit
    }

    /// The geometry a settled grab-bar drag leaves behind: the current
    /// config's geometry with the height quantized once from where the
    /// pointer released — the *latest* target, not the last one the
    /// cadence gate landed, so throttling never decides the final
    /// height. The settled surface then matches the persisted percent,
    /// at most half a percent off the dragged pixels (accepted
    /// tolerance).
    fn settled_geometry(&self, drag: &Drag, output: (u32, u32)) -> Geometry {
        let final_target =
            geometry::drag_target_px(self.config.edge, output.1, drag.anchor, drag.pointer_y);
        let mut geom = Geometry::from_config(&self.config);
        geom.height_percent = geometry::percent_from_px(output.1, final_target);
        geom
    }

    /// Apply a live drag target now: the cadence gate has passed.
    /// Records the apply, offers the height to the terminal pin —
    /// adoptions are debounced (see `offer_window_height`), because
    /// the runtime double-reports every `set_size` (once stale, once
    /// real) and each adoption costs a terminal reflow plus a pty
    /// resize that made the drag sluggish on hardware — and sends the
    /// single-path triple with the pixel height.
    fn apply_drag_target(
        &mut self,
        id: window::Id,
        output: (u32, u32),
        target: u32,
    ) -> Task<Message> {
        if let Some(drag) = self.drag.as_mut() {
            drag.applied = Some((std::time::Instant::now(), target));
            drag.anchor.applied_px = target;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px are < 2^24, exact in f32"
        )]
        let pin_task = self.offer_window_height(target as f32);
        let geom = Geometry::from_config(&self.config);
        let apply = geometry::apply_drag_geometry(id, geom, output, target);
        match pin_task {
            Some(t) => Task::batch([apply, t]),
            None => apply,
        }
    }

    /// Offer a height to the terminal pin: adopt immediately when the
    /// debounce allows (first size, re-report of the adopted height,
    /// or interval elapsed), else remember it and return the
    /// trailing-adopt timer that lands it once the granted stream
    /// settles. Every granted-size follower goes through here —
    /// `WindowResized` echoes outside drags and the drag's own
    /// applies — so the double-reported stream can never reflow the
    /// terminal at the apply rate.
    fn offer_window_height(&mut self, height: f32) -> Option<Task<Message>> {
        let now = std::time::Instant::now();
        if geometry::window_pin_due(self.pin_applied, now, height) {
            self.window_height = Some(height);
            // Re-adopting the height the pin already has must not
            // refresh the adoption clock (see `window_pin_due`): the
            // echo stream would otherwise starve changed heights.
            #[expect(
                clippy::float_cmp,
                reason = "echo detection: the compositor returns the exact f32 it was granted"
            )]
            let pinned = match self.pin_applied {
                Some((at, adopted)) if adopted == height => (at, height),
                _ => (now, height),
            };
            self.pin_applied = Some(pinned);
            self.pending_height = None;
            return None;
        }
        if self.pending_height != Some(height) {
            self.pending_height = Some(height);
            return Some(Task::perform(
                tokio::time::sleep(geometry::WINDOW_PIN_INTERVAL),
                |()| cosmic::Action::App(Message::WindowPinDue),
            ));
        }
        None
    }

    /// The surface's current width in px: the value both the drawer
    /// width policy (render + drag) and the height drag target
    /// against. 480 while no output has reported a size — the same
    /// fallback the drawer's first frame renders at.
    fn current_surface_width(&self) -> f32 {
        self.output_size.map_or(480.0, |(width, _)| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "output px are < 2^24, exact in f32"
            )]
            {
                geometry::surface_width(width as f32, self.config.width_percent) as f32
            }
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

    fn debounced_toggle(&mut self, action: ToggleAction) -> Task<Message> {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_toggle
            && now.duration_since(last) < TOGGLE_DEBOUNCE
        {
            return Task::none();
        }
        self.last_toggle = Some(now);

        // A remote close-tab is a quiet state edit, not a toggle
        // step: it applies through [`Self::close_tab_at`] and shows
        // nothing — no overlay, and nothing re-shows — so
        // a script closing a tab never disturbs what the user is
        // looking at. The debounce above still guards it: a
        // compositor double-delivery must not close two tabs.
        if let ToggleAction::CloseTab { tab_index } = action {
            return self.close_remote_tab(tab_index);
        }

        // A remote rename-tab is a quiet state edit like close-tab:
        // the title lands on the target — overriding or clearing —
        // with no overlay and none of the settings close-first rule
        // below. The debounce above still guards it: a compositor
        // double-delivery must not apply the rename twice against a
        // stale target.
        if let ToggleAction::RenameTab { title, tab_index } = action {
            return self.rename_remote_tab(&title, tab_index);
        }

        // A remote select-tab is a tab-facing verb like a new-tab: a
        // resolved target activates — riding set_active_tab's
        // attention retirement, session-dirty mark, and
        // already-active no-op — and the overlay shows through the
        // same path a `new-tab` takes, mid-slide-out included. An
        // unresolvable target — no tabs, out of range — is a quiet
        // no-op like close-tab's: no activation and no surface half
        // (nothing re-shows; a script's failed select must not
        // disturb what the user is looking at). The debounce above
        // still guards it: a compositor double-delivery must not
        // count twice.
        if let ToggleAction::SelectTab { index } = action {
            return match self.select_remote_tab(index) {
                Some(activate) => Task::batch([activate, self.show_intent_surface()]),
                None => Task::none(),
            };
        }

        // A remote new-tab is not a toggle step: the tab opens — now
        // or once the channel bootstraps — and the overlay shows,
        // whatever state it was in. The surface half rides the show
        // path below, so a request arriving mid-slide-out reverses on
        // the live surface exactly like a `show` would.
        if let ToggleAction::NewTab { .. } = action {
            let open = self.open_remote_tab(action, self.config.new_tab_after_current);
            return Task::batch([open, self.show_intent_surface()]);
        }

        // A remote duplicate-tab is a tab-facing verb like a new-tab:
        // a fresh shell tab opens in the target's cwd — now or once
        // the channel bootstraps — and the overlay shows through the
        // same path. An unresolvable target never blocks it: the
        // spawn degrades to the bare new-tab inheritance.
        if let ToggleAction::DuplicateTab { tab_index } = action {
            let open = self.duplicate_remote_tab(tab_index);
            return Task::batch([open, self.show_intent_surface()]);
        }

        // A remote switch-profile-tab is a tab-facing verb like a
        // duplicate-tab: the result — the tab now running the
        // profile — is meant to be seen, so the overlay shows through
        // the same path whatever state it was in. The verb resolves
        // its inputs up front and rides the `Message::SwitchTabProfile`
        // seam — the same apply the tab context menu will use, and
        // the construction that keeps the variant live under
        // `-D warnings`. Unresolvable inputs warn and edit nothing —
        // unlike duplicate-tab there is no sensible bare fallback —
        // while the surface half still shows.
        if let ToggleAction::SwitchProfileTab { profile, tab_index } = action {
            // `profile::find`'s exact, case-sensitive, first-match
            // rule, resolved to the index the apply wants.
            let Some(profile_index) = self
                .config
                .profiles
                .iter()
                .position(|candidate| candidate.name == profile)
            else {
                log::warn!("switch-profile-tab: no profile named {profile}");
                return self.show_intent_surface();
            };
            // The target — absent means the active tab — resolves
            // like every remote tab verb's, strict 1-based.
            let Some(target) =
                tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
            else {
                log::warn!(
                    "switch-profile-tab: no tab at --tab-index {tab_index:?} ({} open)",
                    self.tabs.len()
                );
                return self.show_intent_surface();
            };
            let tab_id = self.tabs[target].id;
            return Task::batch([
                cosmic::task::message(cosmic::Action::App(Message::SwitchTabProfile(
                    tab_id,
                    profile_index,
                ))),
                self.show_intent_surface(),
            ]);
        }

        // Every toggle decision executes through `toggle_step` — most
        // importantly, a mid-hide toggle or `Show` reverses on the
        // live surface instead of being dropped. The debounce above
        // already collapses true spam (~6–7 effective toggles per
        // second at 150 ms, each a full reversal); the state machine
        // itself never drops or queues. The slide toggle rides in
        // with the decision: with it off, a shown surface destroys
        // now instead of sliding out; with it on, every hide slides.
        // The settings drawer, if open, rides along: it lives inside
        // the surface, so a hide preserves it and the next show
        // uncovers it exactly as it was (spec criterion 8).
        match toggle_step(action, &self.surface, self.config.slide_animation_enabled) {
            ToggleStep::Measure => self.show(),
            ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) => self.hide(),
            ToggleStep::ReverseShow(id) => self.reverse_show(id),
            ToggleStep::Leave => Task::none(),
        }
    }

    /// The surface half a tab-facing remote verb takes: the same show
    /// decision a `new-tab` gets, so the verb shows the overlay
    /// whatever state it was in — and a request arriving
    /// mid-slide-out reverses on the live surface exactly like a
    /// `show` would.
    fn show_intent_surface(&mut self) -> Task<Message> {
        match toggle_step(
            ToggleAction::Show,
            &self.surface,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::ReverseShow(id) => self.reverse_show(id),
            _ => Task::none(),
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
    /// General tab with no selection — fresh from config, whatever
    /// page it closed on.
    fn close_settings_drawer(&mut self) -> Task<Message> {
        self.settings_open = false;
        self.leave_profiles_page();
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

    /// Leaves the Profiles page (back affordance, or the drawer's
    /// explicit close): page state — selection, the form draft —
    /// drops so a reopened page starts from config, the page resets
    /// to the General tab, and the strip follows. Focus-loss hides
    /// never route here: preservation is their rule.
    fn leave_profiles_page(&mut self) {
        self.settings_page = home_settings_page();
        self.sync_settings_tab_strip();
        self.profile_selection = None;
        settings::apply_profile_draft_transition(
            &mut self.profile_draft,
            settings::ProfileDraftTransition::Clear,
        );
    }

    /// Re-sync the strip's selection to `settings_page` (criterion
    /// 7): the row of the page's tab — General's row while the
    /// Profiles drill-down is open — becomes the model's selection.
    /// Applied after every page mutation: a pick, the Profiles flip
    /// and its back, the explicit close's reset. The rows themselves
    /// never change.
    fn sync_settings_tab_strip(&mut self) {
        // A page turn away from the Keybindings tab disarms any armed
        // capture and clears its conflict — an invisible capture must
        // never eat presses on another page (the view can't show its
        // hint row there).
        if !matches!(
            self.settings_page,
            SettingsPage::Tab(SettingsTab::Keybindings)
        ) {
            self.keybind_capture = None;
            self.keybind_conflict = None;
        }
        settings::activate_tab_row(
            &mut self.settings_tab_model,
            strip_selection(self.settings_page),
        );
    }
}

/// Which page the settings drawer shows. A page turn, never a drawer
/// state: Close, Escape, and `Ctrl+Shift+I` close the whole drawer
/// from any page — the funnel and `drawer_escape_routing` are
/// untouched by the flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    /// A tab's page: the sections [`settings::tab_sections`] groups
    /// onto it.
    Tab(SettingsTab),
    /// The profile list and its (task-by-task growing) form.
    Profiles,
}

/// The page the drawer resets to — an explicit close and the
/// Profiles back affordance both land here — so a reopened drawer
/// always starts on the everyday controls, where the Profiles entry
/// row lives (criteria 5 and 6).
const fn home_settings_page() -> SettingsPage {
    SettingsPage::Tab(SettingsTab::General)
}

/// The page after a tab-strip pick (criterion 4): the picked tab,
/// from whichever page the drawer is on — including the Profiles
/// drill-down, where the strip stays usable. The turn is the pick's
/// whole effect: the update handler assigns this field and nothing
/// else, so no drawer close, no `settings_buffer` clear, and no
/// `DrawerTransition` can ride a pick (the `OpenProfiles` rule; the
/// only draft-clearing path stays the close funnel's
/// `ExplicitClose`). Pure so the rule is headless-testable
/// (constructing `DropTerm` needs a live `Core`).
const fn page_after_tab_pick(_current: SettingsPage, tab: SettingsTab) -> SettingsPage {
    SettingsPage::Tab(tab)
}

/// The strip's highlighted row while the drawer shows `page`
/// (criterion 7's lockstep): the page's own tab, or General while
/// the Profiles drill-down is open — the tab its Back always lands
/// on. Pure so the sync rule is headless-testable.
const fn strip_selection(page: SettingsPage) -> SettingsTab {
    match page {
        SettingsPage::Tab(tab) => tab,
        SettingsPage::Profiles => SettingsTab::General,
    }
}

/// Whether a `ConfigChanged` may reshape a shown surface in place:
/// pure so the guard is headless-testable (constructing `DropTerm`
/// needs a live `Core`). The geometry must actually have changed; a
/// drag armed mid-edit defers the reshape to the release settle (so
/// config reshapes and drag deltas never interleave `set_size`
/// calls); a slide animating defers it to the next show — geometry
/// stays frozen while animating. The in-memory config still updates
/// on every path (the caller does that first); only the live reshape
/// is gated.
fn config_edit_reshapes(geometry_changed: bool, drag_armed: bool, slide_animating: bool) -> bool {
    geometry_changed && !drag_armed && !slide_animating
}

/// What a toggle-intent request must do with the surface: pure so
/// the decision is headless-testable (constructing `DropTerm` needs
/// a live `Core`). Every toggle caller goes through here — the
/// toggle hotkey and D-Bus activation (`debounced_toggle`), plus
/// `Ctrl+Shift+Q` and focus loss through `hide`'s `Hide` column —
/// so the full state × intent matrix lives in one place and
/// reversibility is just another cell. Slide activity needs no
/// separate input: it is implied by the state — an `Out` slide only
/// exists while `Hiding`, an `In` slide only rides a `Shown`
/// surface — and the executors read the live slide for offsets.
///
/// `slide_animation_enabled` gates transition *entry* only: it
/// swaps the `Shown`-hide cell between sliding out and destroying
/// now — the destroy *style*, never the direction. The helper takes
/// no mid-flight input by construction, so a running slide is never
/// aborted by its own toggle: the `Hiding` cells (destroy-on-
/// completion, reversal) key on state alone and ignore the flag
/// entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleStep {
    /// Run the measuring show pass (a hidden surface only): measure
    /// → reshape → shown, then slide in.
    Measure,
    /// Destroy immediately: a measuring surface has no reshaped
    /// content to animate, so there is nothing to slide.
    DestroyNow(window::Id),
    /// Slide out from the current offset and destroy on the slide's
    /// completion tick. Keyboard interactivity releases at slide-out
    /// start.
    SlideOut(window::Id),
    /// Reverse a hiding slide back to shown on the same live surface:
    /// no new measuring pass — `output_size` is retained — the slide
    /// re-targets `In` from the current offset, keyboard
    /// interactivity is re-grabbed per config, and `surface_focused`
    /// resets pending the next `LayerFocused`.
    ReverseShow(window::Id),
    /// Nothing: the intent is already satisfied — a hide while
    /// hiding or hidden (no restart, no double destroy), a show
    /// while measuring or shown.
    Leave,
}

fn toggle_step(action: ToggleAction, surface: &SurfaceState, slide_enabled: bool) -> ToggleStep {
    match (action, surface) {
        // Show intents: only a hidden surface starts the measuring
        // pass; a hiding one reverses; a measuring or shown one is
        // already on its way. None of these arms key on the slide
        // toggle — a hiding surface always reverses on the live
        // slide, whatever the flag says now (never aborted).
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hidden) => ToggleStep::Measure,
        (ToggleAction::Show | ToggleAction::Toggle, SurfaceState::Hiding(id)) => {
            ToggleStep::ReverseShow(*id)
        }
        (ToggleAction::Show, SurfaceState::Measuring(_) | SurfaceState::Shown(_)) => {
            ToggleStep::Leave
        }
        // Hide intents — and a toggle on any visible surface: a
        // measuring surface dies now, a hiding or hidden one stays
        // put (idempotent — no restart, no double destroy). A shown
        // one slides out (mid-slide-in the executor starts from the
        // live eased offset) — or, with the slide toggle off,
        // destroys now: instant hide.
        (ToggleAction::Hide | ToggleAction::Toggle, SurfaceState::Measuring(id)) => {
            ToggleStep::DestroyNow(*id)
        }
        (ToggleAction::Hide | ToggleAction::Toggle, SurfaceState::Shown(id)) => {
            if slide_enabled {
                ToggleStep::SlideOut(*id)
            } else {
                ToggleStep::DestroyNow(*id)
            }
        }
        (ToggleAction::Hide, SurfaceState::Hiding(_) | SurfaceState::Hidden) => ToggleStep::Leave,
        // Structurally unreachable: `debounced_toggle` applies the
        // remote verbs — opening a new-tab, closing a close-tab,
        // selecting a select-tab, renaming a rename-tab, duplicating
        // a duplicate-tab, switching a switch-profile-tab — outside
        // the toggle step, so none ever gets here. Loud rather than
        // silently wrong.
        (
            ToggleAction::NewTab { .. }
            | ToggleAction::CloseTab { .. }
            | ToggleAction::SelectTab { .. }
            | ToggleAction::RenameTab { .. }
            | ToggleAction::DuplicateTab { .. }
            | ToggleAction::SwitchProfileTab { .. },
            _,
        ) => {
            unreachable!("remote verbs apply outside the funnel")
        }
    }
}

/// Whether a `LayerUnfocused` event for `id` should reach
/// [`DropTerm::focus_lost`]: only a shown surface reacts to losing
/// keyboard focus — one arriving while `Hiding` (the synthesized
/// unfocus of the hide's own keyboard release, or a racing focus
/// loss) is a no-op, so it can never restart or double-drive the
/// out-slide.
fn layer_unfocused_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(surface, SurfaceState::Shown(shown) if *shown == id)
}

/// Whether losing keyboard focus should hide the surface: exactly
/// the `hide_on_focus_loss` config flag. No settings-drawer input
/// is needed: focus loss may hide the surface with the drawer open —
/// the drawer and its drafts survive for the next show (spec
/// criterion 8) — so the gate runs the same in the drawer-open
/// state. Only the automatic focus-loss path consults it: explicit
/// hides (the toggle keybind, `Action::Hide`) call `hide()` directly
/// and ignore the gate entirely. Both unfocused routes — the layer
/// surface's `LayerUnfocused` and the `WindowUnfocused` callback —
/// route into [`DropTerm::focus_lost`], so one gate covers both.
fn focus_loss_hides(hide_on_focus_loss: bool) -> bool {
    hide_on_focus_loss
}

/// What the app does with a key press before any other key role
/// while the settings drawer is open (spec criterion 6): Escape
/// closes the drawer — and only that. The surface does not hide,
/// and the press is never forwarded to the terminal (which is
/// disabled while the drawer is open anyway, criterion 4). Every
/// other key — including the Ctrl+Shift+I toggle, which resolves
/// through the normal bind match into [`DropTerm::toggle_settings`]
/// — keeps the normal routing; with the drawer closed the rule
/// never fires and Escape is terminal input exactly as before.
/// Pure so the drawer-first rule is headless-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawerKeyRouting {
    /// The drawer is open and this press closes it: swallow it here.
    CloseDrawer,
    /// No drawer-first role: the press takes the normal routing.
    FallThrough,
}

fn drawer_escape_routing(settings_open: bool, key: &Key) -> DrawerKeyRouting {
    if settings_open && matches!(key, Key::Named(Named::Escape)) {
        DrawerKeyRouting::CloseDrawer
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

/// Whether the grab-bar drag handlers may act: pure so the guard is
/// headless-testable, in the same style as
/// [`config_edit_reshapes`] and [`layer_unfocused_acts_on`]. A drag
/// must only ever run on a shown surface at rest: the `Hiding` arm
/// excludes out-slides and their reversals by state, and the slide
/// arm excludes slide-ins — which ride a `Shown` surface, so the
/// surface state alone cannot gate them (a press inside the 130 ms
/// window must not arm a drag, or the first cadence-eligible move
/// would fire `set_size` on the animating surface). Gating the
/// *arming* here covers the trailing `DragApplyDue` and the
/// `GrabBarReleased` settle by construction: both only act on an
/// armed drag, no drag can arm while a slide animates, and `hide()`
/// clears any live drag before any slide can start. A throttled
/// press is simply inert — the surface rests in at most 130 ms and
/// the next press drags normally.
fn grab_bar_acts_on(surface: &SurfaceState, slide_animating: bool) -> bool {
    matches!(surface, SurfaceState::Shown(_)) && !slide_animating
}

/// Whether a `WindowResized` event for `id` may feed the
/// terminal-height pin: only the tracked layer surface's granted
/// sizes count — a foreign window (the clipboard anchor) must never
/// pin the terminal's. Pure, in the same style
/// as [`layer_unfocused_acts_on`].
fn resize_acts_on(surface: &SurfaceState, id: window::Id) -> bool {
    matches!(
        surface,
        SurfaceState::Measuring(shown) | SurfaceState::Shown(shown) | SurfaceState::Hiding(shown)
            if *shown == id
    )
}

/// The window-background source decision: a scheme that resolves a
/// background owns the window background — the tab-bar/grab-bar
/// chrome and the unset-bg cell fallback all read `WINDOW_BG_COLOR`,
/// so they follow by construction. A scheme without one (the COSMIC
/// pair, and any unknown name that fell back to it) defers to the
/// system-theme derivation, passed in already packed so this stays a
/// pure, headless-testable decision.
fn window_bg_color(colors: &aterm::color::Colors, system_bg: u32) -> u32 {
    match colors[NamedColor::Background] {
        Some(rgb) => {
            0xFF00_0000 | (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
        }
        None => system_bg,
    }
}

/// The per-tab terminal-color resolution — the one seam both
/// application points (spawn and the live config-apply pass)
/// consume: the tab's recorded profile id → [`profile::find`] → the
/// profile's `color_scheme` → [`theme_source::by_name`]. Every miss
/// — no recorded id, a dangling id, no scheme, an unknown scheme
/// name — resolves the injected `global` pick (itself the
/// [`DropTerm::active_colors`] result, this chain's fallback the way
/// the COSMIC pair is that one's): the chain never errors, and the
/// store is never rewritten to "fix" a stored name. Only the unknown
/// name warns — the `live_preset_cwd` precedent.
fn colors_for(
    config: &DropTermConfig,
    global: &aterm::color::Colors,
    profile_name: Option<&str>,
) -> aterm::color::Colors {
    let Some(profile) = profile_name.and_then(|name| profile::find(&config.profiles, name)) else {
        return *global;
    };
    let Some(scheme) = profile.color_scheme.as_deref() else {
        return *global;
    };
    theme_source::by_name(scheme).unwrap_or_else(|| {
        log::warn!(
            "ignoring profile {} color scheme {scheme:?}: not a built-in scheme name",
            profile.name
        );
        *global
    })
}

/// Whether the app resolves colors for dark mode right now. The
/// config preference is the source of truth; the global theme (what
/// widgets read) follows via the `set_theme` task, so color
/// decisions must not wait for it. Call sites inject the current
/// system value — the [`window_bg_color`] pattern — keeping the
/// decision pure and headless-testable.
fn effective_is_dark(preference: ThemePreference, system_is_dark: bool) -> bool {
    match preference {
        ThemePreference::System => system_is_dark,
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
    }
}

/// What a theme-preference transition asks the runtime to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeOverrideKind {
    /// Pin the app to the user's dark system palette.
    PinDark,
    /// Pin the app to the user's light system palette.
    PinLight,
    /// Clear the pin and follow the system mode again.
    FollowSystem,
}

/// The one pure transition matrix shared by `init` and
/// `ConfigChanged`: `None` when the preference did not change —
/// steady state issues no task, so a default `System` config never
/// calls `set_theme` and unrelated config echoes never storm one.
fn theme_override(old: ThemePreference, new: ThemePreference) -> Option<ThemeOverrideKind> {
    match (old, new) {
        (ThemePreference::System, ThemePreference::System)
        | (ThemePreference::Dark, ThemePreference::Dark)
        | (ThemePreference::Light, ThemePreference::Light) => None,
        (_, ThemePreference::Dark) => Some(ThemeOverrideKind::PinDark),
        (_, ThemePreference::Light) => Some(ThemeOverrideKind::PinLight),
        (ThemePreference::Dark | ThemePreference::Light, ThemePreference::System) => {
            Some(ThemeOverrideKind::FollowSystem)
        }
    }
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

/// The impure shell over [`theme_override`]: builds the override
/// theme from the user's own system palettes — never libcosmic's
/// built-in constants — marks it through `ThemeType::prefer_dark`
/// (which only writes through on `System`, the only theme type these
/// constructors produce), and hands it to the runtime's global.
/// Everything decidable is decided in the pure fns above; the
/// chained [`Message::ReapplyColors`] re-runs the color apply once
/// the global has landed, so the window-background derivation — the
/// one consumer that must read the global — sees the new mode.
///
/// The follow-up rides [`Task::chain`], never [`Task::then`]: `then`
/// CONSUMES the first task's output and replaces it with the
/// continuation's, so `set_theme(...).then(...)` would silently drop
/// the `Action::AppThemeChange` message — the pin would never reach
/// the runtime and the theme would stay whatever it was (the
/// "dark mode does nothing" bug). `chain` forwards every output of
/// the first task and only then runs the second, preserving both
/// messages and their order. Pinned by
/// `theme_override_task_delivers_set_theme_then_reapply`.
fn theme_override_task(kind: ThemeOverrideKind) -> Task<Message> {
    let (mut theme, prefer_dark) = match kind {
        ThemeOverrideKind::PinDark => (cosmic::theme::system_dark(), Some(true)),
        ThemeOverrideKind::PinLight => (cosmic::theme::system_light(), Some(false)),
        // Restore: re-read the system mode from cosmic-config fresh
        // and clear the marker, so system flips resume driving the
        // app.
        ThemeOverrideKind::FollowSystem => (cosmic::theme::system_preference(), None),
    };
    theme.theme_type.prefer_dark(prefer_dark);
    cosmic::command::set_theme(theme).chain(Task::done(cosmic::Action::App(Message::ReapplyColors)))
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

/// Which side the strip's tooltips pop to: the terminal's side of the
/// bar, wherever the bar sits — the bar widget itself stays
/// edge-agnostic.
fn bar_tooltip_side(bar_at_bottom: bool) -> TooltipSide {
    if bar_at_bottom {
        TooltipSide::Above
    } else {
        TooltipSide::Below
    }
}

/// Settings for the clipboard-anchor window.
///
/// libcosmic binds its single runtime [`Clipboard`] to the first
/// window created, and when that surface is destroyed the binding is
/// dropped — taking the smithay-clipboard connection, and with it
/// any selection this app copied, down with it. `DropTerm`'s only
/// surface is the layer shell, which every hide destroys, so
/// copying out could never survive the focus switch to the paste
/// target. This window — invisible, never closed, created before
/// any layer surface — owns the binding for the whole process
/// lifetime instead: unmapped, so it appears nowhere and paints
/// nothing, and exempt from close requests, so nothing the user or
/// the runtime does can drop the connection mid-session.
fn clipboard_anchor_settings() -> window::Settings {
    window::Settings {
        visible: false,
        exit_on_close_request: false,
        resizable: false,
        decorations: false,
        size: cosmic::iced::Size::new(1.0, 1.0),
        ..window::Settings::default()
    }
}

// The test module sits mid-file by long-standing layout; whether this
// lint fires depends on which item happens to follow it (a
// derive-marked struct passed, an impl trips it). Moving ~770 lines
// of tests to the file tail is churn the carve refuses.
#[cfg(test)]
#[expect(
    clippy::items_after_test_module,
    reason = "tests sit mid-file by layout"
)]
mod tests {
    use super::*;

    #[test]
    fn geometry_edit_mid_slide_skips_the_in_place_reshape() {
        // Frozen-geometry boundary: an edit arriving while the
        // content slides must not fire `set_size` on the animating
        // surface — the edit lands on the next show instead.
        assert!(!config_edit_reshapes(true, false, true));
    }

    #[test]
    fn geometry_edit_at_rest_reshapes_in_place() {
        assert!(config_edit_reshapes(true, false, false));
    }

    #[test]
    fn geometry_edit_mid_drag_still_defers_to_release() {
        assert!(!config_edit_reshapes(true, true, false));
    }

    #[test]
    fn non_geometry_edit_never_reshapes() {
        assert!(!config_edit_reshapes(false, false, false));
    }

    #[test]
    fn drag_and_slide_together_still_skip_the_reshape() {
        assert!(!config_edit_reshapes(true, true, true));
    }

    // --- the grab-bar guard ---
    //
    // `GrabBarDragged` is the one geometry-call site that could run
    // while a slide animates: a slide-in rides a `Shown` surface, so
    // matching `Shown` alone does not exclude it — a press inside the
    // 130 ms window must neither arm a drag nor send `set_size` on
    // the animating surface. With arming gated, `DragApplyDue` and
    // `GrabBarReleased` are covered by construction: they only act on
    // an armed drag, and `hide()` clears any live drag before a
    // slide can start.

    #[test]
    fn grab_bar_drag_at_rest_acts() {
        let id = window::Id::unique();
        assert!(grab_bar_acts_on(&SurfaceState::Shown(id), false));
    }

    #[test]
    fn grab_bar_drag_mid_slide_in_does_not_act() {
        // A slide-in runs in `Shown`, so the surface state alone
        // cannot gate the drag — the guard needs the slide's
        // existence too.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Shown(id), true));
    }

    #[test]
    fn grab_bar_drag_ignores_every_non_shown_state() {
        // Pinned: a `Hiding` surface rejects drag messages (no
        // interleave with an out-slide or its reversal);
        // Measuring/Hidden never arm one.
        let id = window::Id::unique();
        assert!(!grab_bar_acts_on(&SurfaceState::Hiding(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Measuring(id), false));
        assert!(!grab_bar_acts_on(&SurfaceState::Hidden, false));
    }

    // --- the toggle decision matrix ---
    //
    // `toggle_step` decides over intent × surface state; slide
    // activity needs no separate input because it is implied by the
    // state — an `Out` slide only exists while `Hiding`, an `In`
    // slide only rides a `Shown` surface — and the executors read
    // the live slide for their offsets (`hide`, `reverse_show` via
    // `slide::reverse`).

    #[test]
    fn toggle_or_show_from_hidden_runs_the_measuring_pass() {
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hidden, true),
            ToggleStep::Measure
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hidden, true),
            ToggleStep::Measure
        );
    }

    #[test]
    fn toggle_or_hide_from_measuring_destroys_immediately() {
        // A measuring surface has no reshaped content to animate —
        // the surface dies now, not in 130 ms.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Measuring(id), true),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(id), true),
            ToggleStep::DestroyNow(id)
        );
    }

    #[test]
    fn toggle_or_hide_from_shown_slides_out() {
        // Slide out, destroy on completion. Mid-slide-in the same
        // decision applies — the executor starts the out-slide from
        // the live eased offset, so nothing teleports.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
    }

    #[test]
    fn toggle_or_show_mid_hiding_reverses_on_the_same_surface() {
        // A mid-hide toggle reverses instead of being dropped — and
        // on the SAME window::Id: the live surface is reused (no new
        // measuring pass) and `reverse_show` re-grabs keyboard
        // interactivity on it per config.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(id), true),
            ToggleStep::ReverseShow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hiding(id), true),
            ToggleStep::ReverseShow(id)
        );
    }

    #[test]
    fn show_while_measuring_or_shown_is_already_satisfied() {
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Measuring(id), true),
            ToggleStep::Leave
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Shown(id), true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn hide_while_hiding_neither_restarts_nor_double_destroys() {
        // Idempotence carried into the matrix: `WindowUnfocused`
        // reaches `focus_lost` unguarded, and the keyboard release
        // synthesizes its own `Unfocused` — a second hide request
        // must neither restart the out-slide nor queue a second
        // destroy task.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hiding(id), true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn hide_while_hidden_destroys_nothing_in_the_matrix() {
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hidden, true),
            ToggleStep::Leave
        );
    }

    #[test]
    fn a_hide_intent_never_decides_to_show() {
        // Pins `hide()`'s unreachable arms as intended behavior: the
        // hide column of the matrix only ever destroys, slides out,
        // or leaves — under either slide-toggle value (the flag
        // swaps the destroy *style*, never the direction).
        let id = window::Id::unique();
        for slide_enabled in [true, false] {
            for surface in [
                SurfaceState::Hidden,
                SurfaceState::Measuring(id),
                SurfaceState::Shown(id),
                SurfaceState::Hiding(id),
            ] {
                assert!(matches!(
                    toggle_step(ToggleAction::Hide, &surface, slide_enabled),
                    ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) | ToggleStep::Leave
                ));
            }
        }
    }

    // --- the slide toggle in the toggle matrix ---

    #[test]
    fn shown_hide_with_slides_disabled_destroys_immediately() {
        // The one decision the slide toggle owns: a hide from
        // `Shown` with the key off destroys now — instant hide.
        // Applies to a plain hide and a toggle alike.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), false),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Shown(id), false),
            ToggleStep::DestroyNow(id)
        );
    }

    #[test]
    fn hiding_reversal_ignores_the_slide_toggle() {
        // A running out-slide is never aborted by its own toggle: a
        // show intent mid-hide still reverses on the live surface
        // with the key off. The toggle gates transition ENTRY only;
        // `toggle_step` takes no mid-flight input by construction.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hiding(id), false),
            ToggleStep::ReverseShow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Toggle, &SurfaceState::Hiding(id), false),
            ToggleStep::ReverseShow(id)
        );
    }

    #[test]
    fn measuring_and_hidden_cells_ignore_the_slide_toggle() {
        // Only the Shown-hide decision keys on the flag: a measuring
        // surface always dies immediately (no content to animate),
        // hidden always measures, and a disabled hide while hidden
        // still leaves.
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Measuring(id), false),
            ToggleStep::DestroyNow(id)
        );
        assert_eq!(
            toggle_step(ToggleAction::Show, &SurfaceState::Hidden, false),
            ToggleStep::Measure
        );
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Hidden, false),
            ToggleStep::Leave
        );
    }

    #[test]
    fn layer_unfocused_while_hiding_is_rejected_by_the_shown_guard() {
        // Only a shown surface reacts to losing keyboard focus —
        // during `Hiding` (the synthesized unfocus of the hide's own
        // keyboard release) the event must be a no-op.
        let id = window::Id::unique();
        assert!(layer_unfocused_acts_on(&SurfaceState::Shown(id), id));
        assert!(!layer_unfocused_acts_on(&SurfaceState::Hiding(id), id));
        assert!(!layer_unfocused_acts_on(
            &SurfaceState::Shown(window::Id::unique()),
            id
        ));
    }

    #[test]
    fn focus_loss_hides_when_configured() {
        // The ordinary path: with `hide_on_focus_loss` on, losing
        // keyboard focus hides. No settings-window input is needed:
        // while that window is open no shown surface exists (opening
        // it hides the terminal; a toggle while it is open closes it
        // first), so the gate never runs in the settings-open state.
        assert!(focus_loss_hides(true));
    }

    #[test]
    fn focus_loss_never_hides_when_focus_loss_hiding_is_off() {
        // `hide_on_focus_loss` off means no focus-loss hide, period —
        // the gate only ever narrows the hide, never widens it.
        assert!(!focus_loss_hides(false));
    }

    #[test]
    fn explicit_hides_never_consult_the_focus_loss_gate() {
        // The gate covers the automatic focus-loss path only:
        // explicit hide intents (the toggle keybind's `toggle_step`,
        // `Action::Hide`) take no gate input and never pass through
        // `focus_loss_hides`. Asserted side by side: the gate's most
        // restrictive input yields no hide, yet the explicit hide
        // step still steps its hide.
        assert!(!focus_loss_hides(false));
        let id = window::Id::unique();
        assert_eq!(
            toggle_step(ToggleAction::Hide, &SurfaceState::Shown(id), true),
            ToggleStep::SlideOut(id)
        );
    }

    #[test]
    fn resize_from_the_tracked_surface_feeds_the_pin() {
        // The granted-size stream that pins the terminal height
        // belongs to the layer surface alone: every state tracking
        // that surface accepts its resizes.
        let id = window::Id::unique();
        assert!(resize_acts_on(&SurfaceState::Measuring(id), id));
        assert!(resize_acts_on(&SurfaceState::Shown(id), id));
        assert!(resize_acts_on(&SurfaceState::Hiding(id), id));
    }

    // --- the settings drawer's key routing (settings-drawer Task 0) ---

    #[test]
    fn escape_with_the_drawer_open_closes_it_and_only_it() {
        // Criterion 6: with the drawer open the press closes the
        // drawer — it never hides the surface and never reaches the
        // terminal (disabled while the drawer is open, criterion 4).
        assert_eq!(
            drawer_escape_routing(true, &Key::Named(Named::Escape)),
            DrawerKeyRouting::CloseDrawer
        );
    }

    #[test]
    fn escape_with_the_drawer_closed_keeps_today_s_routing() {
        // Criterion 6's second half: with the drawer closed, Escape
        // behaves exactly as today — terminal input, the rule never
        // fires.
        assert_eq!(
            drawer_escape_routing(false, &Key::Named(Named::Escape)),
            DrawerKeyRouting::FallThrough
        );
    }

    #[test]
    fn every_other_key_falls_through_even_with_the_drawer_open() {
        // The Ctrl+Shift+I toggle resolves through the normal bind
        // match (criterion 5), and typing never trips the escape
        // rule — only Escape is the drawer's key.
        for key in [
            Key::Character("i".into()),
            Key::Character("a".into()),
            Key::Named(Named::Enter),
        ] {
            assert_eq!(
                drawer_escape_routing(true, &key),
                DrawerKeyRouting::FallThrough
            );
        }
    }

    // --- the close-confirmation gate and modal key routing
    // (close-confirm T3) ---

    #[test]
    fn confirm_needed_requires_both_the_toggle_and_a_running_child() {
        // Criterion 3's truth table as one pin: either input false →
        // immediate close. The toggle-off row is criterion 5
        // (byte-identical to today); the idle-shell row is criterion
        // 2 (a tab at its prompt never confirms).
        assert!(!confirm_needed(false, false));
        assert!(!confirm_needed(false, true));
        assert!(!confirm_needed(true, false));
        assert!(confirm_needed(true, true));
    }

    #[test]
    fn escape_while_a_close_confirms_cancels_the_modal_not_the_drawer() {
        // Criterion 4's precedence: the modal's routing sits ahead of
        // `drawer_escape_routing`, so Escape with both the drawer and
        // a pending close cancels the modal only — the drawer stays
        // open beneath (a second Escape, unpending now, closes it as
        // today).
        assert_eq!(
            modal_key_routing(true, &Key::Named(Named::Escape)),
            ModalKeyRouting::Cancel
        );
    }

    #[test]
    fn every_other_key_is_swallowed_while_a_close_confirms() {
        // While the decision waits, the modal owns the keyboard:
        // binds never fire and nothing reaches the terminal (pointer
        // blocking comes free from the scrim).
        for key in [
            Key::Character("w".into()),
            Key::Named(Named::Enter),
            Key::Named(Named::Tab),
        ] {
            assert_eq!(
                modal_key_routing(true, &key),
                ModalKeyRouting::Swallow,
                "{key:?} must be swallowed while a close confirms"
            );
        }
    }

    #[test]
    fn modal_key_routing_is_transparent_without_a_pending_close() {
        // No pending close: every key — Escape included — keeps
        // today's routing; the rule never fires.
        assert_eq!(
            modal_key_routing(false, &Key::Named(Named::Escape)),
            ModalKeyRouting::FallThrough
        );
        assert_eq!(
            modal_key_routing(false, &Key::Character("w".into())),
            ModalKeyRouting::FallThrough
        );
    }

    // --- the armed capture's key routing (keybinds-editor KE-3) ---

    #[test]
    fn a_disarmed_capture_gate_is_transparent() {
        // No capture armed: no verdict, so the press keeps today's
        // routing — rename guard, drawer Escape, bind match — exactly
        // as before the editor existed (criterion: disarmed behavior
        // byte-identical).
        assert_eq!(
            capture_verdict_for(None, Modifiers::empty(), &Key::Named(Named::Escape)),
            None
        );
        assert_eq!(
            capture_verdict_for(
                None,
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("c".into())
            ),
            None
        );
    }

    #[test]
    fn an_armed_capture_owns_escape_before_the_drawer_close_rule() {
        // While armed, the capture gate sits ahead of
        // `drawer_escape_routing`: Escape cancels the capture instead
        // of closing the drawer (a later Escape — disarmed again —
        // closes as today).
        assert_eq!(
            capture_verdict_for(
                Some(Action::Copy),
                Modifiers::empty(),
                &Key::Named(Named::Escape)
            ),
            Some(keybinds::CaptureVerdict::Cancel)
        );
    }

    #[test]
    fn an_armed_capture_consumes_bound_chords_instead_of_matching_them() {
        // Ctrl+Shift+C is Copy's live binding, but while a capture is
        // armed the gate takes it as the captured chord — bind
        // matching never runs (criterion: capture consumes every
        // press, even a bound keybind).
        assert_eq!(
            capture_verdict_for(
                Some(Action::ZoomIn),
                Modifiers::CTRL | Modifiers::SHIFT,
                &Key::Character("c".into())
            ),
            Some(keybinds::CaptureVerdict::Commit(keybinds::Binding {
                modifiers: vec![keybinds::ModifierName::Ctrl, keybinds::ModifierName::Shift],
                key: "C".to_string(),
            }))
        );
        // A bare modifier never commits — the capture stays armed.
        assert_eq!(
            capture_verdict_for(
                Some(Action::ZoomIn),
                Modifiers::CTRL,
                &Key::Named(Named::Shift)
            ),
            Some(keybinds::CaptureVerdict::Ignore)
        );
    }

    #[test]
    fn resize_from_a_foreign_window_never_feeds_the_pin() {
        // A foreign window (the clipboard anchor) must never pin the
        // terminal's height; a hidden surface tracks no window at
        // all.
        let surface_id = window::Id::unique();
        let settings_id = window::Id::unique();
        assert!(!resize_acts_on(
            &SurfaceState::Shown(surface_id),
            settings_id
        ));
        assert!(!resize_acts_on(&SurfaceState::Hidden, settings_id));
        assert!(!resize_acts_on(&SurfaceState::Hidden, surface_id));
    }

    // --- the drawer's page turns (settings-tabs ST-2) ---

    #[test]
    fn a_tab_pick_lands_on_the_picked_tab_from_every_page() {
        // The `OpenProfiles` rule for picks: a pure page turn — the
        // pick works from either tab and from the Profiles
        // drill-down (the strip stays usable there), never lands on
        // Profiles, and the left-behind page's drafts survive (the
        // pick handler assigns this field and nothing else; the
        // only draft-clearing path stays the close funnel's
        // `ExplicitClose`).
        for current in [
            SettingsPage::Tab(SettingsTab::General),
            SettingsPage::Tab(SettingsTab::Advanced),
            SettingsPage::Profiles,
        ] {
            assert_eq!(
                page_after_tab_pick(current, SettingsTab::Advanced),
                SettingsPage::Tab(SettingsTab::Advanced)
            );
            assert_eq!(
                page_after_tab_pick(current, SettingsTab::General),
                SettingsPage::Tab(SettingsTab::General)
            );
        }
    }

    #[test]
    fn the_strip_highlights_the_page_s_tab_and_general_on_profiles() {
        // Criterion 7's lockstep rule: the highlighted row follows
        // `settings_page` after every mutation — a pick lands on the
        // picked tab (pinned above), and the Profiles drill-down
        // highlights General, where Back always lands.
        assert_eq!(
            strip_selection(SettingsPage::Tab(SettingsTab::Advanced)),
            SettingsTab::Advanced
        );
        assert_eq!(
            strip_selection(SettingsPage::Tab(SettingsTab::General)),
            SettingsTab::General
        );
        assert_eq!(
            strip_selection(SettingsPage::Profiles),
            SettingsTab::General
        );
    }

    #[test]
    fn the_close_funnel_and_back_both_reset_to_the_general_tab() {
        // Criteria 5 and 6's reset rule: an explicit close and the
        // Profiles back affordance land on General — the entry
        // row's tab — so a reopened drawer always starts on the
        // everyday controls.
        assert_eq!(
            home_settings_page(),
            SettingsPage::Tab(SettingsTab::General)
        );
    }

    // --- the window-background source decision ---

    #[test]
    fn scheme_background_wins_over_the_system_derivation() {
        // Dracula owns its background (#282a36, alacritty-theme
        // Dracula.toml): the resolved slot must land in
        // WINDOW_BG_COLOR's packed layout (0xAARRGGBB, opaque), not
        // the system theme's — chrome and the unset-bg fallback then
        // follow by construction.
        let dracula = theme_source::by_name("Dracula").unwrap();
        assert_eq!(window_bg_color(&dracula, 0xFF1E_1E2E), 0xFF28_2A36);
    }

    #[test]
    fn unset_background_passes_the_system_derivation_through() {
        // The COSMIC pair owns no background (the system theme
        // supplies it), so the decision is a plain passthrough —
        // default-config chrome follows the system theme, and a
        // bogus scheme name keeps that too (the `active_colors`
        // fallback resolves to this same pair).
        let cosmic_dark = theme_source::by_name("COSMIC Dark").unwrap();
        assert_eq!(window_bg_color(&cosmic_dark, 0xFF1E_1E2E), 0xFF1E_1E2E);
    }

    // --- the per-tab color resolution chain ---
    //
    // `colors_for` is the one seam both application points consume
    // (spawn and the config-apply pass): the tab's recorded profile
    // id → the profile's scheme name → the built-in registry. Every
    // miss falls back to the injected global pick; the chain never
    // errors. Nord plays the global below — its slots differ from
    // Dracula's everywhere asserted, so a silent fallback to the
    // scheme (or a scheme leaking into a fallback) cannot pass.

    /// Decode a packed `0xRRGGBB` literal into an `Rgb` (mirrors
    /// `theme_source`'s test helper).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "low-byte extraction is the 0xRRGGBB decode contract"
    )]
    fn rgb(data: u32) -> alacritty_terminal::vte::ansi::Rgb {
        alacritty_terminal::vte::ansi::Rgb {
            r: (data >> 16) as u8,
            g: (data >> 8) as u8,
            b: data as u8,
        }
    }

    fn chain_profile(name: &str, scheme: Option<&str>) -> crate::profile::Profile {
        crate::profile::Profile {
            name: name.to_string(),
            command: "ssh host".to_string(),
            cwd: None,
            color_scheme: scheme.map(str::to_owned),
            env: None,
        }
    }

    fn chain_config(profiles: &[crate::profile::Profile]) -> DropTermConfig {
        DropTermConfig {
            profiles: profiles.to_vec(),
            ..DropTermConfig::default()
        }
    }

    #[test]
    fn colors_for_resolves_a_known_scheme_name() {
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let colors = colors_for(
            &config,
            &theme_source::by_name("Nord").unwrap(),
            Some("ssh-prod"),
        );
        // Dracula owns bg #282a36 and red #ff5555 (alacritty-theme).
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x28_2A36)));
        assert_eq!(colors[NamedColor::Red], Some(rgb(0xFF_5555)));
    }

    #[test]
    fn colors_for_falls_back_to_global_for_a_plain_tab() {
        // No recorded id — plain shell tabs, and the degraded preset
        // arm whose blank command line records none (the shared
        // `profile_id = None` rule): the global pick, unchanged.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let colors = colors_for(&config, &theme_source::by_name("Nord").unwrap(), None);
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn colors_for_falls_back_to_global_on_a_dangling_profile_id() {
        // The tab records a profile the config no longer carries
        // (deleted mid-run): the global pick, never an error.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let colors = colors_for(
            &config,
            &theme_source::by_name("Nord").unwrap(),
            Some("ghost"),
        );
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn colors_for_falls_back_to_global_when_the_profile_sets_no_scheme() {
        // A profile without a scheme is today's profile: its tabs
        // follow the global pick.
        let config = chain_config(&[chain_profile("ssh-prod", None)]);
        let colors = colors_for(
            &config,
            &theme_source::by_name("Nord").unwrap(),
            Some("ssh-prod"),
        );
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn colors_for_falls_back_to_global_on_an_unknown_scheme_name() {
        // A hand-edited config can name a scheme the registry does
        // not ship ("Catppuccin Frappe" is a real upstream variant):
        // warn and resolve global — never an error, never a rewrite.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Catppuccin Frappe"))]);
        let colors = colors_for(
            &config,
            &theme_source::by_name("Nord").unwrap(),
            Some("ssh-prod"),
        );
        assert_eq!(colors[NamedColor::Background], Some(rgb(0x2E_3440)));
    }

    #[test]
    fn one_config_pass_resolves_mixed_tabs_to_different_colors() {
        // The apply criterion's headless pin: one config, one global
        // pick, two recorded ids (a profile tab and a plain tab) —
        // the per-tab resolutions differ, so one `apply_config_to_tabs`
        // pass paints them differently.
        let config = chain_config(&[chain_profile("ssh-prod", Some("Dracula"))]);
        let global = theme_source::by_name("Nord").unwrap();
        let profile_tab = colors_for(&config, &global, Some("ssh-prod"));
        let plain_tab = colors_for(&config, &global, None);
        assert_ne!(
            profile_tab[NamedColor::Background],
            plain_tab[NamedColor::Background]
        );
        assert_ne!(profile_tab[NamedColor::Red], plain_tab[NamedColor::Red]);
    }

    // --- the clipboard anchor ---
    //
    // libcosmic binds its single runtime Clipboard to the first
    // window created and drops that binding — and with it the
    // smithay-clipboard connection holding any copied selection —
    // when the owning surface is destroyed. DropTerm's only surface
    // is the layer shell, which every hide destroys, so a copy could
    // never survive the focus switch to wherever it would be pasted.
    // An invisible, never-closed normal window created first owns
    // the binding forever; these pin the two invariants that keep it
    // both invisible and immortal.

    #[test]
    fn clipboard_anchor_window_is_invisible() {
        // Unmapped: absent from the dock and Alt-Tab, and it never
        // paints — it exists only to hold the clipboard binding.
        assert!(!clipboard_anchor_settings().visible);
    }

    #[test]
    fn clipboard_anchor_window_cannot_be_closed() {
        // No close request is ever honored (and `exit_on_close(false)`
        // in main means the runtime cannot exit through it either),
        // so the binding outlives every hide/show cycle.
        assert!(!clipboard_anchor_settings().exit_on_close_request);
    }

    // --- the pure theme-preference decisions ---

    #[test]
    fn effective_is_dark_pins_dark_and_light() {
        // A pin wins regardless of the system mode — that is the
        // whole point of the feature.
        assert!(effective_is_dark(ThemePreference::Dark, false));
        assert!(effective_is_dark(ThemePreference::Dark, true));
        assert!(!effective_is_dark(ThemePreference::Light, false));
        assert!(!effective_is_dark(ThemePreference::Light, true));
    }

    #[test]
    fn effective_is_dark_system_follows_the_injected_mode() {
        // The system value is a parameter, not a global read (the
        // same pattern as `window_bg_color`): the decision stays
        // pure, and tests never depend on the process-wide THEME
        // state.
        assert!(effective_is_dark(ThemePreference::System, true));
        assert!(!effective_is_dark(ThemePreference::System, false));
    }

    #[test]
    fn echo_retitles_on_a_title_mode_flip_regardless_of_profiles() {
        // The mode-flip guard's standing rule: every mode reads
        // different inputs, so a flip always re-resolves every live
        // tab — even when the profiles list did not move.
        assert!(echo_retitles_tabs(
            TabTitleMode::Shell,
            TabTitleMode::Profile,
            &[],
            &[]
        ));
        assert!(echo_retitles_tabs(
            TabTitleMode::Profile,
            TabTitleMode::Shell,
            &[chain_profile("ssh", None)],
            &[chain_profile("ssh", None)]
        ));
    }

    #[test]
    fn echo_retitles_on_a_profiles_change_only_in_profile_mode() {
        // Ruling 1: a profile renamed or deleted under live tabs is a
        // stale name the moment it happens — while the (new) mode is
        // Profile, a profiles-list change retitles. In every other
        // mode the titles read no profile name, so the same change
        // retitles nothing.
        let old = vec![chain_profile("ssh", None)];
        let renamed = vec![chain_profile("server", None)];
        assert!(echo_retitles_tabs(
            TabTitleMode::Profile,
            TabTitleMode::Profile,
            &old,
            &renamed
        ));
        assert!(!echo_retitles_tabs(
            TabTitleMode::Shell,
            TabTitleMode::Shell,
            &old,
            &renamed
        ));
        assert!(!echo_retitles_tabs(
            TabTitleMode::LastSegment,
            TabTitleMode::LastSegment,
            &old,
            &renamed
        ));
    }

    #[test]
    fn echo_never_retitles_on_an_unrelated_change() {
        // The anti-churn pin: an echo that moves neither the mode nor
        // (in Profile mode) the profiles list — an opacity slide, a
        // keybind, our own profile write's echo before and after —
        // retitles nothing.
        let profiles = vec![chain_profile("ssh", None)];
        assert!(!echo_retitles_tabs(
            TabTitleMode::Shell,
            TabTitleMode::Shell,
            &profiles,
            &profiles.clone()
        ));
        assert!(!echo_retitles_tabs(
            TabTitleMode::Profile,
            TabTitleMode::Profile,
            &profiles,
            &profiles.clone()
        ));
        // The empty list is a value like any other: a config that
        // gains its first profile retitles in Profile mode...
        assert!(echo_retitles_tabs(
            TabTitleMode::Profile,
            TabTitleMode::Profile,
            &[],
            &profiles
        ));
        // ...and loses its last without one.
        assert!(echo_retitles_tabs(
            TabTitleMode::Profile,
            TabTitleMode::Profile,
            &profiles,
            &[]
        ));
    }

    #[test]
    fn theme_override_is_none_when_preference_is_unchanged() {
        // Steady state issues nothing: a default `System` config
        // never calls `set_theme`, and unrelated config echoes —
        // including our own theme echo — never storm the task.
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::System),
            None
        );
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::Dark),
            None
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::Light),
            None
        );
    }

    #[test]
    fn theme_override_pins_on_any_transition_to_dark_or_light() {
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::Dark),
            Some(ThemeOverrideKind::PinDark)
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::Dark),
            Some(ThemeOverrideKind::PinDark)
        );
        assert_eq!(
            theme_override(ThemePreference::System, ThemePreference::Light),
            Some(ThemeOverrideKind::PinLight)
        );
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::Light),
            Some(ThemeOverrideKind::PinLight)
        );
    }

    #[test]
    fn theme_override_restores_follow_system_from_a_pin() {
        assert_eq!(
            theme_override(ThemePreference::Dark, ThemePreference::System),
            Some(ThemeOverrideKind::FollowSystem)
        );
        assert_eq!(
            theme_override(ThemePreference::Light, ThemePreference::System),
            Some(ThemeOverrideKind::FollowSystem)
        );
    }

    /// Drives a [`Task`](cosmic::app::Task)'s stream to completion on
    /// the current thread, collecting every runtime
    /// [`Action::Output`] message in delivery order. `Task::then`
    /// consumes the first task's output and replaces it with the
    /// continuation's, so only a stream-level check can prove a
    /// message actually ships.
    fn delivered_outputs<T: Send + 'static>(task: cosmic::app::Task<T>) -> Vec<cosmic::Action<T>> {
        use cosmic::iced::futures::StreamExt;
        use cosmic::iced::futures::executor::block_on;

        let mut stream = cosmic::iced::runtime::task::into_stream(task)
            .expect("theme override task has a stream");
        let mut outputs = Vec::new();
        block_on(async {
            while let Some(action) = stream.next().await {
                if let cosmic::iced::runtime::Action::Output(message) = action {
                    outputs.push(message);
                }
            }
        });
        outputs
    }

    #[test]
    fn theme_override_task_delivers_set_theme_then_reapply() {
        // `Task::then` would CONSUME the `set_theme` output — the
        // pin message would never reach the runtime, the process
        // theme would never flip, and pinning Dark would leave a
        // light, frosted terminal. The task must deliver BOTH
        // messages: the theme override first, the `ReapplyColors`
        // echo second — `Task::chain`'s contract.
        for (kind, expected_prefer) in [
            (ThemeOverrideKind::PinDark, Some(true)),
            (ThemeOverrideKind::PinLight, Some(false)),
            // FollowSystem's palette is whatever the system prefers —
            // only the cleared marker is environment-independent.
            (ThemeOverrideKind::FollowSystem, None),
        ] {
            let outputs = delivered_outputs(theme_override_task(kind));
            assert_eq!(
                outputs.len(),
                2,
                "expected exactly the set_theme + ReapplyColors pair for {kind:?}"
            );
            let cosmic::Action::Cosmic(cosmic::app::Action::AppThemeChange(theme)) = &outputs[0]
            else {
                panic!("first delivered message must be the theme override for {kind:?}");
            };
            let cosmic::theme::ThemeType::System { prefer_dark, .. } = &theme.theme_type else {
                panic!("override theme must be System-typed for {kind:?}");
            };
            assert_eq!(*prefer_dark, expected_prefer, "marker for {kind:?}");
            assert!(
                matches!(&outputs[1], cosmic::Action::App(Message::ReapplyColors)),
                "second delivered message must be the ReapplyColors echo for {kind:?}"
            );
        }
    }

    // --- bar placement + tooltip side ---

    #[test]
    fn effective_bar_at_bottom_auto_keeps_the_strip_at_the_leading_edge() {
        // Auto placement: the strip stacks opposite the screen edge
        // — surface bottom under a Top anchor, surface top under a
        // Bottom anchor.
        assert!(effective_bar_at_bottom(Edge::Top, TabPosition::Auto));
        assert!(!effective_bar_at_bottom(Edge::Bottom, TabPosition::Auto));
    }

    #[test]
    fn effective_bar_at_bottom_top_pin_places_the_bar_at_the_top() {
        // A pin wins regardless of anchor.
        assert!(!effective_bar_at_bottom(Edge::Top, TabPosition::Top));
        assert!(!effective_bar_at_bottom(Edge::Bottom, TabPosition::Top));
    }

    #[test]
    fn effective_bar_at_bottom_bottom_pin_places_the_bar_at_the_bottom() {
        assert!(effective_bar_at_bottom(Edge::Top, TabPosition::Bottom));
        assert!(effective_bar_at_bottom(Edge::Bottom, TabPosition::Bottom));
    }

    #[test]
    fn tooltip_side_pops_terminal_ward_wherever_the_bar_sits() {
        // The tip pops to the terminal's side of the strip: below a
        // top bar, above a bottom bar — the anchor edge is not an
        // input.
        assert_eq!(bar_tooltip_side(false), TooltipSide::Below);
        assert_eq!(bar_tooltip_side(true), TooltipSide::Above);
    }

    #[test]
    fn tooltip_side_auto_layouts_match_today_on_both_edges() {
        // Regression pins: the derived side must match the fixed
        // `Auto`-layout values on both edges.
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Top, TabPosition::Auto)),
            TooltipSide::Above
        );
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Bottom, TabPosition::Auto)),
            TooltipSide::Below
        );
    }

    #[test]
    fn tooltip_side_pinned_layouts_follow_the_bar_not_the_anchor() {
        // A strip pinned to the screen edge still tips terminal-ward.
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Top, TabPosition::Top)),
            TooltipSide::Below
        );
        assert_eq!(
            bar_tooltip_side(effective_bar_at_bottom(Edge::Bottom, TabPosition::Bottom)),
            TooltipSide::Above
        );
    }
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
        let mut settings_tab_model = settings::tab_strip_model();
        settings::activate_tab_row(
            &mut settings_tab_model,
            strip_selection(home_settings_page()),
        );

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
            surface_focused: false,
            key_binds,
            keybind_capture: None,
            keybind_search: String::new(),
            keybind_conflict: None,
            last_toggle: None,
            slide: None,
            settings_open: false,
            settings_page: home_settings_page(),
            settings_tab_model,
            profile_selection: None,
            profile_draft: None,
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

        let opacity = f32::from(self.config.opacity) / 100.0;
        // Pin the terminal's height to the granted window minus the bars'
        // strips — both always reserved, so the 1↔2 tab crossing never
        // resizes the terminal (see `surface::terminal_view_height`).
        let terminal_height = self.window_height.map_or(Length::Fill, |h| {
            Length::Fixed(surface::terminal_view_height(h))
        });
        let terminal: Element<'_, Message> = TerminalBox::new(&tab.terminal, &self.key_binds)
            .id(self.terminal_id.clone())
            .height(terminal_height)
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
                TabMenuPick::SwitchProfile(index) => Message::SwitchTabProfile(tab, index),
                TabMenuPick::Move(direction) => Message::TabMove(tab, direction),
                TabMenuPick::Close => Message::TabClosed(tab),
                TabMenuPick::CloseOthers => Message::CloseOtherTabs(tab),
                TabMenuPick::CloseRight => Message::CloseTabsRight(tab),
            })
            .tooltip_side(tooltip_side);
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
        // Stacking: the grab bar is fixed at the leading edge —
        // opposite the screen edge, where all of `geometry.rs`'s
        // drag/anchor math expects it — the terminal fills the
        // middle, and the tab strip takes whichever side
        // `effective_bar_at_bottom` picks. Both bars take their
        // strips out of the terminal's pinned height (see
        // `surface::terminal_view_height` — subtracted
        // order-agnostically), so neither layout changes the
        // terminal's pixel height. Fill dimensions keep iced's flex
        // from deferring the bars past the terminal's allocation: a
        // Shrink cross-axis makes a fixed-height, fill-width bar a
        // fourth-pass child, so the Fill terminal would take the
        // whole surface and push the bars below its bottom edge.
        let column: Element<'_, Message> = match (self.config.edge, bar_at_bottom) {
            (Edge::Top, false) => widget::column![bar, terminal, grab_bar]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (Edge::Top, true) => widget::column![terminal, bar, grab_bar]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (Edge::Bottom, false) => widget::column![grab_bar, bar, terminal]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (Edge::Bottom, true) => widget::column![grab_bar, terminal, bar]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        };

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
        // The drawer hosts the tab pages plus the Profiles drill-down
        // its entry row flips to (settings-tabs criterion 2). The
        // flip is this switch — title plus content at the one
        // hand-composed site; the Close button keeps routing through
        // the same `Action::Settings` toggle from any page, so the
        // keybind/Close/gear funnel never changes under a page turn.
        // The title stays "Settings" on every tab — the strip
        // disambiguates.
        let (title, drawer_content) = match self.settings_page {
            SettingsPage::Tab(SettingsTab::General) => (
                "Settings",
                settings::general_view(&self.config, &self.settings_buffer),
            ),
            SettingsPage::Tab(SettingsTab::Advanced) => (
                "Settings",
                settings::advanced_view(&self.config, &self.settings_buffer),
            ),
            SettingsPage::Tab(SettingsTab::Keybindings) => (
                "Settings",
                settings::keybindings_view(
                    &self.config.custom_key_binds,
                    &self.keybind_search,
                    self.keybind_capture,
                    self.keybind_conflict,
                ),
            ),
            SettingsPage::Profiles => (
                "Profiles",
                settings::profiles_view(
                    &self.config,
                    self.profile_selection,
                    self.profile_draft.as_ref(),
                ),
            ),
        };
        // The tab strip rides the drawer's header slot — pinned under
        // the title, above the pane's scrollable — on every page,
        // including the Profiles drill-down (a pick from there turns
        // straight to the picked tab, per `page_after_tab_pick`).
        // Rendered from the stored model (never rebuilt per frame);
        // activation carries the row's entity back as a message —
        // the closure is `'static` and cannot borrow the model — and
        // the handler resolves it (criterion 7). 32px like
        // cosmic-edit's doc tabs; the TabBar preset's 76-250px
        // button band fits the 344px drawer floor (OQ-2's cap ruling
        // happens at the hardware check).
        let tab_strip = widget::tab_bar::horizontal(&self.settings_tab_model)
            .button_height(32)
            .on_activate(Message::PickSettingsTab);
        let drawer: Element<'_, Message> = widget::context_drawer(
            Some(title.into()),
            None,
            Some(tab_strip.into()),
            None,
            Message::Action(Action::Settings),
            content,
            settings::drawer_with_handle(drawer_content).map(Message::Settings),
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
                    return theme_override_task(kind);
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
                    return Task::none();
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
                    return geometry::apply_geometry(*id, new_geom, output);
                }
            }
            Message::ReapplyColors => {
                // The theme override landed: re-run the idempotent
                // color apply so `refresh_window_bg_color` derives
                // the fallback background from the new global mode;
                // cells were already correct via `effective_is_dark`.
                self.apply_config_to_tabs();
            }
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
                // below runs exactly as today.
                if drawer_escape_routing(self.settings_open, &key) == DrawerKeyRouting::CloseDrawer
                {
                    return self.close_settings_drawer();
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
            Message::TabMove(id, direction) => {
                return self.move_tab_by(id, direction);
            }
            Message::CloseOtherTabs(id) => {
                return self.close_tabs_sweep(id, CloseSweep::Others);
            }
            Message::CloseTabsRight(id) => {
                return self.close_tabs_sweep(id, CloseSweep::Right);
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
                    let geom = self.settled_geometry(&drag, output);
                    // Write-only-if-changed: a drag that settles back
                    // on the config's current percent persists nothing.
                    if geom.height_percent != self.config.height_percent {
                        self.config.height_percent = geom.height_percent;
                        let generation = self.write_back.schedule(geom.height_percent).generation;
                        return Task::batch([
                            geometry::apply_geometry(*id, geom, output),
                            Task::perform(
                                tokio::time::sleep(geometry::WRITE_BACK_DEBOUNCE),
                                move |()| cosmic::Action::App(Message::WriteDue(generation)),
                            ),
                        ]);
                    }
                    return geometry::apply_geometry(*id, geom, output);
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
                // criterion 7), then the `OpenProfiles` rule for
                // picks (criterion 4): the page turns, nothing else —
                // no drawer close, no buffer clear, no
                // `DrawerTransition` — and the model's selection
                // follows the page. An entity the model does not know
                // (impossible while rows are only `SettingsTab::ALL`)
                // resolves to nothing: no turn, no invented tab.
                if let Some(tab) = self.settings_tab_model.data::<SettingsTab>(entity) {
                    self.settings_page = page_after_tab_pick(self.settings_page, *tab);
                    self.sync_settings_tab_strip();
                }
            }
            Message::Settings(event) => {
                // The settings drawer's commit path: every event
                // resolves through a pure planner and lands as one
                // `Bare` write via [`Self::write_config`] — the
                // drawer never applies anything itself, the
                // `ConfigChanged` echo drives every apply.
                // Display-only events (drafts, slider moves) touch
                // just the buffer; commits that fail to parse are
                // no-ops that leave the draft for the user to fix.
                match event {
                    settings::SettingsEvent::Draft(field, text) => {
                        self.settings_buffer.set_text(field, text);
                    }
                    settings::SettingsEvent::Submitted(field, text) => {
                        if let Some(write) = settings::submit_write(field, &text) {
                            // A parsed submit clears its draft so the
                            // input falls back to the echoed config
                            // value; a rejected one keeps the draft.
                            self.settings_buffer.clear_text(field);
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::SliderMoved(key, value) => {
                        self.settings_buffer.set_slider(key, value);
                    }
                    settings::SettingsEvent::SliderReleased(key) => {
                        // `on_release` carries no value (the pinned
                        // rev's API), so the release commits the
                        // buffered position the readout showed.
                        let value = self.settings_buffer.slider_value(key, &self.config);
                        if let Some(write) = settings::slider_release_write(key, value) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::Picked(key, index) => {
                        if let Some(write) = settings::dropdown_write(key, index) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::Toggled(key, checked) => {
                        if let Some(write) = settings::toggle_write(key, checked) {
                            self.write_config(settings::WriteKind::Bare, &write);
                        }
                    }
                    settings::SettingsEvent::OpenProfiles => {
                        // A page turn like a pick (criterion 5); the
                        // strip keeps highlighting General — the tab
                        // Back lands on.
                        self.settings_page = SettingsPage::Profiles;
                        self.sync_settings_tab_strip();
                    }
                    settings::SettingsEvent::KeybindSearchChanged(text) => {
                        // Display only: the next view build re-filters
                        // the Keybindings tab's rows.
                        self.keybind_search = text;
                    }
                    settings::SettingsEvent::CaptureStart(action) => {
                        // Arm the capture: from here the KeyPressed
                        // arm's first gate owns every press. A fresh
                        // arm clears any standing conflict error.
                        self.keybind_capture = Some(action);
                        self.keybind_conflict = None;
                    }
                    settings::SettingsEvent::CaptureCancel => {
                        self.keybind_capture = None;
                    }
                    settings::SettingsEvent::RevertKeybind(action) => {
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
                    settings::SettingsEvent::BackFromProfiles => {
                        self.leave_profiles_page();
                    }
                    settings::SettingsEvent::SelectProfile(index) => {
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
                    settings::SettingsEvent::PickDefaultProfile(index) => {
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
                    settings::SettingsEvent::OpenProfileDraft => {
                        self.profile_draft = Some(settings::ProfileDraft::add());
                    }
                    settings::SettingsEvent::CloneProfile(index) => {
                        // Seeding only — no write until the draft
                        // commits.
                        if let Some(source) = self.config.profiles.get(index) {
                            self.profile_draft = Some(settings::ProfileDraft::clone_of(
                                source,
                                &self.config.profiles,
                            ));
                        }
                    }
                    settings::SettingsEvent::DeleteProfile(index) => {
                        // Immediate delete, no dialog (OQ-1): one
                        // whole-list write, plus the paired auto-clear
                        // when the deleted profile was the default.
                        // Page state follows the shrunken list
                        // through the pure adjusters.
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
                    }
                    settings::SettingsEvent::ReorderProfile(index, direction) => {
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
                    settings::SettingsEvent::DraftProfileField(field, text) => {
                        // Display-only buffering; the keystroke also
                        // clears any stale error.
                        if let Some(draft) = self.profile_draft.as_mut() {
                            draft.set_field_text(field, text);
                        }
                    }
                    settings::SettingsEvent::CommitProfileField(field, text) => {
                        // The per-field commit: plan purely, then land
                        // one whole-list write (plus the paired
                        // rename-cascade write). Invalid Enters keep
                        // the draft and show the inline error.
                        if let Some(draft) = self.profile_draft.clone() {
                            match settings::plan_profile_commit(
                                &draft,
                                field,
                                &text,
                                &self.config.profiles,
                                self.config.default_profile.as_deref(),
                            ) {
                                settings::ProfileCommitPlan::Write {
                                    profiles,
                                    default_after_rename,
                                } => {
                                    // `profiles` first, then the
                                    // cascade — the spec's two-key
                                    // order; read-side tolerance
                                    // covers the crash window.
                                    self.write_profile_config(&settings::ProfileWrite::Profiles(
                                        profiles.clone(),
                                    ));
                                    if let Some(new_default) = default_after_rename {
                                        self.write_profile_config(
                                            &settings::ProfileWrite::DefaultProfile(Some(
                                                new_default,
                                            )),
                                        );
                                    }
                                    match draft.mode {
                                        settings::ProfileDraftMode::Add
                                        | settings::ProfileDraftMode::Clone { .. } => {
                                            // The profile entered the
                                            // list: select it and
                                            // continue as its edit
                                            // draft.
                                            let index = profiles.len() - 1;
                                            self.profile_selection = Some(index);
                                            self.profile_draft =
                                                Some(settings::ProfileDraft::edit(
                                                    index,
                                                    &profiles[index],
                                                ));
                                        }
                                        settings::ProfileDraftMode::Edit { index } => {
                                            // Keep the draft open
                                            // with its other fields'
                                            // buffered text; the
                                            // committed field falls
                                            // back to the written
                                            // value.
                                            let mut draft = draft;
                                            draft.field_committed(field, &profiles[index]);
                                            self.profile_draft = Some(draft);
                                        }
                                    }
                                }
                                settings::ProfileCommitPlan::Invalid { field, error } => {
                                    if let Some(draft) = self.profile_draft.as_mut() {
                                        draft.error = Some((field, error));
                                    }
                                }
                                settings::ProfileCommitPlan::StaleEdit => {
                                    // The edited profile is gone: the
                                    // draft (and its selection) drop.
                                    self.profile_selection = None;
                                    self.profile_draft = None;
                                }
                            }
                        }
                    }
                    settings::SettingsEvent::PickProfileScheme(pick) => {
                        // Dropdowns commit on change (the `Picked`
                        // rule): an Edit draft's pick writes the base
                        // profile with only the scheme replaced — the
                        // one field a pick can never reject — while
                        // Add/Clone drafts only buffer it (no profile
                        // exists to write until the first valid triple
                        // commit carries it). An out-of-range pick is
                        // ignored — the options are the only writable
                        // vocabulary, never an invented name; a stale
                        // edit index drops the draft (`StaleEdit`'s
                        // rule).
                        if let Some(draft) = self.profile_draft.clone()
                            && let Some(pick) = settings::profile_scheme_write(pick)
                        {
                            let scheme = pick.field_value();
                            if let settings::ProfileDraftMode::Edit { index } = draft.mode
                                && self.config.profiles.get(index).is_none()
                            {
                                self.profile_selection = None;
                                self.profile_draft = None;
                            } else {
                                if let settings::ProfileDraftMode::Edit { index } = draft.mode
                                    && let Some(base) = self.config.profiles.get(index)
                                {
                                    let mut edited = base.clone();
                                    edited.color_scheme.clone_from(&scheme);
                                    self.write_profile_config(&settings::ProfileWrite::Profiles(
                                        settings::apply_edit(&self.config.profiles, index, edited),
                                    ));
                                }
                                if let Some(open) = self.profile_draft.as_mut() {
                                    open.scheme = scheme;
                                    open.error = None;
                                }
                            }
                        }
                    }
                    settings::SettingsEvent::AddProfileEnvRow => {
                        // A buffered list edit, not a write: the row
                        // joins the draft (an Enter in any env input
                        // writes the whole map); any stale error
                        // clears — the row list it described is gone.
                        if let Some(draft) = self.profile_draft.as_mut() {
                            draft.env_rows.push(settings::ProfileEnvRow::default());
                            draft.error = None;
                        }
                    }
                    settings::SettingsEvent::RemoveProfileEnvRow(index) => {
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
                            draft.error = None;
                        }
                    }
                    settings::SettingsEvent::DrawerResize(pointer_x) => {
                        // Live width (criterion 2): display-only while
                        // the press is held — the same band render
                        // clamps through, so the drag can never leave
                        // it. Nothing writes mid-drag.
                        self.drawer_drag = Some(settings::drawer_drag_px(
                            pointer_x,
                            self.current_surface_width(),
                        ));
                    }
                    settings::SettingsEvent::DrawerResizeReleased => {
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
                    settings::SettingsEvent::DrawerReset => {
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
