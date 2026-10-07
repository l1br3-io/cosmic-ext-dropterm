// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;
use std::sync::Mutex;

use cosmic::app::Task;
use cosmic::iced::clipboard;
use cosmic::widget::menu::{self, action::MenuAction};

use super::DropTerm;
use super::Message;
use super::ToggleAction;
use super::spawn::{
    SpawnCommand, SpawnedTerminal, default_spawn_command, live_preset_cwd, profile_pick,
};
use crate::context_menu::items;
use crate::keybinds;
use crate::profile::Profile;
use crate::tabs::{self, TabId};
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
        // An open inline rename on the target is discarded (the
        // rename-tab precedent): the draft must not commit over the
        // respawn, and the explicit end gives the deterministic
        // refocus below.
        let refocus = self.end_rename_on(index);
        let old_override = self.tabs[index].title_override.clone();
        let Some(SpawnedTerminal {
            tab_id,
            terminal,
            shell,
            ..
        }) = self.spawn_terminal(&command, respawn_cwd.as_deref())
        else {
            // Deferred (no event channel yet) or the spawn errored —
            // both already logged; the tab is untouched.
            return Task::none();
        };
        let plan = tabs::plan_respawn(
            self.config.tab_title_mode,
            tab_id,
            respawn_cwd.as_deref(),
            old_override.as_deref(),
            name,
        );
        let target = &mut self.tabs[index];
        // Every field the old terminal's life touched comes from the
        // plan — the profile id included: the tab is a profile tab
        // from now on, whatever it was before. (A degraded blank
        // command line still records the pick; the next restore
        // re-derives the pair through `spawn_shell`, which drops the
        // id for a degraded line, so a broken preset heals itself
        // after one cycle.)
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
        target.id = tab_id;
        target.terminal = Mutex::new(terminal);
        // The cwd cache follows the id swap: drop the dead tab's
        // entry, seed the new one with the respawn cwd so the fresh
        // shell's first OSC title report reads as unchanged.
        self.tab_cwds.remove(&tab);
        self.tab_cwds.insert(tab_id, respawn_cwd);
        Task::batch([refocus, self.mark_session_dirty()])
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
                let mut terminal = tab.terminal.lock().unwrap();
                let adj = match action {
                    Action::ZoomIn => terminal.zoom_adj().saturating_add(1),
                    Action::ZoomOut => terminal.zoom_adj().saturating_sub(1),
                    _ => 0,
                };
                terminal.set_zoom_adj(adj);
                terminal.update_cell_size();
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
                // exactly today's shell spawn.
                let command = default_spawn_command(&self.config);
                let spawn_cwd = match &command {
                    SpawnCommand::Profile {
                        name, cwd: preset, ..
                    } => live_preset_cwd(name, preset.as_ref()).or(inherited),
                    _ => tabs::resolve_new_tab_cwd(None, inherited),
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
            Action::CloseTab => return self.close_tab_at(self.active_tab),
            Action::DuplicateTab => {
                // Duplicate the active tab: capture its cwd and rename
                // pin before the helper's `&mut self` call — `active()`
                // borrows the list the helper inserts into (the
                // `commit_rename_on` precedent). Unlike rename there is
                // no bar-visibility gate: duplicating a single tab is
                // meaningful, and the fresh bar it implies shows.
                let source = self.active().map(|tab| {
                    (
                        tab.terminal.lock().unwrap().working_directory(),
                        tab.title_override.clone(),
                    )
                });
                let (cwd, title_override) = source.unwrap_or((None, None));
                return self.open_duplicate_tab(cwd, title_override);
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

    /// Closes the tab at `index` and activates its successor (or, when
    /// it closed last, its predecessor). A close that took the final
    /// tab immediately spawns a fresh one — the overlay is never left
    /// empty (see `ensure_tabs`). Shared by the close-tab keybind and
    /// by shell exits; `set_focused` reports only real focus changes,
    /// so refocusing a tab that already had focus is a no-op.
    pub(super) fn close_tab_at(&mut self, index: usize) -> Task<Message> {
        let closed = self.tabs.get(index).map(|tab| tab.id);
        if let Some(decision) = tabs::close_tab(&mut self.tabs, index, self.active_tab) {
            self.active_tab = decision.next_active;
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
    /// incoming one), the same way a window focus change does. A real
    /// switch marks the session dirty — the focused index is saved
    /// state; the index-unchanged early return never marks.
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
}
