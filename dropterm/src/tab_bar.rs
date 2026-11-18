// SPDX-License-Identifier: GPL-3.0-only

//! The tab strip on GPUI (gpui-tabs): the bar's view model over
//! [`dropterm::core::tabs`]' unchanged model — every behavior (title
//! resolution and fallback, the title prefix's pin and attention
//! markers, insert positions, activation, attention clearing) is a
//! core call; this module only maps tab state to presentation and
//! renders it. The presentation rules port `src/tab_bar.rs` (the
//! libcosmic bar, read-only reference): the strip is always present
//! carrying the `+` and gear affordances at its right corner, tab
//! rows render from two tabs on, the strip's paint derives from the
//! terminal pane's background (a slightly darkened strip, a
//! luminance-readable foreground), and the border hairline sits on
//! the terminal-facing edge.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};

use dropterm::core::config::{Edge, TabPosition, TabTitleMode};
use dropterm::core::tabs::{
    Attention, CloseDecision, CloseSweep, MoveDirection, TabId, close_tab, commit_rename,
    insert_new_tab, move_target, next_default_title, reorder_tabs, resolve_title,
    shell_title_from_report, sweep_targets, title_prefix,
};

// The inline rename's Escape (bound in the strip's key context; the
// Kit's input propagates an Escape it does not consume, so the
// binding reaches the cancellation from the focused input).
gpui::actions!(dropterm_gpui, [CancelTabRename]);

/// A tab row's drag payload (drag-reorder): the dragged tab's id —
/// the drop target's own row decides the landing index.
#[derive(Clone, Copy)]
pub(crate) struct TabDrag(pub(crate) TabId);

/// The drag's floating chip: the lifted row's label following the
/// cursor while it is held.
struct TabDragChip {
    label: String,
    fg: gpui::Hsla,
}

impl gpui::Render for TabDragChip {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
            .px(gpui::px(BUTTON_PADDING_X))
            .py(gpui::px(4.))
            .rounded_sm()
            .bg(gpui::Hsla { a: 0.18, ..self.fg })
            .text_color(self.fg)
            .text_size(gpui::px(TITLE_FONT_SIZE))
            .child(self.label.clone())
    }
}

/// The strip's height in logical pixels — the pane yields this much of
/// the surface to the bar (the surface itself never grows for it; the
/// reference bar's own `BAR_HEIGHT` rule, ported).
pub(crate) const BAR_HEIGHT: f32 = 32.0;

/// Whether the strip sits at the surface's bottom — the
/// `TabPosition` layout rule ported from the libcosmic app's
/// `effective_bar_at_bottom`: `Auto` places the strip opposite the
/// anchored edge (at the surface's free side), while `Top`/`Bottom`
/// pin it to that side regardless of anchor.
pub(crate) fn bar_at_bottom(edge: Edge, tab_position: TabPosition) -> bool {
    match tab_position {
        TabPosition::Auto => matches!(edge, Edge::Top),
        TabPosition::Top => false,
        TabPosition::Bottom => true,
    }
}

/// Whether the tab row renders its buttons: the strip is always
/// present, but tab rows — and their close marks — only from two tabs
/// on; the single-tab strip carries the corner affordances alone (the
/// reference bar's rule).
pub(crate) fn shows_tab_buttons(tab_count: usize) -> bool {
    tab_count >= 2
}

/// A color's relative luminance (the WCAG definition) — the reference
/// bar's own one flip formula, ported: the strip's foreground picks by
/// it over the pane background.
pub(crate) fn relative_luminance(r: f32, g: f32, b: f32) -> f32 {
    fn channel(c: f32) -> f32 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// An 8-bit channel triple as GPUI's color.
pub(crate) fn hsla(r: u8, g: u8, b: u8) -> gpui::Hsla {
    gpui::Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

/// The strip's paint pair — (background, foreground) — derived from
/// the terminal pane's effective background exactly like the reference
/// bar's `pane_colors`: the strip slightly darkened (×0.92, so the
/// bar reads as its own band instead of blending into the terminal),
/// and a foreground picked near-black or near-white by the
/// *background's* luminance, so labels and glyphs stay readable in
/// either scheme. `None` (no scheme background) falls back to
/// black/near-white — the dark-drawer default.
pub(crate) fn strip_pair(
    background: Option<alacritty_terminal::vte::ansi::Rgb>,
) -> (gpui::Hsla, gpui::Hsla) {
    let Some(bg) = background else {
        return (hsla(0, 0, 0), hsla(0xF5, 0xF5, 0xFF));
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a darkened 0..=255 channel: round keeps the nearest u8 and the value stays in range by construction"
    )]
    let dimmed = |channel: u8| (f32::from(channel) * 0.92).round() as u8;
    let luminance = relative_luminance(
        f32::from(bg.r) / 255.,
        f32::from(bg.g) / 255.,
        f32::from(bg.b) / 255.,
    );
    let foreground = if luminance > 0.5 {
        hsla(0x1B, 0x1B, 0x22)
    } else {
        hsla(0xF5, 0xF5, 0xFF)
    };
    (hsla(dimmed(bg.r), dimmed(bg.g), dimmed(bg.b)), foreground)
}

/// One rendered tab row: the pure presentation mapping of one tab
/// record (the headless-testable half of the bar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowVm {
    pub(crate) id: TabId,
    /// The row's whole label: core's title prefix (pin, then
    /// attention marker) composed ahead of the resolved title.
    pub(crate) label: String,
    pub(crate) is_active: bool,
    pub(crate) is_pinned: bool,
    pub(crate) attention: Attention,
}

