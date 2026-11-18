// SPDX-License-Identifier: GPL-3.0-only

//! The surface lifecycle's decision core (shared-core spec, moved
//! from the frontend's `app/surface_lifecycle.rs`): the state
//! machine, the toggle funnel, and the pure guards — plain data over
//! plain types, headless-constructible, emitting
//! [`SurfaceIntent`]s. The frontend's thin glue applies those intents
//! through its executor and keeps the app-side halves (tabs, drawer,
//! rename) beside them.

use std::time::Instant;

use crate::core::config::{DropTermConfig, KeyboardMode, Layer};
use crate::core::geometry::{self, Geometry};
use crate::core::intent::{ExclusiveZone, KeyboardHold, ShapeHeight, SurfaceHandle, SurfaceIntent};
use crate::core::slide;

/// Debounce window for toggle requests, guarding against the double
/// key-press events a compositor occasionally delivers for one physical
/// press (see the README's known-risks section).
pub const TOGGLE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SurfaceState {
    #[default]
    Hidden,
    /// A surface stretched across the whole active output, waiting for its
    /// `Window::Opened` event so we learn that output's real size.
    Measuring(SurfaceHandle),
    Shown(SurfaceHandle),
    /// A shown surface sliding out toward its anchored edge: the
    /// content still renders — translated by the live `Out` slide —
    /// and the layer surface is destroyed only when that slide
    /// completes. Keyboard interactivity was already released at
    /// slide-out start, so focus events find no `Shown` surface to
    /// act on; the drag handlers and live reshape ignore this state
    /// by construction (they match `Shown` exclusively).
    Hiding(SurfaceHandle),
}

/// The config subset every surface decision reads, resolved once per
/// decision so the machine never sees the whole config — and stays
/// constructible headless (a frontend app struct needs a live
/// toolkit runtime).
#[derive(Debug, Clone, Copy)]
pub struct SurfaceInputs {
    pub geometry: Geometry,
    pub layer: Layer,
    pub keyboard_mode: KeyboardMode,
    pub hide_on_focus_loss: bool,
    pub slide_animation_enabled: bool,
    pub respect_panel: bool,
}

impl SurfaceInputs {
    pub fn from_config(config: &DropTermConfig) -> Self {
        Self {
            geometry: Geometry::from_config(config),
            layer: config.layer,
            keyboard_mode: config.keyboard_mode,
            hide_on_focus_loss: config.hide_on_focus_loss,
            slide_animation_enabled: config.slide_animation_enabled,
            respect_panel: config.respect_panel,
        }
    }
}

/// The toggle funnel's action vocabulary: the three surface verbs a
/// toggle-intent request can ask of the surface. The frontend's
/// richer verb sets (the CLI's remote tab verbs) apply outside the
/// funnel — each is handled before a word ever reaches
/// [`toggle_step`] — so the decision layer owns exactly the verbs
/// that act on the surface, and each frontend maps its own verbs onto
/// these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleIntent {
    /// Reverse: show from hidden, hide from shown.
    Toggle,
    /// Show — a no-op when the surface is already on its way.
    Show,
    /// Hide — idempotent when already hiding or hidden.
    Hide,
}

/// The keyboard mode a *shown* surface runs, resolved from the config
/// pair: `hide_on_focus_loss` off forces `OnDemand`, because a
/// stay-open surface must share focus like a normal window.
///
/// The keyboard-hostage bug this prevents: `Exclusive` held while
/// `hide_on_focus_loss` is off means the compositor's focus fixup
/// returns the keyboard to the terminal on every refresh cycle (a
/// click on another window's input moves focus there for an instant
/// and the next fixup steals it back — with `Exclusive` held and no
/// focus-loss hide to unmap the surface, no other input could ever
/// keep the keyboard while the terminal stayed open). A stay-open
/// surface must share focus like a normal window instead, which is
/// `OnDemand`: the compositor's map-time grant fires for any
/// non-`None` mode on Top/Overlay (so the measuring pass's
/// empty-desktop grab is unaffected), a click away leaves focus where
/// it lands, and a click on the terminal wins it back.
pub fn shown_keyboard_mode(mode: KeyboardMode, hide_on_focus_loss: bool) -> KeyboardMode {
    if hide_on_focus_loss {
        mode
    } else {
        KeyboardMode::OnDemand
    }
}

