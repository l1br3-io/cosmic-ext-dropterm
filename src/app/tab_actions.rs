// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;
use std::sync::Mutex;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::{self as aterm};
use cosmic::app::Task;
use cosmic::iced::clipboard;
use cosmic::widget;
use cosmic::widget::menu::{self, action::MenuAction};

use super::DropTerm;
use super::Message;
use super::SurfaceState;
use super::ToggleAction;
use super::confirm_needed;
use super::spawn::{
    SpawnCommand, SpawnedTerminal, default_spawn_command, live_preset_cwd, plain_spawn_cwd,
    profile_pick,
};
use crate::context_menu::items;
use crate::keybinds;
use crate::notify;
use crate::profile::{self, Profile};
use crate::session;
use crate::settings;
use crate::tabs::{self, CloseSweep, TabId};
use crate::terminal::Action;

/// An in-flight inline tab rename: the tab being renamed and the
/// current draft. The input itself is an overlay the tab bar hosts
/// over the renamed tab's button.
pub(super) struct RenameEdit {
    pub(super) tab: TabId,
    pub(super) draft: String,
}

/// Turns a menu pick into the app message that carries it: the same
/// entry the keybind path uses, so menu behavior cannot fork from
/// keybind behavior.
impl MenuAction for Action {
    type Message = Message;

    fn message(&self) -> Self::Message {
        Message::Action(*self)
    }
}

/// The menu as libcosmic trees, one per row of [`rows(profiles)`],
/// each item rendered with its keybind as a hint (every static action
/// is bound in `default_key_binds`; profile rows carry none — the
/// lookup tolerates unbound actions with an empty hint). Called by
/// the view only while the menu is open.
pub(super) fn trees(profiles: &[Profile]) -> Vec<menu::Tree<Message>> {
    menu::items(&keybinds::default_key_binds(), items(profiles))
}

/// Whether a tab button exists to host the inline-rename input: the
/// bar hides its buttons below two tabs (its own visibility rule, the
/// same one `prune_rename_session` enforces), and the input overlays a
/// button — so a rename can only start, or keep running, with one to
/// sit on.
pub(super) fn rename_overlay_hosted(tab_count: usize) -> bool {
    tab_count > 1
}

/// The trees the view hands `widget::context_menu` for the current
/// open state: `None` while closed — the wrapper is then a plain
/// pass-through of the terminal — and the full row trees while open,
/// which arms the wrapper's right-release trigger. Holding the
/// open-state decision here keeps the view wiring free of menu logic.
pub(super) fn open_trees(
    menu_open: bool,
    profiles: &[Profile],
) -> Option<Vec<menu::Tree<Message>>> {
    menu_open.then(|| trees(profiles))
}