/// One tab's plain state — the GPUI-side record the shell's tab list
/// carries. Deliberately not core's `Tab`: that record owns a live
/// `Terminal` plus the session/paste state the libcosmic app tracks;
/// this frontend's pane owns the engine, so the record keeps only the
/// presentation state, with every transition resolved through core's
/// own calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabRecord {
    pub(crate) id: TabId,
    /// The resolved display title (core's `resolve_title` output,
    /// maintained at every apply point).
    pub(crate) title: String,
    /// The last OSC 0/2 report (`None` until the first, cleared by a
    /// reset or an empty payload) — the retitle input, stored by
    /// core's own rule.
    pub(crate) shell_title: Option<String>,
    /// The tab's working directory (the engine's `/proc` cwd read,
    /// the same input the libcosmic retitle path caches) — the cwd
    /// title modes' input.
    pub(crate) cwd: Option<PathBuf>,
    /// The tab's user rename (the inline rename's commit), which
    /// resolution never lets a shell report clobber (core's override
    /// rule).
    pub(crate) title_override: Option<String>,
    pub(crate) pinned: bool,
    pub(crate) attention: Attention,
    /// The last comm sampled running on this tab (gn-2, the frozen
    /// duration's pairing name — command-lifecycle OQ-1): per-tab,
    /// so a switch can never pair one tab's name with another's
    /// duration; an empty name is the honest floor.
    pub(crate) last_running_comm: Option<String>,
    /// The tab's pasted-image ledger (paste-ledger spec; gpui-parity
    /// Task 2): core's row type, one per image paste, existence-
    /// backed and deliberately not serialized (the rows die with the
    /// tab while the files outlive them in the paste cache).
    pub(crate) pasted_images: Vec<dropterm::core::tabs::PastedImage>,
    /// The profile id this tab launched under, if any (py-5): set at
    /// spawn — never on a plain or degraded arm — saved by the
    /// session and re-resolved by id at restore, exactly core's
    /// `Tab::profile` semantics on this frontend's record.
    pub(crate) profile: Option<String>,
    /// The session-ephemeral flag a `--cmd` spawn carries (py-5): the
    /// session saves the profile id and never this, the two facts
    /// being each other's opposite.
    pub(crate) ephemeral: bool,
    /// Whether this tab already posted a bell notification during the
    /// current hide period (gpui-parity Task 3): armed when a
    /// notification is posted, cleared for every tab when the surface
    /// is shown — so a bell storm yields one notification per tab per
    /// hide, and the next bell after a show notifies again.
    pub(crate) bell_notified: bool,
}

/// Resolves one record's display title through core — the one resolve
/// call every apply point shares (a report landing or clearing, a cwd
/// move, a mode flip, a fresh spawn, a rename commit). The profile
/// input stays `None` until a profile spawn path exists (the chrome
/// slices); core's own fallback covers it.
fn resolve_record_title(tab: &TabRecord, mode: TabTitleMode) -> String {
    resolve_title(
        mode,
        tab.shell_title.as_deref(),
        tab.cwd.as_deref(),
        None,
        tab.id,
        tab.title_override.as_deref(),
    )
}

/// The shell's tab list: plain records plus the active index, with
/// every list behavior a [`dropterm::core::tabs`] call — insert
/// positions through `insert_new_tab`, activation and attention
/// clearing through core's `Attention` transition, titles through
/// `resolve_title`. The bar renders from it; nothing here duplicates
/// the model.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TabList {
    tabs: Vec<TabRecord>,
    active: usize,
    next_id: u32,
}