/// The surface lifecycle's decision core: the state (handle-carrying,
/// so no toolkit id crosses into decisions), the live slide, the
/// focus latch, the measured output, and the toggle debounce — plain
/// data over plain types, headless-constructible. The decision
/// methods return intents; the frontend's thin glue applies them
/// through its executor and keeps the app-side halves (tabs, drawer,
/// rename) beside them.
#[derive(Debug, Default)]
pub struct SurfaceMachine {
    /// The lifecycle state. `Default::default` is `Hidden` — the
    /// resting state every run and every test starts from.
    pub state: SurfaceState,
    /// The live slide, if content is animating (`Out` while `Hiding`,
    /// `In` riding a `Shown` surface).
    pub slide: Option<slide::Playing>,
    /// The keyboard-focus latch: set by the `LayerFocused` a keyboard
    /// hold synthesizes, guarding the focus-loss hide so a stray
    /// unfocus before any focus cannot close a freshly-opened surface.
    pub focused: bool,
    /// The output size measured by the last measuring pass, kept so
    /// every later reshape recomputes pixels without a new pass.
    pub output_size: Option<(u32, u32)>,
    /// The last accepted toggle, for the debounce.
    last_toggle: Option<Instant>,
    /// An in-progress grab-bar height drag, if any: the absolute
    /// anchor every target recomputes from, plus the apply cadence
    /// (see [`Drag`]). Cleared by every hide — a leaving surface's
    /// drag is moot.
    pub drag: Option<Drag>,
}

/// One live-drag decision's outcome: the intents (empty when the
/// cadence gate throttled the move), the applied pixel target for
/// the pin's request declaration — the pin is terminal-side
/// state, so the frontend declares and offers it — and whether a
/// trailing apply timer should arm, to land the latest target
/// even if the pointer pauses inside the interval.
#[derive(Debug, Default)]
pub struct DragOutcome {
    pub intents: Vec<SurfaceIntent>,
    pub applied: Option<u32>,
    pub trailing_timer: bool,
}

impl SurfaceMachine {
    /// Whether a toggle intent escapes the debounce window; records
    /// the acceptance when it does. The debounce collapses true spam
    /// (~6–7 effective toggles per second at 150 ms, each a full
    /// reversal) — the state machine itself never drops or queues.
    pub fn toggle_accepted(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_toggle
            && now.duration_since(last) < TOGGLE_DEBOUNCE
        {
            return false;
        }
        self.last_toggle = Some(now);
        true
    }

    /// The show decision: only a hidden surface starts the measuring
    /// pass — a measuring or shown one is already on its way and
    /// leaves with nothing. The intent carries the keyboard mode
    /// already resolved for a shown surface (`shown_keyboard_mode`):
    /// the map-time focus grant is the one path that focuses the
    /// surface on an empty desktop, and the executor's translation
    /// heals a lost clipboard anchor strictly ahead of the probe.
    pub fn decide_show(&mut self, inputs: SurfaceInputs) -> Vec<SurfaceIntent> {
        if !matches!(self.state, SurfaceState::Hidden) {
            return Vec::new();
        }
        let handle = SurfaceHandle::next();
        self.state = SurfaceState::Measuring(handle);
        self.focused = false;
        vec![SurfaceIntent::Measure {
            handle,
            keyboard: shown_keyboard_mode(inputs.keyboard_mode, inputs.hide_on_focus_loss),
        }]
    }

