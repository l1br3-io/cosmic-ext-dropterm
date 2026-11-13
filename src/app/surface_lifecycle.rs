// SPDX-License-Identifier: GPL-3.0-only

//! The surface lifecycle's frontend half (shared-core spec): the thin
//! glue that applies the decision core's intents through the
//! libcosmic executor and keeps the app-side state beside them
//! (tabs, drawer, rename, the terminal-height pin). The decision
//! core itself — the state machine, the toggle funnel, the pure
//! guards — lives in `dropterm-core::surface_lifecycle`, re-exported
//! below so every `surface_lifecycle::` path in the app keeps
//! resolving.

use cosmic::app::Task;

use super::{DropTerm, Message, ToggleAction};
use crate::geometry::{self, Geometry};
use crate::intent::{SurfaceExecutor, SurfaceHandle, SurfaceIntent};
use crate::settings;

pub(crate) use dropterm_core::surface_lifecycle::{
    SurfaceInputs, SurfaceMachine, SurfaceState, ToggleIntent, ToggleStep, grab_bar_acts_on,
    layer_unfocused_acts_on, resize_acts_on, toggle_step,
};

/// The funnel verb a surface-facing `ToggleAction` carries. The
/// remote tab verbs never reach the funnel — `debounced_toggle`
/// applies each before this point — so the mapping is total and
/// loud: a verb that leaked through is a routing bug to hear about,
/// not silently reinterpreted.
fn funnel(action: &ToggleAction) -> ToggleIntent {
    match action {
        ToggleAction::Toggle => ToggleIntent::Toggle,
        ToggleAction::Show => ToggleIntent::Show,
        ToggleAction::Hide => ToggleIntent::Hide,
        ToggleAction::NewTab { .. }
        | ToggleAction::CloseTab { .. }
        | ToggleAction::SelectTab { .. }
        | ToggleAction::RenameTab { .. }
        | ToggleAction::DuplicateTab { .. }
        | ToggleAction::SwitchProfileTab { .. } => {
            unreachable!("remote verbs apply outside the funnel")
        }
    }
}

impl DropTerm {
    /// Applies a decision's intents through the libcosmic executor,
    /// batched — exactly today's composition (the measure pass's
    /// anchor heal chains strictly ahead of its probe inside the
    /// executor's own translation; no other intent sequences).
    pub(super) fn apply_surface_intents(&mut self, intents: &[SurfaceIntent]) -> Task<Message> {
        let tasks: Vec<Task<Message>> = intents
            .iter()
            .copied()
            .map(|intent| self.surface_exec.execute(intent))
            .collect();
        Task::batch(tasks)
    }

    pub(super) fn show(&mut self) -> Task<Message> {
        // The user can see the terminal again: re-arm every tab's
        // notification latches — bell and finished-command — so a
        // bell or a finished long command from here on is fresh news
        // worth one notification per tab again.
        for tab in &mut self.tabs {
            tab.bell_notified = false;
            tab.command_notified = false;
        }
        self.ensure_tabs();
        let intents = self
            .surface
            .decide_show(SurfaceInputs::from_config(&self.config));
        self.apply_surface_intents(&intents)
    }

    pub(super) fn hide(&mut self) -> Task<Message> {
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
        // preserves it (the named no-op; only the explicit close's
        // `turn_settings_page` away from the Profiles tab clears).
        settings::apply_profile_draft_transition(
            &mut self.profile_draft,
            settings::ProfileDraftTransition::Hide,
        );
        let intents = self
            .surface
            .decide_hide(SurfaceInputs::from_config(&self.config));
        let task = self.apply_surface_intents(&intents);
        // The surface is leaving on every destructive path, and the
        // cleanup is idempotent for a leave-with-nothing: the drag
        // is the machine's (cleared inside `decide_hide`), the
        // terminal pin's debounce state starts over so the next
        // show's first granted size adopts immediately, and the
        // active terminal keeps its reported focus — the dropdown's
        // own visibility toggle is not a focus event for the PTY (a
        // focus-reporting program — fish, vim, tmux — would repaint
        // its prompt per toggle), and the PTY keeps running
        // regardless, because PTYs live in tabs, not the window.
        self.pending_height = None;
        self.pin_applied = None;
        // The context menu's popup dies with the surface and its
        // `on_close` never fires — clear the latch here, or the next
        // show's first right-click would be consumed as a
        // toggle-close of a menu that is not there.
        self.context_menu_open = false;
        Task::batch([commit, task])
    }