impl TabList {
    /// Spawns a fresh tab record (the id-numbered `Terminal N`
    /// default title) at the configured position and activates it,
    /// returning its id. Insertion and activation are core's
    /// `insert_new_tab` semantics: `after_current` inserts beside the
    /// active tab (clamped to the end), else appends.
    pub(crate) fn spawn(&mut self, after_current: bool) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        let tab = TabRecord {
            id,
            title: next_default_title(id),
            shell_title: None,
            cwd: None,
            title_override: None,
            pinned: false,
            attention: Attention::None,
            last_running_comm: None,
            pasted_images: Vec::new(),
            bell_notified: false,
            profile: None,
            ephemeral: false,
        };
        self.active = insert_new_tab(&mut self.tabs, self.active, after_current, tab);
        id
    }

    /// Records the spawn facts on the tab with `id` (py-5): the
    /// profile id the spawn ran under — `None` on every plain and
    /// degraded arm — and the session-ephemeral flag a `--cmd` spawn
    /// carries. Called by the one spawning body right after the pane
    /// boots; an unknown id records nothing.
    pub(crate) fn note_spawn_facts(&mut self, id: TabId, profile: Option<String>, ephemeral: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.profile = profile;
            tab.ephemeral = ephemeral;
        }
    }

    /// The profile id the tab with `id` launched under, `None` for an
    /// unknown id or a plain spawn — the session capture's and the
    /// band's profile-seat input (the caller resolves the id against
    /// the live config, never a stale name).
    pub(crate) fn profile_id_of(&self, id: TabId) -> Option<&str> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.profile.as_deref())
    }

    /// Applies an in-place respawn's field half (py-5, core's
    /// `plan_respawn` product): the resolved title through the
    /// profile name, the cleared shell-title cache, the kept rename
    /// pin, the cleared attention and bell latch, and the persistent
    /// profile seat. An unknown id applies nothing (the caller keeps
    /// the live tab).
    pub(crate) fn apply_respawn(&mut self, id: TabId, fields: dropterm::core::tabs::RespawnFields) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.title = fields.title;
            tab.shell_title = None;
            tab.title_override = fields.title_override;
            tab.attention = Attention::None;
            tab.bell_notified = false;
            tab.profile = fields.profile;
            tab.ephemeral = fields.ephemeral;
        }
    }

    /// The active tab's id, `None` while the list is empty.
    pub(crate) fn active_id(&self) -> Option<TabId> {
        self.tabs.get(self.active).map(|tab| tab.id)
    }

    /// The active tab's position — `(index, total)`, the status
    /// band's tab segment input; `None` while the list is empty.
    pub(crate) fn position(&self) -> Option<(usize, usize)> {
        Some((self.active, self.tabs.len())).filter(|&(_, total)| total > 0)
    }

    /// Records the comm sampled running on `id` (the band's frozen
    /// pairing input); an unknown id records nothing.
    pub(crate) fn note_running_comm(&mut self, id: TabId, comm: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.last_running_comm = Some(comm);
        }
    }

    /// The comm last sampled running on `id`, the frozen duration's
    /// pairing name.
    pub(crate) fn running_comm(&self, id: TabId) -> Option<String> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.last_running_comm.clone())
    }

    /// Activates the tab with `id` — clearing its attention marker
    /// through core's transition — returning whether the active tab
    /// changed. An unknown id, or the already-active tab, changes
    /// nothing.
    pub(crate) fn select(&mut self, id: TabId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if index == self.active {
            return false;
        }
        self.active = index;
        let tab = &mut self.tabs[index];
        tab.attention = tab.attention.on_activated();
        true
    }

    /// Stores the tab's current title inputs (the pane's own OSC 0/2
    /// tracker and the engine's cwd read, both polled) and re-resolves
    /// its display title when either moved, returning whether a
    /// re-resolution ran — the title itself may land on the same value
    /// (Numbered mode ignores reports, so a report arriving still
    /// re-resolves to the standing `Terminal N`). The shell feeds this
    /// on its tick.
    pub(crate) fn sync_title(
        &mut self,
        id: TabId,
        report: Option<String>,
        cwd: Option<PathBuf>,
        mode: TabTitleMode,
    ) -> bool {
        // An empty report is a title clear — core's own storage rule.
        let shell_title = report.and_then(shell_title_from_report);
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        if tab.shell_title == shell_title && tab.cwd == cwd {
            return false;
        }
        tab.shell_title = shell_title;
        tab.cwd = cwd;
        tab.title = resolve_record_title(tab, mode);
        true
    }

    /// Re-resolves every tab's display title under `mode` — a live
    /// mode flip re-titles the whole row immediately, from the stored
    /// inputs.
    pub(crate) fn refresh_titles(&mut self, mode: TabTitleMode) {
        for tab in &mut self.tabs {
            tab.title = resolve_record_title(tab, mode);
        }
    }

    /// The bar's view model: one row per tab, active flagged, labels
    /// composed through core's `title_prefix`.
    pub(crate) fn rows(&self) -> Vec<RowVm> {
        let active = self.active;
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| RowVm {
                id: tab.id,
                label: format!("{}{}", title_prefix(tab.pinned, tab.attention), tab.title),
                is_active: index == active,
                is_pinned: tab.pinned,
                attention: tab.attention,
            })
            .collect()
    }

    /// The tab records in strip order (the shell maps them to their
    /// panes).
    pub(crate) fn records(&self) -> &[TabRecord] {
        &self.tabs
    }

    /// Closes the tab with `id` through core's close decision — the
    /// successor activation and the never-empty respawn signal —
    /// returning `None` for an unknown id. The successor's attention
    /// marker retires through core's activation transition (the close
    /// implies a switch, exactly like a click would). Task 1's use is
    /// the spawn-failure rollback; the close flow layers the confirm
    /// gate and pane teardown around the same core call.
    pub(crate) fn close(&mut self, id: TabId) -> Option<CloseDecision> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let decision = close_tab(&mut self.tabs, index, self.active)?;
        self.active = decision.next_active;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.attention = tab.attention.on_activated();
        }
        Some(decision)
    }

    /// Commits an inline rename of the tab with `id` through core —
    /// a non-empty draft becomes the user rename (shown at once, safe
    /// from every later shell report), an empty one is the explicit
    /// clear that returns the live mode's title — returning whether
    /// the tab exists.
    pub(crate) fn commit_rename(&mut self, id: TabId, new_title: &str, mode: TabTitleMode) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        let mut title = std::mem::take(&mut tab.title);
        commit_rename(
            mode,
            tab.shell_title.as_deref(),
            tab.cwd.as_deref(),
            None,
            tab.id,
            &mut title,
            &mut tab.title_override,
            new_title,
        );
        tab.title = title;
        true
    }

    /// Applies a completed tab drag: the tab with `id` moves to `to`
    /// through core's reorder, whose identity rule keeps whichever tab
    /// was active active. A pinned tab refuses the move (it stays
    /// put, the reference bar's ruling 4), and a no-op drop — unknown
    /// id, the tab's own slot, an out-of-range target — reports no
    /// change.
    pub(crate) fn move_to(&mut self, id: TabId, to: usize) -> bool {
        let Some(from) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if self.tabs[from].pinned || from == to || to >= self.tabs.len() {
            return false;
        }
        self.active = reorder_tabs(&mut self.tabs, self.active, from, to);
        true
    }

    /// Nudges the tab with `id` one slot toward `direction` — the
    /// per-tab menu's Move rows — clamping at the edge through core's
    /// own `move_target`, with the same pinned refusal and active
    /// identity rule as a drag.
    pub(crate) fn move_by(&mut self, id: TabId, direction: MoveDirection) -> bool {
        let Some(from) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if self.tabs[from].pinned {
            return false;
        }
        let Some(to) = move_target(from, self.tabs.len(), direction) else {
            return false;
        };
        self.active = reorder_tabs(&mut self.tabs, self.active, from, to);
        true
    }

    /// Flips the pin of the tab with `id` — the per-tab menu's
    /// Pin/Unpin row, the one surface that arms a pin — returning
    /// whether the tab exists.
    pub(crate) fn toggle_pin(&mut self, id: TabId) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        tab.pinned = !tab.pinned;
        true
    }

    /// Spawns a duplicate of the tab with `source` — a fresh record
    /// (never the running terminal) in the source's cwd carrying its
    /// rename pin, landing after the *active* tab and activated, the
    /// reference `duplicate-tab` semantics. A missing source spawns
    /// the plain default record (the keybind's empty-list behavior).
    pub(crate) fn spawn_duplicate(&mut self, source: TabId, mode: TabTitleMode) -> TabId {
        let (cwd, title_override) = self
            .tabs
            .iter()
            .find(|tab| tab.id == source)
            .map_or((None, None), |tab| {
                (tab.cwd.clone(), tab.title_override.clone())
            });
        let id = TabId(self.next_id);
        self.next_id += 1;
        let mut tab = TabRecord {
            id,
            title: next_default_title(id),
            shell_title: None,
            cwd,
            last_running_comm: None,
            pasted_images: Vec::new(),
            bell_notified: false,
            title_override,
            pinned: false,
            attention: Attention::None,
            profile: None,
            ephemeral: false,
        };
        tab.title = resolve_record_title(&tab, mode);
        self.active = insert_new_tab(&mut self.tabs, self.active, true, tab);
        id
    }

    /// Resolves a close sweep's targets through core — pinned
    /// neighbors skipped — as ids in descending strip order, so the
    /// shell's closes never shift a later target's index. An unknown
    /// anchor sweeps nothing.
    pub(crate) fn close_sweep(&self, id: TabId, sweep: CloseSweep) -> Vec<TabId> {
        let Some(anchor) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Vec::new();
        };
        sweep_targets(&self.tabs, anchor, sweep, |tab| tab.pinned)
            .into_iter()
            .rev()
            .map(|index| self.tabs[index].id)
            .collect()
    }

    /// Lands the pane's polled attention signals on the tab with `id`
    /// through core's transitions — but only while the tab is
    /// inactive: the active tab is the one the user is already
    /// looking at and never accrues a marker.
    pub(crate) fn note_attention(&mut self, id: TabId, bell: bool, output: bool) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if index == self.active {
            return;
        }
        let tab = &mut self.tabs[index];
        if output {
            tab.attention = tab.attention.on_activity();
        }
        if bell {
            tab.attention = tab.attention.on_bell();
        }
    }

    /// Spawns one restored tab (gpui-parity Task 3, the session
    /// restore's record; py-5 added the profile seat): appended at
    /// the list's end in the plan's order, carrying the saved chrome
    /// verbatim — the resolved title, the rename pin, the saved pin —
    /// and the profile id the plan re-resolved (a saved id the config
    /// no longer carries restores plain, the plan's own rule).
    pub(crate) fn spawn_restored(
        &mut self,
        title: String,
        title_override: Option<String>,
        pinned: bool,
        cwd: Option<PathBuf>,
        profile: Option<String>,
    ) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        self.tabs.push(TabRecord {
            id,
            title,
            shell_title: None,
            cwd,
            title_override,
            pinned,
            attention: Attention::None,
            last_running_comm: None,
            pasted_images: Vec::new(),
            bell_notified: false,
            profile,
            ephemeral: false,
        });
        id
    }

    /// Activates the index (the restore plan's clamped active seat):
    /// out of range changes nothing.
    pub(crate) fn activate_index(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    /// The active tab's index — the session capture's `active` input.
    pub(crate) fn active_index(&self) -> usize {
        self.active
    }

    /// Whether the tab with `id` holds its bell-notification latch —
    /// `false` for an unknown id (never a notification for a ghost).
    pub(crate) fn bell_latched(&self, id: TabId) -> bool {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.bell_notified)
    }

    /// Arms the bell-notification latch on the tab with `id` (one
    /// notification per tab per hide period); an unknown id arms
    /// nothing.
    pub(crate) fn note_bell_notified(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.bell_notified = true;
        }
    }

    /// Clears every tab's bell-notification latch — the show
    /// transition's arm (a re-hidden surface notifies again).
    pub(crate) fn clear_bell_latches(&mut self) {
        for tab in &mut self.tabs {
            tab.bell_notified = false;
        }
    }

    /// Records one image paste on the tab with `id` (the
    /// [`PaneSignal::ImagePasted` funnel): core's `PastedImage` row —
    /// the stamp from the pasted filename, the dimensions from a
    /// header-only read of the bytes. An unknown id records nothing.
    pub(crate) fn note_pasted_image(&mut self, id: TabId, path: PathBuf, bytes: &[u8]) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs[index]
            .pasted_images
            .push(dropterm::core::tabs::PastedImage::new(path, bytes));
    }

    /// The tab with `id`'s pasted-image ledger (the panel's rows and
    /// the band chip's count), `None` for an unknown id.
    pub(crate) fn pasted_images(&self, id: TabId) -> Option<&[dropterm::core::tabs::PastedImage]> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.pasted_images.as_slice())
    }

    /// Drops the pasted-image row owning `path` from the tab with
    /// `id` (the panel's Delete verb's model half, the reference
    /// `forget_pasted_image`'s record side): the row and nothing else
    /// — the cache file is the caller's best-effort concern. Returns
    /// whether a row dropped; an unknown id or path owns nothing.
    pub(crate) fn forget_pasted_image(&mut self, id: TabId, path: &std::path::Path) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        let before = tab.pasted_images.len();
        tab.pasted_images.retain(|row| row.path != path);
        before != tab.pasted_images.len()
    }

    /// The records mutably — the tests' state seeding (pinned flags,
    /// attention markers); every runtime write goes through a named
    /// core-routed method above.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the tests seed tab state through it; runtime writes are named methods"
        )
    )]
    pub(crate) fn tabs_mut(&mut self) -> &mut Vec<TabRecord> {
        &mut self.tabs
    }
}