impl DropTerm {
    /// Applies an in-place profile switch: the tab identified by `tab`
    /// keeps its slot in the list, its activation state, and its sticky
    /// user rename, while its terminal is replaced by a fresh PTY
    /// running the profile `profile_index` resolves to — the seam the
    /// remote `switch-profile-tab` rides through
    /// [`Message::SwitchTabProfile`]. Unresolvable inputs warn and
    /// no-op, never degrading into a spawn: an unknown tab id, or a
    /// stale profile index (a config edited while a menu snapshot was
    /// open — the `NewProfileTab` precedent). The respawn cwd composes
    /// like a profile launch's — the preset's live cwd
    /// (warn-and-degrade when dead), else the tab's own working
    /// directory read before any teardown — with the tab itself
    /// replacing the "active tab" as the inheritance source. The
    /// replacement takes a fresh [`TabId`]: `Terminal::drop` sends
    /// `Msg::Shutdown` asynchronously, so the dying PTY can still have
    /// events in flight, and those late events then find no tab
    /// carrying the old id (`handle_term_event` drops unknown ids —
    /// the `close_tab` safety). The cosmetic cost — an unrenamed tab's
    /// default title renumbers — is accepted. A respawn is a save
    /// trigger: the next session save records the profile (and the
    /// surviving rename), and a restore re-resolves the tab as that
    /// profile.
    pub(super) fn switch_tab_profile(&mut self, tab: TabId, profile_index: usize) -> Task<Message> {
        let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab) else {
            log::warn!(
                "switch profile: no tab with id {:?} ({} open)",
                tab,
                self.tabs.len()
            );
            return Task::none();
        };
        let Some(command) = profile_pick(&self.config, profile_index) else {
            log::warn!(
                "switch profile: no profile at index {profile_index} ({} configured)",
                self.config.profiles.len()
            );
            return Task::none();
        };
        let SpawnCommand::Profile {
            name, cwd: preset, ..
        } = &command
        else {
            unreachable!("profile_pick returns the Profile arm");
        };
        // The respawn cwd, read before any teardown: the preset's live
        // directory when it has one, else the tab's own — the
        // NewProfileTab composition with the target tab as the
        // inheritance source.
        let respawn_cwd = live_preset_cwd(name, preset.as_ref()).or_else(|| {
            self.tabs[index]
                .terminal
                .lock()
                .unwrap()
                .working_directory()
        });
        // Deferred (no event channel yet) or the spawn errored —
        // both already logged; the tab is untouched, its live
        // terminal still running.
        self.respawn_tab_in_place(index, &command, respawn_cwd, Some(name))
            .unwrap_or_else(Task::none)
    }

    /// The shared in-place respawn tail: the tab at `index` keeps its
    /// slot, activation state, and sticky user rename while its
    /// terminal is replaced by a fresh PTY running `command` in `cwd`
    /// — the whole field dance [`Self::switch_tab_profile`] spells out
    /// for a profile pick and the profile-exit fallback
    /// ([`Self::respawn_shell_after_profile_exit`]) rides for the
    /// plain shell. `profile_name` picks the plan: `Some` records a
    /// persistent profile tab (degraded spawns included — the switch
    /// keeps the pick), `None` a plain shell tab. `None` back means
    /// the spawn never happened (deferred or errored, already
    /// logged); the caller owns the fallback because only it knows
    /// whether the old terminal was still alive.
    fn respawn_tab_in_place(
        &mut self,
        index: usize,
        command: &SpawnCommand,
        cwd: Option<PathBuf>,
        profile_name: Option<&str>,
    ) -> Option<Task<Message>> {
        // An open inline rename on the target is discarded (the
        // rename-tab precedent): the draft must not commit over the
        // respawn, and the explicit end gives the deterministic
        // refocus below.
        let refocus = self.end_rename_on(index);
        let old_override = self.tabs[index].title_override.clone();
        let SpawnedTerminal {
            tab_id,
            terminal,
            shell,
            ..
        } = self.spawn_terminal(command, cwd.as_deref())?;
        let plan = match profile_name {
            Some(name) => tabs::plan_respawn(
                self.config.tab_title_mode,
                tab_id,
                cwd.as_deref(),
                old_override.as_deref(),
                name,
            ),
            None => tabs::plan_shell_respawn(
                self.config.tab_title_mode,
                tab_id,
                cwd.as_deref(),
                old_override.as_deref(),
            ),
        };
        let target = &mut self.tabs[index];
        // Every field the old terminal's life touched comes from the
        // plan — the profile id included for a profile respawn (the
        // tab is a profile tab from now on, whatever it was before; a
        // degraded blank command line still records the pick, and the
        // next restore re-derives the pair through `spawn_shell`), or
        // cleared for the shell fallback (the preset's process is
        // gone, and the tab is a plain shell tab from here on).
        target.title = plan.title;
        target.shell_title = plan.shell_title;
        target.title_override = plan.title_override;
        target.attention = plan.attention;
        target.bell_notified = plan.bell_notified;
        target.ephemeral = plan.ephemeral;
        target.profile = plan.profile;
        target.shell = shell;
        // The id swap and terminal swap land together: assigning the
        // field drops the old `Terminal`, whose `Drop` sends
        // `Msg::Shutdown`, and late events from the dying PTY find no
        // tab carrying the old id.
        let old_id = target.id;
        target.id = tab_id;
        target.terminal = Mutex::new(terminal);
        // The cwd cache follows the id swap: drop the dead tab's
        // entry, seed the new one with the respawn cwd so the fresh
        // shell's first OSC title report reads as unchanged.
        self.tab_cwds.remove(&old_id);
        self.tab_cwds.insert(tab_id, cwd);
        // The spawn-grid record follows the terminal swap the same
        // way — the fresh terminal's settle IS the OQ-2 reset (a
        // respawn starts default), armed here because a respawn only
        // ever runs on a live, laid-out surface: the new terminal's
        // first resize records the resting grid and freezes.
        target.terminal.lock().unwrap().arm_spawn_grid_next_resize();
        // The respawned tab is the focused one: its (possibly new)
        // scheme owns the window background from the first paint of
        // the fresh terminal — the every-assignment rule, the restore
        // precedent in `ensure_tabs` — and its fresh shell owns the
        // status band's process snapshot from the first event.
        self.refresh_active_window_bg();
        self.refresh_status_snapshot();
        Some(Task::batch([refocus, self.mark_session_dirty()]))
    }

    /// The profile-exit fallback: a profile tab's child has exited,
    /// and the tab respawns the configured plain shell in its place
    /// instead of closing — the never-empty rule generalized from
    /// "the last tab" to "every profile tab". A one-shot profile
    /// command (`ls`) must not take its tab with it: the New Tab pick
    /// would flash and vanish ("does not work"), and the Switch
    /// Profile pick would destroy the current slot outright — the old
    /// shell was already killed for the respawn, so closing here
    /// would discard it with no confirmation. The shell is the
    /// configured plain one, never the default spawn: a one-shot
    /// `default_profile` must not loop the fallback forever. The
    /// fresh shell opens in the directory the profile ran in — the
    /// dying child's `/proc` cwd is gone with it, so the seeded cwd
    /// cache is the source. A spawn that cannot land (deferred or
    /// errored) falls back to today's close: a dead-PTY tab left in
    /// the list is the one outcome worse than losing the slot.
    fn respawn_shell_after_profile_exit(&mut self, index: usize) -> Task<Message> {
        let dead_id = self.tabs[index].id;
        let profile = self.tabs[index].profile.clone();
        // The fresh shell opens in the directory the profile ran in.
        // The dying child's own `/proc/<pid>/cwd` is gone with it (and
        // a recycled pid would silently read the wrong process), so
        // the seeded cwd cache is the source — spawn-seeded, refreshed
        // by every title event's sample.
        let cwd = self.tab_cwds.get(&dead_id).cloned().flatten();
        let command = SpawnCommand::Shell(self.config.resolved_shell());
        if let Some(task) = self.respawn_tab_in_place(index, &command, cwd, None) {
            log::info!(
                "profile {} exited; respawned the shell in its tab",
                profile.as_deref().unwrap_or("?")
            );
            task
        } else {
            log::warn!(
                "profile {} exited and the shell respawn failed; closing the tab",
                profile.as_deref().unwrap_or("?")
            );
            self.close_tab_at(index)
        }
    }

    pub(super) fn perform_action(&mut self, action: Action) -> Task<Message> {
        // Hiding must work even before a terminal exists, or a fresh
        // `cargo run` opens a surface with no way to close it.
        if action == Action::Hide {
            return self.hide();
        }
        // The settings drawer toggles from the keybind alone — no
        // active tab required — and never targets the active
        // terminal.
        if action == Action::Settings {
            return self.toggle_settings();
        }
        // The status band's toggle rides the settings write path
        // (status-bar Task 3): the keybind and the Behavior card's
        // toggle are one funnel, and the `ConfigChanged` echo flips
        // the band and re-pins the terminal's height — the same
        // every-apply path, no new geometry code.
        if action == Action::ToggleStatusBar {
            self.write_config(
                settings::WriteKind::Bare,
                &settings::PlannedWrite::StatusBarEnabled(!self.config.status_bar_enabled),
            );
            return Task::none();
        }
        // The tab actions reshape the tab list itself — including keeping
        // it non-empty — so they run before the active-tab borrow below
        // and never reach the per-terminal actions.
        if matches!(
            action,
            Action::NewTab
                | Action::NewTabHome
                | Action::NewProfileTab(_)
                | Action::CloseTab
                | Action::DuplicateTab
                | Action::NextTab
                | Action::PrevTab
                | Action::ActivateTab(_)
                | Action::ActivateLastTab
                | Action::MoveTabLeft
                | Action::MoveTabRight
        ) {
            return self.perform_tab_action(action);
        }
        let Some(tab) = self.active() else {
            return Task::none();
        };
        match action {
            Action::Hide
            | Action::Settings
            | Action::ToggleStatusBar
            | Action::NewTab
            | Action::NewTabHome
            | Action::NewProfileTab(_)
            | Action::CloseTab
            | Action::DuplicateTab
            | Action::NextTab
            | Action::PrevTab
            | Action::ActivateTab(_)
            | Action::ActivateLastTab
            | Action::MoveTabLeft
            | Action::MoveTabRight => unreachable!("handled above"),
            // Rename never reshapes the tab list, so it runs here —
            // after the active-tab resolution — like the per-terminal
            // actions. The id is read before the `&mut self` call:
            // `tab` borrows the list `start_rename` mutates.
            Action::RenameTab => {
                // The input overlays a tab button, and buttons show
                // only with two or more tabs: below that the press is
                // a swallowed no-op (the `ActivateTab` out-of-range
                // precedent).
                if rename_overlay_hosted(self.tabs.len()) {
                    let id = tab.id;
                    return self.start_rename(id);
                }
            }
            // The Switch Profile folder's pick rides the remote
            // verb's seam verbatim: the active tab's id resolved at
            // dispatch, `Message::SwitchTabProfile` carrying it — the
            // same construction the CLI builder uses. The respawn's
            // whole behavior (cwd composition, session-dirty marking,
            // the stale-index warn+no-op) lives inside
            // `switch_tab_profile`; this arm adds routing only.
            Action::SwitchProfileTab(index) => {
                let id = tab.id;
                return cosmic::task::message(cosmic::Action::App(Message::SwitchTabProfile(
                    id, index,
                )));
            }
            Action::Copy => {
                let selected = tab
                    .terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .selection_to_string();
                if let Some(text) = selected {
                    return clipboard::write(text);
                }
            }
            Action::CopyPrimary => {
                let selected = tab
                    .terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .selection_to_string();
                if let Some(text) = selected {
                    return clipboard::write_primary(text);
                }
            }
            Action::Paste => {
                return clipboard::read()
                    .map(|text| cosmic::Action::App(Message::ClipboardPasted(text)));
            }
            Action::PastePrimary => {
                return clipboard::read_primary()
                    .map(|text| cosmic::Action::App(Message::ClipboardPastedPrimary(text)));
            }
            Action::SelectAll => {
                tab.terminal.lock().unwrap().select_all();
            }
            Action::ClearScrollback => {
                tab.terminal
                    .lock()
                    .unwrap()
                    .term
                    .lock()
                    .grid_mut()
                    .clear_history();
            }
            Action::ZoomIn | Action::ZoomOut | Action::ZoomReset => {
                {
                    let mut terminal = tab.terminal.lock().unwrap();
                    let adj = match action {
                        Action::ZoomIn => terminal.zoom_adj().saturating_add(1),
                        Action::ZoomOut => terminal.zoom_adj().saturating_sub(1),
                        _ => 0,
                    };
                    terminal.set_zoom_adj(adj);
                    terminal.update_cell_size();
                }
                // The zoom segment re-renders from the live
                // adjustment; the process snapshot refresh rides the
                // same listed piggyback point (status-bar criterion 5).
                self.refresh_status_snapshot();
            }
        }
        Task::none()
    }

    /// Applies a tab-list keybind action: open, close, or cycle tabs.
    ///
    /// These reshape `tabs` — upholding the never-empty invariant by
    /// respawning after the last tab closes — rather than targeting the
    /// active terminal, so they live apart from the per-terminal actions
    /// in [`Self::perform_action`].
    pub(super) fn perform_tab_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::NewTab => {
                // A new tab inherits the active tab's cwd: the same
                // `/proc/<pid>/cwd` readlink the session save samples,
                // so `Ctrl+Shift+T` after a `cd` lands in the same
                // directory. A failed read — no active tab, non-Linux,
                // an unreadable `/proc` entry — degrades to the pty's
                // default directory.
                let inherited = self
                    .active()
                    .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
                // The plain keybind rides the configured default
                // (ruling 1): with one set and existing, the new tab
                // is a full profile tab — id recorded, session-
                // persisted — in the inherited directory unless the
                // preset's own live cwd applies. No effective default:
                // the shell spawn threads the `new_tab_cwd` mode —
                // `Inherit` exactly today's chain, `Default` replacing
                // the inherited leg with the live-checked
                // `default_cwd`.
                let command = default_spawn_command(&self.config);
                let spawn_cwd = match &command {
                    SpawnCommand::Profile {
                        name, cwd: preset, ..
                    } => live_preset_cwd(name, preset.as_ref()).or(inherited),
                    _ => plain_spawn_cwd(&self.config, None, inherited),
                };
                return self.open_new_tab(&command, spawn_cwd);
            }
            Action::NewTabHome => {
                // Ctrl+Shift+H composes exactly like the plain keybind
                // arm — default profile command, live preset cwd —
                // with home standing in for the inherited directory:
                // an applicable profile cwd setting still wins, and
                // home is what lands without one. An unset HOME —
                // never in a real desktop session — degrades exactly
                // like every other unreadable directory: the pty's
                // default.
                let home = std::env::var_os("HOME").map(PathBuf::from);
                let command = default_spawn_command(&self.config);
                let spawn_cwd = match &command {
                    SpawnCommand::Profile {
                        name, cwd: preset, ..
                    } => live_preset_cwd(name, preset.as_ref()).or(home),
                    _ => home,
                };
                return self.open_new_tab(&command, spawn_cwd);
            }
            Action::NewProfileTab(index) => {
                // A submenu pick: the index came from the menu-open
                // snapshot of `config.profiles`, so a config edited
                // while the menu was open can leave it stale — warn
                // and no-op rather than launch the wrong profile.
                if let Some(command) = profile_pick(&self.config, index) {
                    let inherited = self
                        .active()
                        .and_then(|tab| tab.terminal.lock().unwrap().working_directory());
                    // The preset's cwd composes exactly like the
                    // keybind arm's: live or warn-and-degrade, then
                    // the inherited directory.
                    let spawn_cwd = match &command {
                        SpawnCommand::Profile {
                            name, cwd: preset, ..
                        } => live_preset_cwd(name, preset.as_ref()).or(inherited),
                        // profile_pick only ever builds the Profile arm.
                        _ => unreachable!("profile_pick returns the Profile arm"),
                    };
                    return self.open_new_tab(&command, spawn_cwd);
                }
                log::warn!(
                    "new profile tab: no profile at submenu index {index} \
                     ({} configured)",
                    self.config.profiles.len()
                );
            }
            Action::CloseTab => return self.request_close_tab(self.active_tab),
            Action::DuplicateTab => {
                // Duplicate the active tab — the shared source read
                // (`duplicate_tab_at`), active-tab scoped: unlike
                // rename there is no bar-visibility gate, since
                // duplicating a single tab is meaningful and the fresh
                // bar it implies shows.
                return self.duplicate_tab_at(Some(self.active_tab));
            }
            Action::NextTab => {
                return self.set_active_tab(tabs::cycle_next(&self.tabs, self.active_tab));
            }
            Action::PrevTab => {
                return self.set_active_tab(tabs::cycle_prev(&self.tabs, self.active_tab));
            }
            Action::ActivateTab(requested) => {
                // Strict numbered activation: out of range never
                // wraps; set_active_tab no-ops when the tab is already
                // active.
                if let Some(index) = tabs::activate_index_target(self.tabs.len(), requested) {
                    return self.set_active_tab(index);
                }
            }
            Action::ActivateLastTab => {
                // Ctrl+Shift+9 takes the last tab however many exist
                // (cosmic-term TabActivate8 semantics).
                if let Some(index) = tabs::activate_last_target(self.tabs.len()) {
                    return self.set_active_tab(index);
                }
            }
            Action::MoveTabLeft | Action::MoveTabRight => {
                // One slot toward the direction, clamping at the edge;
                // the moved tab stays active, so no focus handoff.
                // Like every layout change, the move marks the session
                // dirty.
                let direction = match action {
                    Action::MoveTabLeft => tabs::MoveDirection::Left,
                    _ => tabs::MoveDirection::Right,
                };
                if let Some(to) = tabs::move_target(self.active_tab, self.tabs.len(), direction) {
                    self.active_tab = tabs::move_tab(&mut self.tabs, self.active_tab, to);
                    // The moved tab stays active, so the refresh is the
                    // uniform every-assignment rule — idempotent here.
                    self.refresh_active_window_bg();
                    return self.mark_session_dirty();
                }
            }
            _ => unreachable!("not a tab action: {action:?}"),
        }
        Task::none()
    }

    /// The shared tail of every new-tab spawn — keybind, submenu
    /// pick, remote action: spawn, place per `new_tab_after_current`,
    /// activate, and mark the session dirty (the open itself is a
    /// save trigger; when the activation's switch already armed a
    /// window, this second mark coalesces into it).
    pub(super) fn open_new_tab(
        &mut self,
        command: &SpawnCommand,
        cwd: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(tab) = self.spawn_shell(command, cwd) {
            // Placement: after the active tab or at the end, per
            // `new_tab_after_current`; the new tab becomes active
            // either way.
            let at = tabs::insert_new_tab(
                &mut self.tabs,
                self.active_tab,
                self.config.new_tab_after_current,
                tab,
            );
            // The spawn title: `Tab::new` parks at `Terminal N`, but
            // `spawn_shell` already seeded the cwd cache with the
            // spawn directory — resolving through the live mode here
            // means the cwd modes never show the default number
            // while the directory is known (the `plan_respawn`
            // precedent; Shell and Numbered land right back on it).
            self.retitle_tab(at);
            let switch = self.set_active_tab(at);
            Task::batch([switch, self.mark_session_dirty()])
        } else {
            Task::none()
        }
    }

    /// The one canonical duplicate-tab body — the keybind arm, the
    /// remote `duplicate-tab` verb, and (through the action) the
    /// context-menu row all ride here, so the surfaces cannot fork.
    /// A duplicate is a fresh shell in the source tab's cwd: the
    /// running command is never copied and `default_profile` never
    /// applies (the rule the removed `ToggleAction::NewTab { cwd }`
    /// ride used to smuggle through `explicit_cwd`). The tab lands
    /// directly after the active one whatever `new_tab_after_current`
    /// says, becomes active, and carries the source's rename pin —
    /// `title` and `title_override` set together (the `ensure_tabs`
    /// restore pattern) so the bar shows the name immediately and
    /// the fresh shell's first OSC title cannot clobber it. A missing
    /// channel defers through the bare `pending_remote_tab` stash
    /// exactly like a remote new-tab; that window has no tabs, so no
    /// source ever existed to read from.
    pub(super) fn open_duplicate_tab(
        &mut self,
        cwd: Option<PathBuf>,
        title_override: Option<String>,
    ) -> Task<Message> {
        if self.term_event_tx.is_none() {
            self.pending_remote_tab = Some(ToggleAction::NewTab {
                cwd: None,
                cmd: None,
                profile: None,
            });
            return Task::none();
        }
        self.ensure_tabs();
        if let Some(mut tab) =
            self.spawn_shell(&SpawnCommand::Shell(self.config.resolved_shell()), cwd)
        {
            if let Some(name) = title_override {
                name.clone_into(&mut tab.title);
                tab.title_override = Some(name);
            }
            // `true` pins the duplicate's documented after-active
            // landing: `new_tab_after_current` moves new tabs, never
            // duplicates.
            let at = tabs::insert_new_tab(&mut self.tabs, self.active_tab, true, tab);
            // The spawn title through the live mode: a renamed
            // duplicate keeps its pin (`title_override` wins the
            // resolution), an unrenamed one resolves from the cwd it
            // duplicated into — before its own shell reports.
            self.retitle_tab(at);
            let switch = self.set_active_tab(at);
            return Task::batch([switch, self.mark_session_dirty()]);
        }
        Task::none()
    }

    /// Reads the source tab at `index` — its cwd and rename pin — and
    /// opens its duplicate: the one duplicate source-read every
    /// surface funnels into (the keybind and terminal-menu row via the
    /// active index, the remote `--tab-index` verb through its
    /// resolved target, the per-tab menu's row through the clicked
    /// tab). A `None` or out-of-range source reads as no source:
    /// `open_duplicate_tab` then spawns the pty-default fresh tab
    /// (the keybind's empty-list behavior).
    pub(super) fn duplicate_tab_at(&mut self, source: Option<usize>) -> Task<Message> {
        let (cwd, title_override) =
            source
                .and_then(|index| self.tabs.get(index))
                .map_or((None, None), |tab| {
                    (
                        tab.terminal.lock().unwrap().working_directory(),
                        tab.title_override.clone(),
                    )
                });
        self.open_duplicate_tab(cwd, title_override)
    }

    /// Duplicates the tab with `id` — the per-tab context menu's
    /// Duplicate Tab row. The id resolves against the live list at
    /// apply time (the `TabSelected` rule): a tab closed between the
    /// menu's open and the pick is a quiet no-op, never a duplicate of
    /// whatever took its slot.
    pub(super) fn duplicate_tab(&mut self, id: TabId) -> Task<Message> {
        match self.tabs.iter().position(|tab| tab.id == id) {
            Some(index) => self.duplicate_tab_at(Some(index)),
            None => Task::none(),
        }
    }

    /// Flips the pin of the tab with `id` — the per-tab menu's
    /// Pin/Unpin row, the one surface that arms a pin (a restore
    /// replaying a saved pin is the other path in). The flip marks
    /// the session dirty so the pin persists; an unresolvable id — a
    /// tab closed between the menu's open and the pick — is a quiet
    /// no-op (the `TabSelected` rule).
    pub(super) fn toggle_pin(&mut self, id: TabId) -> Task<Message> {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.pinned = !tab.pinned;
            return self.mark_session_dirty();
        }
        Task::none()
    }

    /// Nudges the tab with `id` one slot toward `direction`, clamping
    /// at the edge — the per-tab menu's Move rows, the move binds'
    /// move with the clicked tab in place of the active one. The
    /// active tab stays active whichever tab moved (`reorder_tabs`'s
    /// identity rule); like every layout change the move marks the
    /// session dirty. A pinned tab refuses the move — it stays put
    /// (ruling 4), other tabs reorder around it freely. An
    /// unresolvable id or an edge clamp is a quiet no-op.
    pub(super) fn move_tab_by(
        &mut self,
        id: TabId,
        direction: tabs::MoveDirection,
    ) -> Task<Message> {
        if let Some(from) = self.tabs.iter().position(|tab| tab.id == id) {
            if self.tabs[from].pinned {
                log::warn!("refusing to move pinned tab {id:?}; unpin it to reorder");
                return Task::none();
            }
            if let Some(to) = tabs::move_target(from, self.tabs.len(), direction) {
                self.active_tab = tabs::reorder_tabs(&mut self.tabs, self.active_tab, from, to);
                // The moved tab may be the active one or shift it by
                // one; the refresh is the uniform every-assignment
                // rule.
                self.refresh_active_window_bg();
                return self.mark_session_dirty();
            }
        }
        Task::none()
    }

    /// Closes the `sweep` neighbors of the tab with `id` — the
    /// per-tab menu's Close Other Tabs and Close Tabs to the Right
    /// rows — every one through the gated front door
    /// ([`Self::request_close_tab`]), so the close confirmation holds
    /// per running tab: tabs without a foreground child close at
    /// once, while the running ones prompt one at a time — the
    /// single-slot confirmation ends up armed for the leftmost of
    /// them, the last the descending sweep arms, and the rest stay
    /// open until re-invoked. Pinned tabs are skipped by the target
    /// list itself ([`tabs::sweep_targets`]) — and would refuse the
    /// front door anyway, so a pin armed between list and apply still
    /// holds. Descending order also keeps earlier indices valid as
    /// the list shrinks. An unresolvable anchor or an empty sweep is
    /// a quiet no-op.
    pub(super) fn close_tabs_sweep(&mut self, id: TabId, sweep: CloseSweep) -> Task<Message> {
        let Some(anchor) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Task::none();
        };
        let targets = tabs::sweep_targets(&self.tabs, anchor, sweep, |tab| tab.pinned);
        let mut tasks = Vec::new();
        for index in targets.into_iter().rev() {
            tasks.push(self.request_close_tab(index));
        }
        Task::batch(tasks)
    }

    /// The front door every user-initiated close rides (close-confirm
    /// criterion 3): the keybind and context-menu row
    /// ([`Action::CloseTab`]), the tab-bar `×` and bar middle-click
    /// ([`Message::TabClosed`]), and the remote `close-tab` verb —
    /// with the toggle on and a foreground child running (criterion
    /// 2's detection), the single-slot pending close arms and the
    /// in-surface modal waits for a human decision; otherwise the
    /// close proceeds now through [`Self::close_tab_at`],
    /// byte-identical to today (criterion 5). A pinned tab refuses
    /// every one of these paths — warn and no-op, no dialog (ruling
    /// 2); unpin first. Shell exits (`TermEvent::Exit`) never route
    /// here — the process already ended — so a dead pinned tab still
    /// clears. The decision reads config at request time, so a
    /// mid-session toggle flip applies to the very next close.
    pub(super) fn request_close_tab(&mut self, index: usize) -> Task<Message> {
        let Some(tab) = self.tabs.get(index) else {
            return Task::none();
        };
        let id = tab.id;
        if tab.pinned {
            log::warn!("refusing to close pinned tab {id:?}; unpin it first");
            return Task::none();
        }
        let running = tab.terminal.lock().unwrap().foreground_child_running();
        if confirm_needed(self.config.close_confirmation_enabled, running) {
            // Single-slot (criterion 4): a new request replaces
            // whatever the slot held.
            self.pending_close = Some(id);
            Task::none()
        } else {
            self.close_tab_at(index)
        }
    }

    /// Closes the tab at `index` and activates its successor (or, when
    /// it closed last, its predecessor). A close that took the final
    /// tab immediately spawns a fresh one — the overlay is never left
    /// empty (see `ensure_tabs`). Shared by the close-tab keybind and
    /// by shell exits; `set_focused` reports only real focus changes,
    /// so refocusing a tab that already had focus is a no-op.
    pub(super) fn close_tab_at(&mut self, index: usize) -> Task<Message> {
        let closed = self.tabs.get(index).map(|tab| tab.id);
        // Stale guard (close-confirm criterion 4): every removal rider
        // passes through here — keybind, bar `×`, middle-click,
        // remote, and the never-gated `TermEvent::Exit` — so one hook
        // retires a pending confirmation whose tab just left; no later
        // Proceed may act on it.
        if let Some(id) = closed
            && self.pending_close == Some(id)
        {
            self.pending_close = None;
        }
        if let Some(decision) = tabs::close_tab(&mut self.tabs, index, self.active_tab) {
            self.active_tab = decision.next_active;
            // The window background follows the newly active tab's
            // resolution — closing a profile tab can hand the screen
            // to a plain one (and vice versa) — and the status band's
            // snapshot follows the newly active tab's foreground.
            self.refresh_active_window_bg();
            self.refresh_status_snapshot();
            // The tab activation a close implies also retires the
            // newly active tab's attention marker, exactly like a
            // click or a numbered-switch bind would.
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.attention = tab.attention.on_activated();
            }
            if decision.respawn {
                self.ensure_tabs();
            }
            self.focus_active_tab();
            if let Some(id) = closed {
                self.tab_cwds.remove(&id);
            }
            // Every close comes through here — keybind, tab-bar `×`,
            // middle-click, and a shell exit (`TermEvent::Exit`) — so
            // this is the close trigger's single save-marking site.
            let mark = self.mark_session_dirty();
            // A close can orphan the rename input: its tab may be
            // gone, or the bar may have vanished with a single tab
            // left. End the session so the terminal regains keyboard
            // focus.
            return Task::batch([mark, self.prune_rename_session()]);
        }
        self.prune_rename_session()
    }

    /// Switches the active tab to `index`, reporting the focus change to
    /// both shells (`FOCUS_OUT` to the outgoing one, `FOCUS_IN` to the
    /// incoming one), the same way a window focus change does, and
    /// moving the window background to the incoming tab's color
    /// resolution. A real switch marks the session dirty — the focused
    /// index is saved state; the index-unchanged early return never
    /// marks.
    pub(super) fn set_active_tab(&mut self, index: usize) -> Task<Message> {
        // Cycling with a single tab is a no-op; refocusing it anyway would
        // send the shell a spurious FOCUS_OUT/FOCUS_IN pair.
        if index == self.active_tab {
            return Task::none();
        }
        if let Some(old) = self.active() {
            old.terminal.lock().unwrap().set_focused(false);
        }
        self.active_tab = index;
        // The window background follows the switch: the incoming tab's
        // resolution replaces the outgoing one's. The status band's
        // snapshot follows the same piggyback point (criterion 5).
        self.refresh_active_window_bg();
        self.refresh_status_snapshot();
        // Activation retires whatever attention marker the tab wore:
        // the user is now looking at it.
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.attention = tab.attention.on_activated();
        }
        self.focus_active_tab();
        self.mark_session_dirty()
    }

    /// Marks the active tab's terminal focused. Used after the tab list
    /// changed shape, where the tab that lost focus is already gone.
    pub(super) fn focus_active_tab(&mut self) {
        if let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(true);
        }
    }

    /// Ends an inline rename session sitting on the tab at `index`:
    /// the draft is discarded — it must not commit over a title
    /// arriving from outside — and focus returns to the terminal as
    /// the input overlay goes away. No-op when no session targets
    /// that tab.
    pub(super) fn end_rename_on(&mut self, index: usize) -> Task<Message> {
        let session_on_target = self
            .tabs
            .get(index)
            .is_some_and(|tab| self.rename.as_ref().is_some_and(|edit| edit.tab == tab.id));
        if session_on_target {
            self.rename = None;
            return widget::text_input::focus(self.terminal_id.clone());
        }
        Task::none()
    }

    /// Commits the active inline rename, if any: the draft becomes
    /// the tab's user rename, the session ends, and focus returns to
    /// the terminal. Every committing focus-loss path comes through
    /// here — Enter, a press outside the input, the surface hiding or
    /// losing keyboard focus — while Escape and Tab cancel instead
    /// ([`Message::RenameCancelled`]).
    pub(super) fn commit_pending_rename(&mut self) -> Task<Message> {
        if let Some(edit) = self.rename.take() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == edit.tab) {
                self.commit_rename_on(index, &edit.draft);
            }
            // A rename commit is a save trigger — but only when a
            // rename session actually existed; every committing path
            // comes through here, so this is its single marking site.
            let mark = self.mark_session_dirty();
            return Task::batch([mark, widget::text_input::focus(self.terminal_id.clone())]);
        }
        Task::none()
    }

    /// Ends the active rename when its input can no longer be shown: the
    /// renamed tab is gone, or the bar is hidden now that a single tab
    /// remains. Returns the task refocusing the terminal, if a
    /// session ended.
    fn prune_rename_session(&mut self) -> Task<Message> {
        if let Some(edit) = &self.rename {
            let shown = self.tabs.len() > 1 && self.tabs.iter().any(|tab| tab.id == edit.tab);
            if !shown {
                self.rename = None;
                return widget::text_input::focus(self.terminal_id.clone());
            }
        }
        Task::none()
    }

    /// Starts an inline rename of the tab with `id`: the draft seeds
    /// from the tab's current title, and the input is focused with its
    /// value selected. Both rename starters — the tab bar's
    /// double-click ([`Message::RenameStarted`]) and the
    /// `Ctrl+Shift+R` bind / context-menu row ([`Action::RenameTab`])
    /// — meet here; an unknown `id` is a no-op (the tab may have
    /// closed between the press and the apply).
    pub(super) fn start_rename(&mut self, id: TabId) -> Task<Message> {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
            let draft = tab.title.clone();
            self.rename = Some(RenameEdit { tab: id, draft });
            // Focus the input with its value selected.
            return Task::batch([
                widget::text_input::focus(self.rename_input_id.clone()),
                widget::text_input::select_all(self.rename_input_id.clone()),
            ]);
        }
        Task::none()
    }

    /// Marks the session dirty and arms the debounce window: at most
    /// one `tokio::sleep` is ever in flight — a mark while a window
    /// is armed coalesces into that window's write — and only a mark
    /// after a write re-arms. Returns the window's timer task when
    /// this call armed it, the `WindowPinDue`/`WriteDue` pattern.
    pub(super) fn mark_session_dirty(&mut self) -> Task<Message> {
        // The session toggle gates every save trigger: a disabled
        // trigger no-ops — no dirty mark, no debounce window, no
        // timer task — so a mid-run disable stops further saves
        // immediately.
        match self
            .session_save
            .mark_gated(self.config.session_persistence_enabled)
        {
            Some(generation) => Task::perform(
                tokio::time::sleep(session::SESSION_WRITE_INTERVAL),
                move |()| cosmic::Action::App(Message::SessionSaveDue(generation)),
            ),
            None => Task::none(),
        }
    }

    /// Captures the live tab list and writes the session file
    /// atomically. The cwd readlinks happen here — at write time, not
    /// schedule time — so saved cwds are as fresh as the debounce
    /// allows; `capture` then staleness-filters each one. A failed
    /// write logs and leaves the old file in place: the next dirty
    /// window retries.
    pub(super) fn write_session(&mut self) {
        let cwds: Vec<Option<PathBuf>> = self
            .tabs
            .iter()
            .map(|tab| tab.terminal.lock().unwrap().working_directory())
            .collect();
        let snapshots: Vec<session::TabSnapshot<'_>> = self
            .tabs
            .iter()
            .zip(&cwds)
            .map(|(tab, cwd)| session::TabSnapshot {
                title: &tab.title,
                title_override: tab.title_override.as_deref(),
                shell: &tab.shell,
                cwd: cwd.as_deref(),
                ephemeral: tab.ephemeral,
                profile: tab.profile.as_deref(),
                pinned: tab.pinned,
            })
            .collect();
        let state = session::capture(&snapshots, self.active_tab);
        if let Err(err) = session::save_atomic(&session::state_file_path(), &state) {
            log::warn!("failed to write session file: {err}");
        }
    }

    /// The cwd-change gate on a shell title event: one readlink,
    /// compared against the tab's cached cwd. A moved cwd marks the
    /// session dirty and updates the cache; a title churn with the
    /// cwd holding — a vim statusline — stays quiet.
    fn check_tab_cwd(&mut self, tab_index: usize) -> Task<Message> {
        let tab_id = self.tabs[tab_index].id;
        let cwd = self.tabs[tab_index]
            .terminal
            .lock()
            .unwrap()
            .working_directory();
        if session::cwd_changed(
            self.tab_cwds.get(&tab_id).and_then(Option::as_deref),
            cwd.as_deref(),
        ) {
            self.tab_cwds.insert(tab_id, cwd);
            return self.mark_session_dirty();
        }
        Task::none()
    }

    /// Re-resolves one tab's displayed title from its stored state —
    /// the shared tail of every title apply point: a report arriving
    /// or resetting, a rename clearing, the mode-flip retitle loop,
    /// and a fresh tab's spawn (before any report exists). The cwd
    /// comes from the `tab_cwds` cache, seeded at spawn and refreshed
    /// by [`Self::check_tab_cwd`] — which is why the report handlers
    /// sample the cwd *before* resolving. The tab's profile id
    /// resolves to its current name here — at retitle time, not
    /// spawn — so a profile renamed or deleted under a running tab
    /// reflects on this very retitle: a dangling id yields no name
    /// and Profile mode renders the Shell fallback, never a stale
    /// name.
    pub(super) fn retitle_tab(&mut self, tab_index: usize) {
        let mode = self.config.tab_title_mode;
        let profile_name = self
            .tabs
            .get(tab_index)
            .and_then(|tab| tab.profile.as_deref())
            .and_then(|id| profile::find(&self.config.profiles, id))
            .map(|preset| preset.name.as_str());
        if let Some(tab) = self.tabs.get_mut(tab_index) {
            let cwd = self.tab_cwds.get(&tab.id).and_then(Option::as_deref);
            tab.title = tabs::resolve_title(
                mode,
                tab.shell_title.as_deref(),
                cwd,
                profile_name,
                tab.id,
                tab.title_override.as_deref(),
            );
        }
    }

    /// A tab-title-mode change re-resolves every live tab's title
    /// from stored state (Guake `recompute_tabs_titles`): renamed
    /// tabs are left to [`tabs::resolve_title`]'s override rule,
    /// which keeps them untouched. Called only on an old-vs-new mode
    /// delta, so unrelated config echoes never churn titles.
    pub(super) fn retitle_all_tabs(&mut self) {
        for index in 0..self.tabs.len() {
            self.retitle_tab(index);
        }
    }

    /// Applies a rename commit on one tab — both rename callers, the
    /// tab-bar inline rename and the remote `rename-tab`, meet here:
    /// the override is set or cleared and the displayed title
    /// re-resolves per the live mode (see [`tabs::commit_rename`]);
    /// the profile name resolves at commit time exactly as
    /// [`Self::retitle_tab`]'s does, so a cleared rename on a profile
    /// tab in Profile mode shows the profile's current name.
    pub(super) fn commit_rename_on(&mut self, tab_index: usize, new_title: &str) {
        let mode = self.config.tab_title_mode;
        let profile_name = self
            .tabs
            .get(tab_index)
            .and_then(|tab| tab.profile.as_deref())
            .and_then(|id| profile::find(&self.config.profiles, id))
            .map(|preset| preset.name.as_str());
        if let Some(tab) = self.tabs.get_mut(tab_index) {
            let cwd = self.tab_cwds.get(&tab.id).and_then(Option::as_deref);
            tabs::commit_rename(
                mode,
                tab.shell_title.as_deref(),
                cwd,
                profile_name,
                tab.id,
                &mut tab.title,
                &mut tab.title_override,
                new_title,
            );
        }
    }

    pub(super) fn handle_term_event(&mut self, tab_id: TabId, event: TermEvent) -> Task<Message> {
        let Some(tab_index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            // Events for a tab that is already gone (e.g. a trailing
            // Wakeup after its shell exited) have no target.
            return Task::none();
        };
        match event {
            // The active tab never accrues a marker: the user is
            // already looking at it.
            TermEvent::Bell => {
                // Only an inactive tab accrues a marker — the user
                // is already looking at the active one.
                if tab_index != self.active_tab {
                    let tab = &mut self.tabs[tab_index];
                    tab.attention = tab.attention.on_bell();
                }
                // A bell while the overlay is hidden posts one
                // desktop notification per tab per hide period (the
                // latch arms here and clears on every show). While
                // visible the marker above alone carries the signal.
                let tab = &mut self.tabs[tab_index];
                if notify::should_notify(
                    matches!(self.surface, SurfaceState::Hidden),
                    self.config.bell_notification_enabled,
                    tab.bell_notified,
                ) {
                    tab.bell_notified = true;
                    let title = tab.title.clone();
                    notify::post_bell_notification(title);
                }
            }
            TermEvent::CursorBlinkingChange | TermEvent::ChildExit(_) => {}
            TermEvent::ClipboardLoad(kind, format) => match kind {
                aterm::ClipboardType::Clipboard => {
                    return clipboard::read().map(move |data| {
                        format(&data.unwrap_or_default());
                        cosmic::action::none()
                    });
                }
                aterm::ClipboardType::Selection => {
                    return clipboard::read_primary().map(move |data| {
                        format(&data.unwrap_or_default());
                        cosmic::action::none()
                    });
                }
            },
            TermEvent::ClipboardStore(kind, data) => match kind {
                aterm::ClipboardType::Clipboard => return clipboard::write(data),
                aterm::ClipboardType::Selection => return clipboard::write_primary(data),
            },
            TermEvent::ColorRequest(index, format) => {
                let terminal = self.tabs[tab_index].terminal.lock().unwrap();
                let rgb = terminal.effective_color(index);
                terminal.input_no_scroll(format(rgb).into_bytes());
            }
            TermEvent::PtyWrite(text) => {
                self.tabs[tab_index]
                    .terminal
                    .lock()
                    .unwrap()
                    .input_no_scroll(text.into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                let terminal = self.tabs[tab_index].terminal.lock().unwrap();
                let text = format(terminal.size().into());
                terminal.input_no_scroll(text.into_bytes());
            }
            TermEvent::Title(title) => {
                // The raw report is stored in every mode — retitle
                // inputs stay fresh, so a mode flip or a cleared
                // rename re-resolves from current state.
                self.tabs[tab_index].shell_title = Some(title);
                // Title events double as the cwd-change sample point,
                // and the path modes resolve against the fresh cwd:
                // sample first, then re-resolve. The title itself is
                // not a save trigger — churn alone writes nothing.
                let cwd_task = self.check_tab_cwd(tab_index);
                self.retitle_tab(tab_index);
                // A report from the active tab is one of the status
                // band's snapshot piggyback points (criterion 5) —
                // the foreground process often changed to report.
                if tab_index == self.active_tab {
                    self.refresh_status_snapshot();
                }
                return cwd_task;
            }
            TermEvent::ResetTitle => {
                // A shell reset (RIS) clears the stored report;
                // resolution restores the mode's title — the default
                // in Shell mode — unless the user renamed the tab.
                self.tabs[tab_index].shell_title = None;
                let cwd_task = self.check_tab_cwd(tab_index);
                self.retitle_tab(tab_index);
                if tab_index == self.active_tab {
                    self.refresh_status_snapshot();
                }
                return cwd_task;
            }
            TermEvent::Wakeup => {
                // Output arrived: an inactive tab accrues an
                // activity marker; the active tab shows the output
                // itself, so nothing to signal — but its wakeup is
                // the band's finest piggyback point: a process that
                // just produced output is exactly when the name
                // should be fresh (criterion 5).
                if tab_index == self.active_tab {
                    self.refresh_status_snapshot();
                } else {
                    let tab = &mut self.tabs[tab_index];
                    tab.attention = tab.attention.on_activity();
                }
                self.tabs[tab_index].terminal.lock().unwrap().needs_update = true;
            }
            TermEvent::MouseCursorDirty => {
                self.tabs[tab_index].terminal.lock().unwrap().needs_update = true;
            }
            TermEvent::Exit => {
                // A tab whose shell exits is closed; when it was the
                // last, a fresh tab spawns in its place so the
                // overlay is never left without a terminal (see
                // `ensure_tabs`). A profile tab is the exception: its
                // command exiting respawns the plain shell in place
                // (the one-shot ruling — see
                // [`Self::respawn_shell_after_profile_exit`]), so a
                // profile like `ls` neither vanishes a New Tab pick
                // nor destroys the slot a Switch Profile pick
                // replaced.
                if exit_respawns_shell(
                    self.tabs[tab_index].profile.as_deref(),
                    self.tabs[tab_index].ephemeral,
                ) {
                    return self.respawn_shell_after_profile_exit(tab_index);
                }
                return self.close_tab_at(tab_index);
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_menu::{ROWS, rows};

    #[test]
    fn trees_yield_one_tree_per_row_with_no_profiles() {
        assert_eq!(trees(&[]).len(), ROWS.len());
    }

    #[test]
    fn open_trees_is_none_while_the_menu_is_closed() {
        // A closed menu supplies no trees, so `widget::context_menu`
        // renders and passes events through as a plain wrapper — the
        // pixel-identical closed view.
        assert!(open_trees(false, &[]).is_none());
    }

    #[test]
    fn open_trees_carries_every_row_while_the_menu_is_open() {
        let trees = open_trees(true, &[]).expect("the open menu supplies its trees");
        assert_eq!(trees.len(), ROWS.len());
    }

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_owned(),
            command: format!("run {name}"),
            cwd: None,
            color_scheme: None,
            env: None,
            font_size: None,
            scrollback_lines: None,
        }
    }

    #[test]
    fn open_trees_stays_none_while_closed_even_with_profiles() {
        assert!(open_trees(false, &[profile("ssh-web")]).is_none());
    }

    #[test]
    fn trees_carry_one_tree_per_row_with_profiles() {
        let profiles = [profile("ssh-web"), profile("htop")];
        assert_eq!(trees(&profiles).len(), rows(&profiles).len());
    }

    #[test]
    fn rename_overlay_needs_a_hosted_tab_button() {
        // The rename input overlays a tab button, and buttons show
        // only with two or more tabs (the bar's own visibility rule,
        // see `prune_rename_session`): below that there is nothing to
        // host the input, so the rename bind is a swallowed no-op.
        assert!(!rename_overlay_hosted(0));
        assert!(!rename_overlay_hosted(1));
        assert!(rename_overlay_hosted(2));
        assert!(rename_overlay_hosted(5));
    }

    #[test]
    fn only_a_profile_tab_s_exit_respawns_the_shell() {
        // The one-shot ruling's routing pin: a profile tab's exit
        // respawns the plain shell in place — a plain shell's exit
        // (the user typed `exit`) and a `--cmd` tab's exit (ephemeral
        // by design) keep today's close.
        assert!(exit_respawns_shell(Some("hahaha"), false));
        assert!(!exit_respawns_shell(None, false));
        assert!(!exit_respawns_shell(None, true));
        // Exclusive by construction today; the defensive half of the
        // pin — a hypothetical profile-carrying ephemeral tab must
        // still close, never respawn.
        assert!(!exit_respawns_shell(Some("hahaha"), true));
    }
}