    /// Reverses a hiding slide back to shown (see
    /// [`SurfaceMachine::decide_reverse_show`]); the executor's
    /// translation re-holds keyboard per the resolved mode and
    /// focuses the terminal input.
    pub(super) fn reverse_show(&mut self, handle: SurfaceHandle) -> Task<Message> {
        let intents = self
            .surface
            .decide_reverse_show(handle, SurfaceInputs::from_config(&self.config));
        self.apply_surface_intents(&intents)
    }

    /// Handles the surface losing keyboard focus: marks the terminal
    /// unfocused and, when configured, hides the overlay. The hide is
    /// guarded by the focus latch so a stray unfocus before any focus
    /// (possible with `keyboard_mode: OnDemand`) can't close a
    /// freshly-opened surface. The latch is cleared only when the
    /// hide fires; a loss that doesn't hide (the config off) leaves
    /// it armed. No settings-window case exists here: while that
    /// window is open the surface is hidden by construction, so
    /// focus-loss events cannot arrive for a shown surface in that
    /// state.
    pub(super) fn focus_lost(&mut self) -> Task<Message> {
        // Losing keyboard focus commits a rename: the input's own
        // unfocus signal only covers Escape and Tab, which cancel.
        let commit = self.commit_pending_rename();
        let (report_unfocus, hide) = self
            .surface
            .decide_focus_loss(SurfaceInputs::from_config(&self.config));
        // Only a shown surface's focus loss reaches the PTY.
        if report_unfocus && let Some(tab) = self.active() {
            tab.terminal.lock().unwrap().set_focused(false);
        }
        if hide {
            return Task::batch([commit, self.hide()]);
        }
        commit
    }