// -- the per-tab menu (Task 2's interactions) -------------------------

/// One pick the per-tab context menu can publish, carried with the
/// right-pressed tab's id. Every variant routes to a core call
/// through the shell (the reference bar's `TabMenuPick` whole, py-5
/// landing the Switch Profile section its spawn path was waiting
/// for).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabMenuPick {
    /// The default spawn — the `+`'s funnel.
    NewTab,
    /// Inline rename of the clicked tab (no activation side effect).
    Rename,
    /// Duplicate the clicked tab — cwd and rename pin read from it,
    /// after-active landing, the duplicate activated.
    Duplicate,
    /// Flip the clicked tab's pin.
    Pin,
    /// The Switch Profile section's header: muted, non-clickable — a
    /// press only dismisses, like every muted row.
    SwitchProfileHeader,
    /// Respawn the clicked tab in place under the profile at the
    /// config index (slot, activation, and rename pin kept).
    SwitchProfile(usize),
    /// Nudge the clicked tab one slot toward the direction, clamping
    /// at the edge.
    Move(MoveDirection),
    /// Close the clicked tab through the gated front door.
    Close,
    /// Close every tab but the clicked one, through the same gate.
    CloseOthers,
    /// Close every tab right of the clicked one, through the same
    /// gate.
    CloseRight,
}