    /// The hide decision — every path through `toggle_step`'s `Hide`
    /// column. A measuring surface has no reshaped content to
    /// animate and dies now; a shown one slides out from wherever the
    /// content stands (the live eased offset of an in-flight
    /// slide-in, or rest — nothing teleports; the in-flight distance
    /// is kept when one exists, because a config edit that arrived
    /// mid-slide updated the config but not the frozen geometry); a
    /// hiding or hidden one leaves with nothing (no restart, no
    /// double destroy).
    pub fn decide_hide(&mut self, inputs: SurfaceInputs) -> Vec<SurfaceIntent> {
        let intents = match toggle_step(
            ToggleIntent::Hide,
            &self.state,
            inputs.slide_animation_enabled,
        ) {
            ToggleStep::DestroyNow(handle) => {
                self.slide = None;
                self.state = SurfaceState::Hidden;
                vec![SurfaceIntent::Destroy { handle }]
            }
            ToggleStep::SlideOut(handle) => {
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
                    None => (0.0, self.slide_distance_px(inputs)),
                };
                self.state = SurfaceState::Hiding(handle);
                self.slide = Some(slide::Playing {
                    started: Instant::now(),
                    active: slide::Active {
                        direction: slide::Direction::Out,
                        from_offset_px,
                        distance_px,
                    },
                });
                // Keyboard interactivity releases at slide-out START,
                // not at destroy: the previously focused window regains
                // focus while the slide plays. The compositor answers
                // with an `Unfocused` layer event, which the
                // `Shown(handle)` guard in its handler already rejects —
                // the surface is `Hiding` by the time it arrives.
                vec![SurfaceIntent::Hold {
                    handle,
                    keyboard: KeyboardHold::Released,
                }]
            }
            // `Hide` never decides to show, and a hiding or hidden
            // surface leaves with nothing.
            ToggleStep::Measure | ToggleStep::ReverseShow(_) | ToggleStep::Leave => Vec::new(),
        };
        // Any live grab-bar drag is moot on every leaving path, and
        // the cleanup is idempotent on the leave-with-nothing one.
        self.drag = None;
        intents
    }

    /// Reverses a hiding slide back to shown: the live surface is
    /// reused — no new measuring pass, `output_size` is retained (the
    /// `geometry` module's guarantee) — the slide re-targets `In`
    /// from the current offset (`slide::reverse`: no teleport, a
    /// fresh fixed 130 ms), keyboard interactivity is re-held per the
    /// resolved mode ([`shown_keyboard_mode`]) with the terminal
    /// input focused, and the focus latch resets pending the
    /// `LayerFocused` the re-hold synthesizes. Only a `Hiding`
    /// machine reverses — any other state leaves with nothing, the
    /// same state-matched discipline as `decide_configure` — so a
    /// misrouted call can never re-hold a live surface's keyboard or
    /// park an out-slide on a `Shown` machine.
    pub fn decide_reverse_show(
        &mut self,
        handle: SurfaceHandle,
        inputs: SurfaceInputs,
    ) -> Vec<SurfaceIntent> {
        let SurfaceState::Hiding(_) = self.state else {
            return Vec::new();
        };
        self.state = SurfaceState::Shown(handle);
        self.focused = false;
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
                started: Instant::now(),
                active: slide::reverse(
                    &playing.active,
                    playing.started.elapsed().as_millis() as u64,
                ),
            };
            self.slide = Some(reversed);
        }
        // The mirror of hide's focus bookkeeping: none — hide reports
        // nothing, so nothing returns here; the `LayerFocused` this
        // re-hold synthesizes repeats the terminal's standing focus
        // harmlessly (`set_focused` reports only real changes). The
        // re-hold runs the resolved mode: a stay-open surface reversed
        // mid-slide comes back as `OnDemand`, which commits no grab of
        // its own — the window that regained focus at slide-out start
        // keeps the keyboard, and a click on the terminal wins it back
        // (the sharing rule of `shown_keyboard_mode`).
        vec![
            SurfaceIntent::Hold {
                handle,
                keyboard: KeyboardHold::from(shown_keyboard_mode(
                    inputs.keyboard_mode,
                    inputs.hide_on_focus_loss,
                )),
            },
            SurfaceIntent::Focus { handle },
        ]
    }

    /// The focus-loss decision: whether the terminal must report the
    /// unfocus (only a shown surface's — unfocus events trailing our
    /// own hide must not report, or a focus-reporting program repaints
    /// its prompt per toggle), and whether the focus-loss hide fires
    /// (the config gate plus the focus latch; the latch clears only
    /// when the hide fires — a loss that doesn't hide leaves it
    /// armed). The hide itself runs through the caller's full hide
    /// path; deciding here only reads the latch.
    pub fn decide_focus_loss(&mut self, inputs: SurfaceInputs) -> (bool, bool) {
        let report_unfocus = matches!(self.state, SurfaceState::Shown(_));
        let hide = focus_loss_hides(inputs.hide_on_focus_loss) && self.focused;
        if hide {
            self.focused = false;
        }
        (report_unfocus, hide)
    }

    /// The first-configure decision (`SurfaceOpened` on the
    /// measuring probe): adopts the measured output as the reshape
    /// base, moves to `Shown` on the same handle, arms the slide-in
    /// — gated by the slide toggle; the full surface height is both
    /// the from-offset and the distance, deterministic and known at
    /// open time, unlike the granted window height, which
    /// double-reports through `WindowResized` — and emits the show
    /// intents: the surface shown at the config geometry, riding the
    /// configured layer, the resolved keyboard hold, and the
    /// configured exclusive zone, with the terminal input focused.
    /// A foreign window's open event finds no measuring surface and
    /// leaves with nothing.
    pub fn decide_configure(
        &mut self,
        output: (u32, u32),
        inputs: SurfaceInputs,
    ) -> Vec<SurfaceIntent> {
        let SurfaceState::Measuring(handle) = self.state else {
            return Vec::new();
        };
        self.output_size = Some(output);
        self.state = SurfaceState::Shown(handle);
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        let distance_px =
            geometry::surface_height(output.1 as f32, inputs.geometry.height_percent) as f32;
        self.slide = slide::show_slide(inputs.slide_animation_enabled, distance_px).map(|active| {
            slide::Playing {
                started: Instant::now(),
                active,
            }
        });
        vec![
            SurfaceIntent::Show {
                handle,
                geometry: inputs.geometry,
                output,
                layer: inputs.layer,
                keyboard: shown_keyboard_mode(inputs.keyboard_mode, inputs.hide_on_focus_loss),
                zone: ExclusiveZone::from_panel_respect(inputs.respect_panel),
            },
            SurfaceIntent::Focus { handle },
        ]
    }

    /// The config-edit reshape decision: a shown surface at rest
    /// reshapes in place when the geometry actually changed; a drag
    /// armed mid-edit defers to the release settle (config reshapes
    /// and drag deltas never interleave `set_size` calls); a
    /// slide animating defers to the next show — geometry stays
    /// frozen while animating, and the in-memory config (updated by
    /// the caller first) applies on that show.
    pub fn decide_reshape(
        &mut self,
        old: Geometry,
        new: Geometry,
        drag_armed: bool,
    ) -> Vec<SurfaceIntent> {
        if config_edit_reshapes(
            geometry::geometry_changed(old, new),
            drag_armed,
            self.slide.is_some(),
        ) && let (SurfaceState::Shown(handle), Some(output)) = (&self.state, self.output_size)
        {
            let handle = *handle;
            return vec![SurfaceIntent::Shape {
                handle,
                geometry: new,
                output,
                height: ShapeHeight::Percent,
            }];
        }
        Vec::new()
    }

    /// The config-edit zone decision: a shown surface at rest rezones
    /// in place — the `respect_panel` edit applies immediately, one
    /// recreate with the surface's shape standing — when the exclusive
    /// zone actually changed. The same defer gates as the reshape: a
    /// drag armed mid-edit defers (the release settle is a shape
    /// apply; the in-memory config's zone lands on the next show), a
    /// slide animating defers the same way. Any state but `Shown`
    /// needs no intent: a measuring surface's pending configure reads
    /// the caller's fresh inputs, and a hidden or hiding one picks the
    /// zone up on its next show.
    pub fn decide_rezone(
        &mut self,
        old: ExclusiveZone,
        new: ExclusiveZone,
        drag_armed: bool,
    ) -> Vec<SurfaceIntent> {
        if config_edit_rezones(old != new, drag_armed, self.slide.is_some())
            && let SurfaceState::Shown(handle) = self.state
        {
            return vec![SurfaceIntent::Rezone { handle, zone: new }];
        }
        Vec::new()
    }

    /// The live drag: arm on the first move — anchored at the height
    /// the surface has right now — then retarget absolutely per move
    /// in pixels: one pixel of pointer travel is one pixel of
    /// surface, and the target recomputes from the height actually
    /// applied, so the moving window frame of a bottom-anchored
    /// surface cannot eat travel. The first move always applies —
    /// no grab latency — and later applies pass a deadbanded,
    /// interval-gated cadence: every apply reaches the app twice
    /// (the runtime double-reports each `set_size`) and reflows the
    /// terminal, so a fast drag cannot storm the pipeline; skipped
    /// targets land via the trailing apply, and the release settle
    /// is exact and immediate. The caller guarantees the
    /// `grab_bar_acts_on` guard held.
    pub fn drag_move(
        &mut self,
        pointer_y: f32,
        now: Instant,
        inputs: SurfaceInputs,
    ) -> DragOutcome {
        self.drag_decide(pointer_y, now, inputs)
    }

    /// The trailing apply of a throttled live drag: recompute from
    /// the latest reported pointer position and apply if the gate
    /// allows; a fire after the drag ended (or with nothing changed)
    /// does nothing — and never re-arms itself.
    pub fn drag_trailing(&mut self, now: Instant, inputs: SurfaceInputs) -> DragOutcome {
        let Some(drag) = self.drag.as_ref() else {
            return DragOutcome::default();
        };
        let mut outcome = self.drag_decide(drag.pointer_y, now, inputs);
        outcome.trailing_timer = false;
        outcome
    }

    /// The drag core both entry points share.
    fn drag_decide(&mut self, pointer_y: f32, now: Instant, inputs: SurfaceInputs) -> DragOutcome {
        let (SurfaceState::Shown(handle), Some(output)) = (&self.state, self.output_size) else {
            return DragOutcome::default();
        };
        let handle = *handle;
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        let anchored_px = geometry::surface_height(output.1 as f32, inputs.geometry.height_percent);
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
        let target =
            geometry::drag_target_px(inputs.geometry.edge, output.1, drag.anchor, pointer_y);
        if geometry::drag_apply_due(drag.applied, now, target) {
            if let Some(drag) = self.drag.as_mut() {
                drag.applied = Some((now, target));
                drag.anchor.applied_px = target;
            }
            DragOutcome {
                intents: vec![SurfaceIntent::Shape {
                    handle,
                    geometry: inputs.geometry,
                    output,
                    height: ShapeHeight::Pixels(target),
                }],
                applied: Some(target),
                trailing_timer: false,
            }
        } else {
            DragOutcome {
                intents: Vec::new(),
                applied: None,
                trailing_timer: true,
            }
        }
    }

    /// The drag release — the settle: the dragged pixels quantize to
    /// a whole percent once (the settled surface may snap by at most
    /// half a percent, the accepted tolerance, and then matches the
    /// persisted percent), targeting where the pointer released —
    /// the latest move, not the last target the cadence landed.
    /// Re-applying also flushes a config edit that arrived mid-drag
    /// and was deferred: width and alignment come from the config,
    /// the height from the drag — the hand outranks the file. The
    /// second return is the settled percent when it differs from the
    /// config's (write-only-if-changed: a drag settling back on the
    /// standing percent persists nothing).
    pub fn drag_release(
        &mut self,
        inputs: SurfaceInputs,
    ) -> (Vec<SurfaceIntent>, Geometry, Option<u8>) {
        let (drag, state, output) = (self.drag.take(), self.state, self.output_size);
        let (SurfaceState::Shown(handle), Some(output)) = (&state, output) else {
            // A drag that outlived its surface hands its settle back:
            // the take already cleared it, nothing applies.
            return (Vec::new(), inputs.geometry, None);
        };
        let handle = *handle;
        let Some(drag) = drag else {
            return (Vec::new(), inputs.geometry, None);
        };
        let final_target =
            geometry::drag_target_px(inputs.geometry.edge, output.1, drag.anchor, drag.pointer_y);
        let mut geom = inputs.geometry;
        geom.height_percent = geometry::percent_from_px(output.1, final_target);
        let write =
            (geom.height_percent != inputs.geometry.height_percent).then_some(geom.height_percent);
        (
            vec![SurfaceIntent::Shape {
                handle,
                geometry: geom,
                output,
                height: ShapeHeight::Percent,
            }],
            geom,
            write,
        )
    }

    /// The slide clock's parking decision: the eased offset is
    /// evaluated at draw time inside the slide wrapper, so the tick's
    /// only job is parking a finished slide (which drops the tick
    /// subscription until the next one). An `Out` slide's completion
    /// is the only path that destroys a hiding surface — the
    /// destroy the state machine reaches through its own completion
    /// event. Teardown note: app teardown has no graceful path —
    /// process exit destroys the surface compositor-side, inherently
    /// immediate; any future quit action must bypass the slide and
    /// destroy straight away, like this does.
    pub fn decide_slide_tick(&mut self) -> Vec<SurfaceIntent> {
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
            if let SurfaceState::Hiding(handle) = self.state {
                self.state = SurfaceState::Hidden;
                return vec![SurfaceIntent::Destroy { handle }];
            }
        }
        Vec::new()
    }

    /// The slide distance for the currently shown surface: the same
    /// deterministic `geometry::surface_height` formula `SurfaceOpened`
    /// used — deliberately not the granted window height, which
    /// double-reports through `WindowResized`. A shown surface always
    /// has a measured output beside it; the `0.0` fallback covers a
    /// state that cannot occur and still hides (a degenerate at-rest
    /// out-slide that parks, then destroys).
    fn slide_distance_px(&self, inputs: SurfaceInputs) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "output px are < 2^24, exact in f32"
        )]
        self.output_size.map_or(0.0, |output| {
            geometry::surface_height(output.1 as f32, inputs.geometry.height_percent) as f32
        })
    }
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
#[derive(Debug)]
pub struct Drag {
    pub anchor: geometry::DragAnchor,
    /// The pointer's window y as of the last `GrabBarDragged`, so the
    /// trailing apply and the release settle at where the pointer
    /// actually is, not where the last apply landed.
    pub pointer_y: f32,
    /// The last live apply, for the cadence gate.
    pub applied: Option<(std::time::Instant, u32)>,
}