    /// Applies a live drag decision's outcome: the intents through
    /// the executor, and — when the decision applied a target — the
    /// pin half beside it. The pin declaration matters before the
    /// offer: the cadence-gated adoption is the drag's own grant
    /// path, and the echo gate needs to know this height was asked
    /// for; adoptions stay debounced (`offer_window_height`) because
    /// the runtime double-reports every `set_size` (once stale, once
    /// real) and each adoption costs a terminal reflow plus a pty
    /// resize that made the drag sluggish on hardware.
    pub(super) fn apply_drag(
        &mut self,
        intents: &[SurfaceIntent],
        applied: Option<u32>,
    ) -> Task<Message> {
        let Some(target) = applied else {
            return self.apply_surface_intents(intents);
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "surface px are < 2^24, exact in f32"
        )]
        let requested = target as f32;
        self.requested_height = Some(requested);
        let pin_task = self.offer_window_height(requested);
        let apply = self.apply_surface_intents(intents);
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
    pub(super) fn offer_window_height(&mut self, height: f32) -> Option<Task<Message>> {
        // The stale-echo gate (toggle-grid-stability, round 2): a
        // granted height may only reach the debounce when it is the
        // grant of a request we made or a re-report of the adopted
        // pin. Anything else is the synthetic pre-request echo every
        // `set_size` double-reports — the full-output stretch at
        // show, an intermediate drag height after a release — and
        // the interval debounce only defers it while it lands
        // inside the window; on hardware the startup echo arrived
        // after wgpu init, outwaited the interval, and the
        // pre-armed pin adopted it. Dropped here, timing-free.
        if geometry::offer_is_stale_echo(self.requested_height, self.window_height, height) {
            return None;
        }
        let now = std::time::Instant::now();
        if geometry::window_pin_due(self.pin_applied, now, height) {
            self.window_height = Some(height);
            // The spawn-grid settle's close (status-glance criterion
            // 1): the granted stream just settled — and the grant
            // already applied (the draw resized before the echo
            // arrived), while `Terminal::resize` no-ops on unchanged
            // sizes, so waiting for the next changed resize would
            // wait forever and the fresh-prompt latch would never
            // arm (the 2026-10-24 fresh-start trace). Close at the
            // grid already held. A no-op on settled records, so a
            // re-show's adoption can never absorb a live deviation
            // into the record.
            for tab in &self.tabs {
                tab.terminal.lock().unwrap().settle_spawn_grid_now();
            }
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

    /// Pre-arm the terminal-height pin at the height the show just
    /// requested: [`SurfaceOpened`] calls this before its reshape
    /// tasks run, so the granted-size echoes those tasks produce
    /// meet an armed pin, not an empty one. The load-bearing case
    /// is the stale synthetic configure every `set_size` carries —
    /// at show time it reports the measuring pass's full-output
    /// height, and an empty pin adopts immediately
    /// (`window_pin_due(None, ..)`), growing the grid once per
    /// toggle and pushing the grown lines into scrollback on the
    /// shrink back. Armed, that echo matches neither the request
    /// nor the adopted pin and drops at the stale-echo gate
    /// ([`geometry::offer_is_stale_echo`]) whatever the clock says;
    /// the real grant equals the armed height and re-adopts for
    /// free. The pre-arm only ever names the request, never a
    /// grant: under the gate, a grant that differed from the
    /// request would drop as echo noise too — the compositor
    /// grants each `set_size` exactly ([`geometry::DRAG_APPLY_INTERVAL`]'s
    /// contract), and the pin would rest on the request until the
    /// next show's measuring pass re-reads the output and re-arms.
    pub(super) fn prearm_show_pin(&mut self, output: (u32, u32), geom: Geometry) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        let height = geometry::surface_height(output.1 as f32, geom.height_percent) as f32;
        self.window_height = Some(height);
        self.pin_applied = Some((std::time::Instant::now(), height));
        self.pending_height = None;
        self.requested_height = Some(height);
    }

    /// Record the height a geometry reshape (`set_size`) is about to
    /// request, without touching the pin: the config-edit reshape
    /// and the drag release call this beside their `apply_geometry`,
    /// so the stale-echo gate can tell their grant from the
    /// synthetic pre-request echo the double-report carries. The
    /// show path pre-arms instead ([`Self::prearm_show_pin`]) —
    /// arming the pin ahead of the request is what makes the show
    /// pass inert; the later reshapes keep the live pin and only
    /// declare their request.
    pub(super) fn request_geometry_height(&mut self, output: (u32, u32), geom: Geometry) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        let height = geometry::surface_height(output.1 as f32, geom.height_percent) as f32;
        self.requested_height = Some(height);
    }

    /// The surface's current width in px: the value both the drawer
    /// width policy (render + drag) and the height drag target
    /// against. 480 while no output has reported a size — the same
    /// fallback the drawer's first frame renders at.
    pub(super) fn current_surface_width(&self) -> f32 {
        self.surface.output_size.map_or(480.0, |(width, _)| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "output px are < 2^24, exact in f32"
            )]
            {
                geometry::surface_width(width as f32, self.config.width_percent) as f32
            }
        })
    }

    pub(super) fn debounced_toggle(&mut self, action: ToggleAction) -> Task<Message> {
        if !self.surface.toggle_accepted(std::time::Instant::now()) {
            return Task::none();
        }

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
                crate::tabs::remote_target_index(self.tabs.len(), tab_index, self.active_tab)
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
        match toggle_step(
            funnel(&action),
            &self.surface.state,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::DestroyNow(_) | ToggleStep::SlideOut(_) => self.hide(),
            ToggleStep::ReverseShow(handle) => self.reverse_show(handle),
            ToggleStep::Leave => Task::none(),
        }
    }

    /// The surface half a tab-facing remote verb takes: the same show
    /// decision a `new-tab` gets, so the verb shows the overlay
    /// whatever state it was in — and a request arriving
    /// mid-slide-out reverses on the live surface exactly like a
    /// `show` would.
    pub(super) fn show_intent_surface(&mut self) -> Task<Message> {
        match toggle_step(
            ToggleIntent::Show,
            &self.surface.state,
            self.config.slide_animation_enabled,
        ) {
            ToggleStep::Measure => self.show(),
            ToggleStep::ReverseShow(handle) => self.reverse_show(handle),
            _ => Task::none(),
        }
    }
}