/// One rendered row of the per-tab context menu: its label, the pick
/// a press publishes, whether it renders active (edge-clamped moves
/// and refusals render muted — the GNOME disabled-row rule), and
/// whether a separator line renders above it. Plain data, composed
/// headless.
pub(crate) struct TabMenuRow {
    pub(crate) label: String,
    pub(crate) pick: TabMenuPick,
    pub(crate) enabled: bool,
    pub(crate) separated: bool,
}

/// The per-tab context menu's rows for the tab at `tab_index` of
/// `tab_count`, `pinned` naming that tab's pin state and `profiles`
/// the config's list — the reference bar's composition whole (py-5
/// lands the Switch Profile section): New Tab leads, the clicked-tab
/// cluster follows with Pin/Unpin labeled by the flip in store, the
/// Switch Profile section renders exactly while ≥1 profile exists
/// (the muted header plus one row per profile in config order), moves
/// mute at the edges they clamp at and — a pinned tab staying put —
/// for a pinned target, whose own Close Tab row mutes with them (the
/// close front door's refusal, rendered), and Close Tabs to the Right
/// is omitted at the rightmost tab (the row would always no-op
/// there).
pub(crate) fn tab_menu_rows(
    profile_names: &[String],
    tab_count: usize,
    tab_index: usize,
    pinned: bool,
) -> Vec<TabMenuRow> {
    let row = |label: &str, pick: TabMenuPick, enabled: bool, separated: bool| TabMenuRow {
        label: label.to_string(),
        pick,
        enabled,
        separated,
    };
    let mut rows = vec![
        row("New Tab", TabMenuPick::NewTab, true, false),
        row("Rename Tab", TabMenuPick::Rename, true, true),
        row("Duplicate Tab", TabMenuPick::Duplicate, true, false),
        row(
            if pinned { "Unpin Tab" } else { "Pin Tab" },
            TabMenuPick::Pin,
            true,
            false,
        ),
    ];
    // The Switch Profile section: the reference composition's own
    // shape — the muted, separated header plus one enabled row per
    // profile, surfacing profiles exactly once.
    if !profile_names.is_empty() {
        rows.push(row(
            "Switch Profile",
            TabMenuPick::SwitchProfileHeader,
            false,
            true,
        ));
        rows.extend(
            profile_names
                .iter()
                .enumerate()
                .map(|(index, name)| row(name, TabMenuPick::SwitchProfile(index), true, false)),
        );
    }
    rows.push(row(
        "Move Tab Left",
        TabMenuPick::Move(MoveDirection::Left),
        !pinned && tab_index > 0,
        true,
    ));
    rows.push(row(
        "Move Tab Right",
        TabMenuPick::Move(MoveDirection::Right),
        !pinned && tab_index + 1 < tab_count,
        false,
    ));
    rows.push(row("Close Tab", TabMenuPick::Close, !pinned, true));
    rows.push(row(
        "Close Other Tabs",
        TabMenuPick::CloseOthers,
        true,
        false,
    ));
    // The rightmost tab's Close-Right row would always no-op: hidden,
    // not muted (the reference bar's Firefox hide rule).
    if tab_index + 1 < tab_count {
        rows.push(row(
            "Close Tabs to the Right",
            TabMenuPick::CloseRight,
            true,
            false,
        ));
    }
    rows
}

/// One row of the `+` affordance's right-press dropdown: the launch a
/// pick publishes — `None` is the plain default spawn (the `+`'s own
/// left-press funnel), `Some(index)` launches the profile at the
/// config index. Plain data, composed headless (the reference
/// `DropdownRow`).
pub(crate) struct DropdownRow {
    pub(crate) label: String,
    pub(crate) profile: Option<usize>,
}

/// The `+` dropdown's rows (py-5): row 0 is the plain `New Tab` (the
/// default spawn — the `+`'s left-press semantics), then one row per
/// profile in config order — the context-menu submenu's composition.
/// An empty profile list yields no rows: zero profiles, no dropdown
/// (the flat-menu ruling; a right press on the `+` no-ops).
pub(crate) fn dropdown_rows(profile_names: &[String]) -> Vec<DropdownRow> {
    if profile_names.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![DropdownRow {
        label: "New Tab".to_string(),
        profile: None,
    }];
    rows.extend(
        profile_names
            .iter()
            .enumerate()
            .map(|(index, name)| DropdownRow {
                label: name.clone(),
                profile: Some(index),
            }),
    );
    rows
}

/// The close gate's decision (the reference bar's criterion 3): the
/// in-surface confirmation waits only when the toggle is on AND a
/// foreground child is running — either input false closes
/// immediately, today's behavior. Pure so the truth table stays
/// headless-testable.
pub(crate) fn confirm_needed(enabled: bool, foreground_running: bool) -> bool {
    enabled && foreground_running
}

// -- the strip element ------------------------------------------------
//
// The presentation constants port the reference bar's `METRICS` (the
// libcosmic bar keeps them constants rather than theme values so its
// pure layout stays deterministic; the same numbers keep the two
// frontends' strips the same size).