/// Whether a `ConfigChanged` may reshape a shown surface in place:
/// pure so the guard is headless-testable. The geometry must actually
/// have changed; a drag armed mid-edit defers the reshape to the
/// release settle (so config reshapes and drag deltas never interleave
/// `set_size` calls); a slide animating defers it to the next show —
/// geometry stays frozen while animating. The in-memory config still
/// updates on every path (the caller does that first); only the live
/// reshape is gated.
pub fn config_edit_reshapes(
    geometry_changed: bool,
    drag_armed: bool,
    slide_animating: bool,
) -> bool {
    geometry_changed && !drag_armed && !slide_animating
}

/// Whether a `ConfigChanged` may rezone a shown surface in place:
/// pure so the guard is headless-testable, the zone twin of
/// [`config_edit_reshapes`]. The zone must actually have changed; the
/// same defer gates apply — a drag armed mid-edit defers the live
/// swap (the release settle runs a shape apply, and the in-memory
/// config's zone lands on the next show), a slide animating defers
/// the same way.
pub fn config_edit_rezones(zone_changed: bool, drag_armed: bool, slide_animating: bool) -> bool {
    zone_changed && !drag_armed && !slide_animating
}

/// What a toggle-intent request must do with the surface: pure so the
/// decision is headless-testable. Every toggle caller goes through
/// here — the toggle hotkey and D-Bus activation (`debounced_toggle`),
/// plus `Ctrl+Shift+Q` and focus loss through the hide column — so
/// the full state × intent matrix lives in one place and
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
pub enum ToggleStep {
    /// Run the measuring show pass (a hidden surface only): measure
    /// → reshape → shown, then slide in.
    Measure,
    /// Destroy immediately: a measuring surface has no reshaped
    /// content to animate, so there is nothing to slide.
    DestroyNow(SurfaceHandle),
    /// Slide out from the current offset and destroy on the slide's
    /// completion tick. Keyboard interactivity releases at slide-out
    /// start.
    SlideOut(SurfaceHandle),
    /// Reverse a hiding slide back to shown on the same live surface:
    /// no new measuring pass — `output_size` is retained — the slide
    /// re-targets `In` from the current offset, keyboard
    /// interactivity is re-grabbed per config, and `surface_focused`
    /// resets pending the next `LayerFocused`.
    ReverseShow(SurfaceHandle),
    /// Nothing: the intent is already satisfied — a hide while
    /// hiding or hidden (no restart, no double destroy), a show
    /// while measuring or shown.
    Leave,
}

pub fn toggle_step(
    action: ToggleIntent,
    surface: &SurfaceState,
    slide_enabled: bool,
) -> ToggleStep {
    match (action, surface) {
        // Show intents: only a hidden surface starts the measuring
        // pass; a hiding one reverses; a measuring or shown one is
        // already on its way. None of these arms key on the slide
        // toggle — a hiding surface always reverses on the live
        // slide, whatever the flag says now (never aborted).
        (ToggleIntent::Show | ToggleIntent::Toggle, SurfaceState::Hidden) => ToggleStep::Measure,
        (ToggleIntent::Show | ToggleIntent::Toggle, SurfaceState::Hiding(id)) => {
            ToggleStep::ReverseShow(*id)
        }
        (ToggleIntent::Show, SurfaceState::Measuring(_) | SurfaceState::Shown(_)) => {
            ToggleStep::Leave
        }
        // Hide intents — and a toggle on any visible surface: a
        // measuring surface dies now, a hiding or hidden one stays
        // put (idempotent — no restart, no double destroy). A shown
        // one slides out (mid-slide-in the executor starts from the
        // live eased offset) — or, with the slide toggle off,
        // destroys now: instant hide.
        (ToggleIntent::Hide | ToggleIntent::Toggle, SurfaceState::Measuring(id)) => {
            ToggleStep::DestroyNow(*id)
        }
        (ToggleIntent::Hide | ToggleIntent::Toggle, SurfaceState::Shown(id)) => {
            if slide_enabled {
                ToggleStep::SlideOut(*id)
            } else {
                ToggleStep::DestroyNow(*id)
            }
        }
        (ToggleIntent::Hide, SurfaceState::Hiding(_) | SurfaceState::Hidden) => ToggleStep::Leave,
        // No remote-verb arm: the funnel's vocabulary carries only
        // the surface verbs, so this match is total — the frontend
        // applies its richer verbs (new-tab, close-tab, ...) before
        // the funnel, loud rather than silently wrong if one ever
        // leaks through its own match.
    }
}