/// Horizontal padding inside a tab button, on each side.
const BUTTON_PADDING_X: f32 = 10.0;
/// Gap between consecutive tab buttons, and around the corner
/// affordances.
const BUTTON_GAP: f32 = 4.0;
/// Side length of the square close/`+`/gear glyph targets.
const GLYPH_SIZE: f32 = 24.0;
/// Title text size in logical pixels.
const TITLE_FONT_SIZE: f32 = 14.0;
/// Narrowest a tab button may get under squeeze or with an empty
/// title; widest no matter how long the title (longer titles
/// ellipsize at the button edge).
const MIN_BUTTON_WIDTH: f32 = 48.0;
const MAX_BUTTON_WIDTH: f32 = 200.0;
/// The border hairline's alpha — the strip foreground at low
/// strength, hierarchy by alpha, never a new hue (the reference bar's
/// `BORDER_ALPHA`).
pub(crate) const BORDER_ALPHA: f32 = 0.3;

/// The strip's select callback: one tab id into the shell.
type SelectFn = std::rc::Rc<dyn Fn(TabId, &mut gpui::Window, &mut gpui::App)>;
/// The strip's close callback: one tab id into the shell's gated
/// front door (the `×` glyph's press and the row's middle-click).
type CloseFn = std::rc::Rc<dyn Fn(TabId, &mut gpui::Window, &mut gpui::App)>;
/// The strip's corner-affordance callbacks (`+`, gear).
type CornerFn = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App)>;
/// The strip's rename trigger: one tab id (a double-click on the row).
type RenameFn = std::rc::Rc<dyn Fn(TabId, &mut gpui::Window, &mut gpui::App)>;
/// The strip's completed-drag callback: the dragged tab id and the
/// drop target's row index.
type MoveFn = std::rc::Rc<dyn Fn(TabId, usize, &mut gpui::Window, &mut gpui::App)>;
/// The strip's per-tab menu pick: the right-pressed tab id and the
/// picked row.
type MenuPickFn = std::rc::Rc<dyn Fn(TabId, TabMenuPick, &mut gpui::Window, &mut gpui::App)>;
/// The strip's rename cancellation (the focused input's Escape).
type CancelRenameFn = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App)>;
/// The strip's `+`-dropdown pick: the row's profile slot — `None` is
/// the plain default spawn (the `+`'s own funnel), `Some(index)`
/// launches the profile at the config index.
type PlusMenuFn = std::rc::Rc<dyn Fn(Option<usize>, &mut gpui::Window, &mut gpui::App)>;

/// The tab strip element (Task 1's render half): one bar-height row —
/// tab buttons while two or more tabs exist, the `+` and gear
/// affordances at the right corner, the border hairline on the
/// terminal-facing edge. Built per frame by the shell from the tab
/// list's view model; every behavior is a callback into the shell.
#[derive(IntoElement)]
pub(crate) struct TabBarView {
    rows: Vec<RowVm>,
    strip: gpui::Hsla,
    fg: gpui::Hsla,
    show_border: bool,
    at_bottom: bool,
    on_select: SelectFn,
    on_close: CloseFn,
    on_rename: RenameFn,
    on_move: MoveFn,
    on_menu_pick: MenuPickFn,
    on_cancel_rename: CancelRenameFn,
    on_new_tab: CornerFn,
    on_settings: CornerFn,
    /// The `+` dropdown's pick (py-5), routed like every corner
    /// affordance.
    on_plus_menu: PlusMenuFn,
    /// The config's profile names in order, the menus' composition
    /// input (the per-tab Switch Profile section and the `+`
    /// dropdown's rows — empty keeps both absent, the flat shape).
    profile_names: Vec<String>,
    /// The open inline rename, if any: the renamed tab and the Kit
    /// input state it edits through (hosted in the row's label slot).
    rename: Option<(TabId, gpui::Entity<gpui_component::input::InputState>)>,
}

impl TabBarView {
    pub(crate) fn new(rows: Vec<RowVm>, strip: gpui::Hsla, fg: gpui::Hsla) -> Self {
        Self {
            rows,
            strip,
            fg,
            show_border: true,
            at_bottom: false,
            on_select: std::rc::Rc::new(|_, _, _| {}),
            on_close: std::rc::Rc::new(|_, _, _| {}),
            on_rename: std::rc::Rc::new(|_, _, _| {}),
            on_move: std::rc::Rc::new(|_, _, _, _| {}),
            on_menu_pick: std::rc::Rc::new(|_, _, _, _| {}),
            on_cancel_rename: std::rc::Rc::new(|_, _| {}),
            on_new_tab: std::rc::Rc::new(|_, _| {}),
            on_settings: std::rc::Rc::new(|_, _| {}),
            on_plus_menu: std::rc::Rc::new(|_, _, _| {}),
            profile_names: Vec::new(),
            rename: None,
        }
    }