/// Whether a `LayerUnfocused` event for `id` should reach the
/// frontend's `focus_lost`: only a shown surface reacts to losing
/// keyboard focus — one arriving while `Hiding` (the synthesized
/// unfocus of the hide's own keyboard release, or a racing focus
/// loss) is a no-op, so it can never restart or double-drive the
/// out-slide.
pub fn layer_unfocused_acts_on(surface: &SurfaceState, id: SurfaceHandle) -> bool {
    matches!(surface, SurfaceState::Shown(shown) if *shown == id)
}

/// Whether losing keyboard focus should hide the surface: exactly
/// the `hide_on_focus_loss` config flag. No settings-drawer input
/// is needed: focus loss may hide the surface with the drawer open —
/// the drawer and its drafts survive for the next show (spec
/// criterion 8) — so the gate runs the same in the drawer-open
/// state. Only the automatic focus-loss path consults it: explicit
/// hides (the toggle keybind, `Action::Hide`) call the hide decision
/// directly and ignore the gate entirely. Both unfocused routes —
/// the layer surface's `LayerUnfocused` and the `WindowUnfocused`
/// callback — route into the frontend's `focus_lost`, so one gate
/// covers both.
pub fn focus_loss_hides(hide_on_focus_loss: bool) -> bool {
    hide_on_focus_loss
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
/// armed drag, no drag can arm while a slide animates, and the hide
/// decision clears any live drag before any slide can start. A
/// throttled press is simply inert — the surface rests in at most
/// 130 ms and the next press drags normally.
pub fn grab_bar_acts_on(surface: &SurfaceState, slide_animating: bool) -> bool {
    matches!(surface, SurfaceState::Shown(_)) && !slide_animating
}

/// Whether a `WindowResized` event for `id` may feed the
/// terminal-height pin: the tracked layer surface's granted sizes
/// alone — never a `Measuring` probe's (the full-output stretch
/// the pass exists to *read*, whose sizes must never become the
/// terminal's — adopting them grows the grid once per toggle and
/// returns every grown line as scrollback rot on the shrink) and
/// never a foreign window's (the clipboard anchor). Pure, in the
/// same style as [`layer_unfocused_acts_on`].
pub fn resize_acts_on(surface: &SurfaceState, id: SurfaceHandle) -> bool {
    matches!(
        surface,
        SurfaceState::Shown(shown) | SurfaceState::Hiding(shown) if *shown == id
    )
}

#[cfg(test)]
mod tests;