    /// The `+` dropdown's pick (py-5): `None` runs the plain default
    /// spawn (the `+`'s own funnel), `Some(index)` launches the
    /// profile at the config index.
    pub(crate) fn on_plus_menu(
        mut self,
        on_plus_menu: impl Fn(Option<usize>, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_plus_menu = std::rc::Rc::new(on_plus_menu);
        self
    }

    /// The config's profile names in order — the Switch Profile
    /// section's and the `+` dropdown's composition input; empty keeps
    /// both menus in their flat shape.
    pub(crate) fn profile_names(mut self, names: Vec<String>) -> Self {
        self.profile_names = names;
        self
    }

    /// Whether the border hairline renders (`tab_bar_border`).
    pub(crate) fn show_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    /// Whether the strip sits at the surface's bottom (the hairline
    /// faces the terminal: the strip's top when at the bottom, its
    /// bottom when at the top).
    pub(crate) fn at_bottom(mut self, at_bottom: bool) -> Self {
        self.at_bottom = at_bottom;
        self
    }

    pub(crate) fn on_select(
        mut self,
        on_select: impl Fn(TabId, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_select = std::rc::Rc::new(on_select);
        self
    }

    /// The strip's close callback — the gated front door's address
    /// (the `×` glyph and the row's middle-click ride it).
    pub(crate) fn on_close(
        mut self,
        on_close: impl Fn(TabId, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_close = std::rc::Rc::new(on_close);
        self
    }

    /// The strip's rename trigger — a double-click on a row's body.
    pub(crate) fn on_rename(
        mut self,
        on_rename: impl Fn(TabId, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_rename = std::rc::Rc::new(on_rename);
        self
    }

    /// The strip's completed-drag callback (reorder): the dragged tab
    /// id and the row index it was dropped on.
    pub(crate) fn on_move(
        mut self,
        on_move: impl Fn(TabId, usize, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_move = std::rc::Rc::new(on_move);
        self
    }

    /// The strip's per-tab menu pick callback (the context menu's
    /// rows ride it).
    pub(crate) fn on_menu_pick(
        mut self,
        on_menu_pick: impl Fn(TabId, TabMenuPick, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_menu_pick = std::rc::Rc::new(on_menu_pick);
        self
    }

    /// The strip's rename cancellation (the focused input's Escape —
    /// the Kit's input propagates an Escape it does not consume).
    pub(crate) fn on_cancel_rename(
        mut self,
        on_cancel_rename: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_cancel_rename = std::rc::Rc::new(on_cancel_rename);
        self
    }

    /// The open inline rename: the Kit input state takes the renamed
    /// row's label slot while the rename runs.
    pub(crate) fn renaming(
        mut self,
        tab: TabId,
        state: gpui::Entity<gpui_component::input::InputState>,
    ) -> Self {
        self.rename = Some((tab, state));
        self
    }

    pub(crate) fn on_new_tab(
        mut self,
        on_new_tab: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_new_tab = std::rc::Rc::new(on_new_tab);
        self
    }

    pub(crate) fn on_settings(
        mut self,
        on_settings: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_settings = std::rc::Rc::new(on_settings);
        self
    }

    /// One tab button: pill background and brighter text when active,
    /// the composed label (or the Kit input, while its inline rename
    /// runs), and the close glyph — whose press rides the shell's
    /// gated front door, never selecting the row beneath it; a
    /// middle-click on the row closes the same way, a double-click on
    /// the body starts the rename, a drag lands on the row it is
    /// dropped on (pinned rows never arm one — they stay put), and a
    /// right-press opens the per-tab menu through the Kit's popover
    /// path.
    #[expect(
        clippy::too_many_arguments,
        reason = "the row's whole gesture surface beside its view model: one element, one wiring block, uniform per row"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "one row, one wiring block — the pill, the close glyph, the press/double-press/middle press trio, the drag/drop pair, and the menu"
    )]
    fn row_element(
        row: &RowVm,
        index: usize,
        count: usize,
        fg: gpui::Hsla,
        profile_names: &[String],
        on_select: SelectFn,
        on_close: CloseFn,
        on_rename: RenameFn,
        on_move: MoveFn,
        on_menu_pick: &MenuPickFn,
        rename: Option<&gpui::Entity<gpui_component::input::InputState>>,
    ) -> impl gpui::IntoElement {
        let active_pill = gpui::Hsla { a: 0.18, ..fg };
        let text = if row.is_active {
            fg
        } else {
            gpui::Hsla { a: 0.65, ..fg }
        };
        let id = row.id;
        let on_close_middle = std::rc::Rc::clone(&on_close);
        let row_label = row.label.clone();
        // The pill: the plain row keeps its composed label while the
        // renamed row hosts the Kit input in the same slot.
        let label_slot = match rename {
            Some(state) => gpui::div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(gpui_component::input::Input::new(state).appearance(false))
                .into_any_element(),
            None => gpui::div()
                .flex_shrink(1.)
                .overflow_hidden()
                .child(row.label.clone())
                .into_any_element(),
        };
        // The per-tab menu's rows, composed at open time from the
        // clicked row's own state (the muted rows a pick would refuse
        // render disabled — the GNOME rule).
        let menu_tab = id;
        let menu_index = index;
        let menu_count = count;
        let menu_pinned = row.is_pinned;
        let menu_profiles = profile_names.to_vec();
        let menu_pick = std::rc::Rc::clone(on_menu_pick);
        gpui::div()
            .id(gpui::ElementId::Name(format!("tab-{}", id.0).into()))
            .h_full()
            // The renamed row claims the full max width: its slot's
            // Kit input has no intrinsic width of its own (size_full
            // against a content-sized parent collapses), so a
            // content-sized row starves the editor into a clipped
            // sliver — the blank-input report's whole story. A
            // definite row width is what gives the flex_1 slot a
            // real span (the strip's caret walk pins this).
            .min_w(gpui::px(if rename.is_some() {
                MAX_BUTTON_WIDTH
            } else {
                MIN_BUTTON_WIDTH
            }))
            .max_w(gpui::px(MAX_BUTTON_WIDTH))
            .px(gpui::px(BUTTON_PADDING_X))
            .rounded_sm()
            .flex()
            .items_center()
            .gap(gpui::px(6.0))
            .when(row.is_active, |button| button.bg(active_pill))
            .text_color(text)
            .overflow_hidden()
            .child(label_slot)
            .child(
                gpui::div()
                    .size(gpui::px(GLYPH_SIZE))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .text_color(gpui::Hsla { a: 0.7, ..fg })
                    .child("×")
                    // The glyph's press closes its tab through the
                    // gated front door, and never also selects it
                    // (the row's own handler sits beneath).
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        move |_, window: &mut gpui::Window, cx: &mut gpui::App| {
                            cx.stop_propagation();
                            on_close(id, window, cx);
                        },
                    ),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |event: &gpui::MouseDownEvent,
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    // A double-click on the body starts the inline
                    // rename (the first press already selected); a
                    // single press selects.
                    if event.click_count >= 2 {
                        on_rename(id, window, cx);
                    } else {
                        on_select(id, window, cx);
                    }
                },
            )
            // A middle-click on the row closes whichever part of the
            // button it lands on (the reference bar's rule).
            .on_mouse_down(
                gpui::MouseButton::Middle,
                move |_, window: &mut gpui::Window, cx: &mut gpui::App| {
                    on_close_middle(id, window, cx);
                },
            )
            // The drag: unpinned rows arm one whose floating chip
            // follows the cursor; every row is a drop target, so the
            // drop lands on the row it is released over (a drop on
            // the dragged row's own slot is a no-op at the model).
            .when(!row.is_pinned, |button| {
                button
                    .on_drag(
                        TabDrag(id),
                        move |_drag: &TabDrag,
                              _offset: gpui::Point<gpui::Pixels>,
                              _window: &mut gpui::Window,
                              cx: &mut gpui::App| {
                            cx.new(|_| TabDragChip {
                                label: row_label.clone(),
                                fg,
                            })
                        },
                    )
                    .on_drop(
                        move |dragged: &TabDrag, window: &mut gpui::Window, cx: &mut gpui::App| {
                            on_move(dragged.0, index, window, cx);
                        },
                    )
            })
            // The per-tab menu, on the Kit's popover path: rows
            // composed at open time from the clicked row's own state
            // (the muted rows a pick would refuse render disabled —
            // the GNOME rule).
            .context_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for entry in tab_menu_rows(&menu_profiles, menu_count, menu_index, menu_pinned) {
                    if entry.separated {
                        menu = menu.separator();
                    }
                    let pick = entry.pick;
                    let menu_pick = std::rc::Rc::clone(&menu_pick);
                    menu = menu.item(
                        PopupMenuItem::new(entry.label)
                            .disabled(!entry.enabled)
                            .on_click(move |_event, window, cx| {
                                menu_pick(menu_tab, pick, window, cx);
                            }),
                    );
                }
                menu
            })
    }

    /// One corner affordance — the `+` or the gear — a square glyph
    /// target at the strip's right corner.
    fn glyph_button(glyph: &'static str, fg: gpui::Hsla, on_press: CornerFn) -> gpui::Div {
        gpui::div()
            .size(gpui::px(GLYPH_SIZE))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .text_color(fg)
            .child(glyph)
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |_, window: &mut gpui::Window, cx: &mut gpui::App| on_press(window, cx),
            )
    }
}

impl gpui::RenderOnce for TabBarView {
    fn render(self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> impl gpui::IntoElement {
        let fg = self.fg;
        let count = self.rows.len();
        let on_cancel_rename = std::rc::Rc::clone(&self.on_cancel_rename);
        let mut bar = gpui::div()
            .w_full()
            .h(gpui::px(BAR_HEIGHT))
            .flex()
            .flex_none()
            .flex_row()
            .items_center()
            .gap(gpui::px(BUTTON_GAP))
            .px(gpui::px(BUTTON_PADDING_X))
            .bg(self.strip)
            .text_size(gpui::px(TITLE_FONT_SIZE))
            .overflow_hidden();
        if self.show_border {
            // The hairline faces the terminal: the strip's top edge
            // when it sits at the bottom, its bottom edge otherwise.
            let hairline = gpui::Hsla {
                a: BORDER_ALPHA,
                ..fg
            };
            bar = if self.at_bottom {
                bar.border_t(gpui::px(1.)).border_color(hairline)
            } else {
                bar.border_b(gpui::px(1.)).border_color(hairline)
            };
        }
        if shows_tab_buttons(count) {
            let on_select = std::rc::Rc::clone(&self.on_select);
            let on_close = std::rc::Rc::clone(&self.on_close);
            let on_rename = std::rc::Rc::clone(&self.on_rename);
            let on_move = std::rc::Rc::clone(&self.on_move);
            bar = bar.children(self.rows.iter().enumerate().map(|(index, row)| {
                Self::row_element(
                    row,
                    index,
                    count,
                    fg,
                    &self.profile_names,
                    std::rc::Rc::clone(&on_select),
                    std::rc::Rc::clone(&on_close),
                    std::rc::Rc::clone(&on_rename),
                    std::rc::Rc::clone(&on_move),
                    &self.on_menu_pick,
                    self.rename
                        .as_ref()
                        .filter(|(tab, _)| *tab == row.id)
                        .map(|(_, state)| state),
                )
            }));
        }
        // The rename's Escape: the focused input propagates an Escape
        // it does not consume, and this context — on the input's own
        // ancestor — catches it (the context only ever matches while
        // a rename's input holds the keyboard inside the strip).
        bar.key_context("tab-rename")
            .on_action(
                move |_: &CancelTabRename, window: &mut gpui::Window, cx: &mut gpui::App| {
                    on_cancel_rename(window, cx);
                },
            )
            .child(gpui::div().flex_1())
            // The `+`'s right press opens the profile dropdown (py-5,
            // the reference `+` dropdown): New Tab first, then one row
            // per profile in config order — the context-menu submenu's
            // composition, on the Kit's popover path. Zero profiles
            // keep the flat shape: no context menu arms, a right press
            // no-ops.
            .when(!self.profile_names.is_empty(), |bar| {
                let dropdown_profiles = self.profile_names.clone();
                let on_plus_menu = std::rc::Rc::clone(&self.on_plus_menu);
                let on_new_tab = std::rc::Rc::clone(&self.on_new_tab);
                bar.child(Self::glyph_button("+", fg, on_new_tab).context_menu(
                    move |menu, _window, _cx| {
                        let mut menu = menu;
                        for entry in dropdown_rows(&dropdown_profiles) {
                            let pick_profile = entry.profile;
                            let on_plus_menu = std::rc::Rc::clone(&on_plus_menu);
                            menu = menu.item(PopupMenuItem::new(entry.label).on_click(
                                move |_event, window, cx| {
                                    on_plus_menu(pick_profile, window, cx);
                                },
                            ));
                        }
                        menu
                    },
                ))
            })
            .when(self.profile_names.is_empty(), |bar| {
                bar.child(Self::glyph_button("+", fg, self.on_new_tab))
            })
            .child(Self::glyph_button("⚙", fg, self.on_settings))
    }
}

#[cfg(test)]
mod tests;
