// SPDX-License-Identifier: GPL-3.0-only

//! The libcosmic executor (surface-intents spec): turns each surface
//! intent into today's exact layer-shell request sequence — the anchor
//! heal chained strictly ahead of the measuring probe at show, the
//! always-all-three reshape triple, the first-configure batch's order.
//!
//! The clipboard anchor lives here, private to this executor (the
//! spec's Out list): it is a libcosmic clipboard-binding workaround,
//! not surface vocabulary, so its lifecycle never appears in the
//! intent enum — the executor heals a lost anchor as part of
//! translating [`SurfaceIntent::Measure`], and the anchor-presence
//! swap runs through [`swap_anchor`] below.

use std::collections::HashMap;

use cosmic::app::Task;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::SctkLayerSurfaceSettings;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    KeyboardInteractivity, Layer as WlrLayer, destroy_layer_surface, get_layer_surface,
    set_exclusive_zone, set_keyboard_interactivity, set_layer,
};
use cosmic::iced::window;
use cosmic::widget;

use crate::app::{APP_ID, Message};
use crate::geometry::{self, Geometry};
use crate::intent::{
    ExclusiveZone, KeyboardHold, ShapeHeight, SurfaceExecutor, SurfaceHandle, SurfaceIntent,
};
use crate::surface;

/// Settings for the clipboard-anchor window — the **dock body**
/// (shell-presence spec, `show_in_dock = true`).
///
/// libcosmic binds its single runtime [`Clipboard`] to the first
/// window created, and when that surface is destroyed the binding is
/// dropped — taking the smithay-clipboard connection, and with it
/// any selection this app copied, down with it. `DropTerm`'s own
/// surface is the layer shell, which every hide destroys, so
/// copying out could never survive the focus switch to the paste
/// target. The anchor — never closed, created before any layer
/// surface — owns the binding for the whole process lifetime instead,
/// exempt from close requests, so nothing the user or the runtime
/// does can drop the connection mid-session.
///
/// Wayland cannot honor `visible: false` (winit's `set_visible` is a
/// no-op there), so this body is a mapped toplevel and the shell's
/// window switcher and dock list it — the presence `show_in_dock`
/// buys; the hidden body (`surface::anchor`) is a layer surface no
/// toplevel listing can see. Its `application_id` is the only name
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

/// Which clipboard-anchor body is live (shell-presence spec): the
/// mapped xdg toplevel that keeps a dock and switcher presence
/// (`show_in_dock = true`), or the never-destroyed 1×1 layer surface
/// that is invisible to both. Each variant carries its window id —
/// the presence swap destroys the old body and needs it.
///
/// `Lost` is the hidden body after the compositor closed it — the
/// one destroy `DropTerm` never asks for (its output went away, a
/// monitor sleeping off its HDMI link among the likely causes). The
/// runtime dropped the clipboard binding with it, and the next
/// surface created would take the binding instead and die with it on
/// the next hide, so a lost anchor reopens before the next surface
/// does ([`heal_anchor`] inside every `Measure` translation).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Anchor {
    Dock(window::Id),
    Hidden(window::Id),
    Lost,
}

/// Whether the live anchor body matches the configured presence: the
/// dock body belongs to `show_in_dock = true`, the layer body to
/// `false`, and a lost anchor to neither. Pure — the `ConfigChanged`
/// swap's gate, keyed on the live anchor rather than the old config
/// so a missed echo heals on the next one.
fn anchor_matches(anchor: &Anchor, show_in_dock: bool) -> bool {
    matches!(
        (anchor, show_in_dock),
        (Anchor::Dock(_), true) | (Anchor::Hidden(_), false)
    )
}

/// Records a layer surface's `Done` against the anchor: when `closed`
/// is the live hidden body, the compositor destroyed it and the
/// anchor becomes [`Anchor::Lost`]. Every other `Done` — terminal
/// hides, measuring probes, the old body of a presence flip (its id
/// already replaced) — leaves the anchor alone. Pure; returns whether
/// the anchor was lost.
pub(crate) fn anchor_closed(anchor: &mut Anchor, closed: window::Id) -> bool {
    if *anchor == Anchor::Hidden(closed) {
        *anchor = Anchor::Lost;
        return true;
    }
    false
}

/// Reopens a lost anchor as the configured body; a no-op for a live
/// one. Must run ahead of any other surface creation for the same
/// reason [`open_anchor`] runs first at startup: the runtime binds
/// its clipboard to the first window created while unbound.
pub(crate) fn heal_anchor(anchor: &mut Anchor, show_in_dock: bool) -> Task<Message> {
    if *anchor != Anchor::Lost {
        return Task::none();
    }
    let (next, open) = open_anchor(show_in_dock);
    *anchor = next;
    open
}

/// Opens the configured clipboard-anchor body and returns it with its
/// open task (shell-presence spec). The task must run before any
/// terminal layer surface is created: the runtime binds its
/// clipboard to the first window created — either body qualifies —
/// and the anchor exists to win that race and hold the binding for
/// the process lifetime.
pub(crate) fn open_anchor(show_in_dock: bool) -> (Anchor, Task<Message>) {
    if show_in_dock {
        let (id, open) = window::open(clipboard_anchor_settings());
        (Anchor::Dock(id), open.discard())
    } else {
        let id = window::Id::unique();
        (Anchor::Hidden(id), get_layer_surface(surface::anchor(id)))
    }
}

/// Swaps the clipboard anchor to the configured body (shell-presence
/// spec): destroys the live body first — the runtime drops the
/// clipboard binding with it, so whatever is on the clipboard dies
/// once, the documented cost of a live flip — then opens the other,
/// whose creation event rebinds the clipboard immediately (the
/// destroy-before-create order is what heals it without waiting for
/// the next surface). A no-op when the live body already matches.
pub(crate) fn swap_anchor(anchor: &mut Anchor, show_in_dock: bool) -> Task<Message> {
    if anchor_matches(anchor, show_in_dock) {
        return Task::none();
    }
    let close_old: Task<Message> = match anchor {
        Anchor::Dock(id) => window::close(*id),
        Anchor::Hidden(id) => destroy_layer_surface(*id),
        Anchor::Lost => Task::none(),
    };
    let (next, open_new) = open_anchor(show_in_dock);
    *anchor = next;
    // `chain`, never `then`: these commands are effects that carry no
    // output, and `then` runs its continuation only on an output — a
    // `then`-chained open never fires (the flip-needs-restart bug).
    // `chain` streams the close's action strictly before the open's,
    // preserving the destroy-before-create order.
    close_old.chain(open_new)
}

/// One window-system request the libcosmic executor derives from an
/// intent. Deliberately protocol-shaped — translating *into*
/// libcosmic's request set is this module's entire job, so this
/// private vocabulary may mirror it; the intent layer above stays
/// declarative. The pin tests assert these sequences; converting
/// them into runtime tasks is the thin mechanical layer below.
enum Request {
    /// Reopen the lost clipboard-anchor body. Carries the built task:
    /// the body's window id is decided when the open is decided, not
    /// when the task runs.
    HealAnchor(Task<Message>),
    /// Create the measuring probe with these settings (the window id
    /// the executor just mapped for the intent's handle is
    /// `settings.id`).
    Probe(SctkLayerSurfaceSettings),
    /// The reshape triple — `set_anchor` + `set_size` + `set_margin`
    /// through `geometry`'s single path, always all three (zero
    /// margins included) so no transition can leave a stale anchor,
    /// margin, or size behind. Carries the declarative inputs; the
    /// anchor/margin derivation stays `geometry`'s, with its own
    /// tests.
    ShapeTriple {
        id: window::Id,
        geometry: Geometry,
        output: (u32, u32),
        height: u32,
    },
    SetLayer {
        id: window::Id,
        layer: WlrLayer,
    },
    SetKeyboard {
        id: window::Id,
        interactivity: KeyboardInteractivity,
    },
    SetExclusiveZone {
        id: window::Id,
        zone: i32,
    },
    /// Focus the terminal's text input (IME routing).
    FocusInput(widget::Id),
    Destroy {
        id: window::Id,
    },
}

impl Request {
    /// The runtime task this request is. The composition above
    /// (`chain` for a measure, `batch` for the rest) preserves each
    /// intent's request order; this layer only converts, one request
    /// to one task.
    fn into_task(self) -> Task<Message> {
        match self {
            Request::HealAnchor(task) => task,
            Request::Probe(settings) => get_layer_surface(settings),
            Request::ShapeTriple {
                id,
                geometry,
                output,
                height,
            } => geometry::apply_drag_geometry(id, geometry, output, height),
            Request::SetLayer { id, layer } => set_layer(id, layer),
            Request::SetKeyboard { id, interactivity } => {
                set_keyboard_interactivity(id, interactivity)
            }
            Request::SetExclusiveZone { id, zone } => set_exclusive_zone(id, zone),
            Request::FocusInput(id) => widget::text_input::focus(id),
            Request::Destroy { id } => destroy_layer_surface(id),
        }
    }
}

/// The libcosmic executor (see the module docs): maps surface handles
/// to iced window ids, owns the clipboard anchor, and translates each
/// intent into today's exact layer-shell request sequence.
pub(crate) struct LibcosmicExecutor {
    /// Live surface handles and their iced window ids: filled at
    /// `Measure`, emptied at `Destroy` — a handle never names a
    /// successor surface.
    ids: HashMap<SurfaceHandle, window::Id>,
    /// The clipboard-anchor body this executor owns (shell-presence
    /// spec; private to the libcosmic executor by the spec's Out
    /// list). Healed inside every `Measure` translation, swapped by
    /// the config echo through [`swap_anchor`].
    pub(crate) anchor: Anchor,
    /// The dock-presence config the anchor heals and swaps by.
    show_in_dock: bool,
    /// The terminal's text input (IME routing) a `Focus` intent
    /// targets.
    terminal_id: widget::Id,
}

impl LibcosmicExecutor {
    /// Builds the executor around the startup anchor ([`open_anchor`]
    /// decides the first body) and the terminal's input id.
    pub(crate) fn new(anchor: Anchor, show_in_dock: bool, terminal_id: widget::Id) -> Self {
        Self {
            ids: HashMap::new(),
            anchor,
            show_in_dock,
            terminal_id,
        }
    }

    /// The window id a handle maps to. An intent always names a live
    /// surface — the decision layer allocates the handle at `Measure`
    /// and retires it at `Destroy` — so a miss is a decision-layer
    /// bug, loud here rather than a silently wrong-window command.
    fn window_id(&self, handle: SurfaceHandle) -> window::Id {
        *self
            .ids
            .get(&handle)
            .expect("an intent names a live surface")
    }

    /// The handle a window id belongs to, if any: the reverse mapping
    /// the event handlers need — runtime events arrive carrying
    /// window ids, decisions speak handles. A foreign window (the
    /// clipboard anchor, the settings window) maps to none.
    pub(crate) fn handle_of(&self, id: window::Id) -> Option<SurfaceHandle> {
        self.ids
            .iter()
            .find(|(_, window_id)| **window_id == id)
            .map(|(handle, _)| *handle)
    }

    /// Tracks the dock-presence config the anchor heals and swaps by
    /// (a `show_in_dock` flip changes every future heal, not just the
    /// live body).
    pub(crate) fn set_dock_presence(&mut self, show_in_dock: bool) {
        self.show_in_dock = show_in_dock;
    }

    /// Translates one intent into its ordered requests, mutating the
    /// executor's own state (handle map, anchor) exactly as today's
    /// decision paths do. Pure of runtime effects: the tasks are
    /// built by [`Request::into_task`] so this layer stays
    /// headless-pinnable.
    fn requests(&mut self, intent: SurfaceIntent) -> Vec<Request> {
        match intent {
            SurfaceIntent::Measure { handle, keyboard } => {
                let id = window::Id::unique();
                self.ids.insert(handle, id);
                let mut requests = Vec::with_capacity(2);
                // A lost anchor reopens strictly ahead of the probe
                // (`heal_anchor`'s own no-op gate, duplicated here so
                // the request list carries no empty heal): the runtime
                // binds its clipboard to the first surface created
                // while unbound, and the probe is a surface every show
                // destroys.
                if self.anchor == Anchor::Lost {
                    requests.push(Request::HealAnchor(heal_anchor(
                        &mut self.anchor,
                        self.show_in_dock,
                    )));
                }
                requests.push(Request::Probe(surface::measuring(id, keyboard)));
                requests
            }
            SurfaceIntent::Show {
                handle,
                geometry,
                output,
                layer,
                keyboard,
                zone,
            } => {
                let id = self.window_id(handle);
                let (_, height) = geometry::requested_size(geometry, output);
                vec![
                    Request::ShapeTriple {
                        id,
                        geometry,
                        output,
                        height,
                    },
                    Request::SetLayer {
                        id,
                        layer: surface::wlr_layer(layer),
                    },
                    Request::SetKeyboard {
                        id,
                        interactivity: surface::keyboard_interactivity(keyboard),
                    },
                    Request::SetExclusiveZone {
                        id,
                        zone: zone_request(zone),
                    },
                ]
            }
            SurfaceIntent::Shape {
                handle,
                geometry,
                output,
                height,
            } => {
                let id = self.window_id(handle);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "output px are < 2^24, exact in f32"
                )]
                let height = match height {
                    ShapeHeight::Percent => {
                        geometry::surface_height(output.1 as f32, geometry.height_percent)
                    }
                    ShapeHeight::Pixels(px) => px,
                };
                vec![Request::ShapeTriple {
                    id,
                    geometry,
                    output,
                    height,
                }]
            }
            SurfaceIntent::Hold { handle, keyboard } => {
                vec![Request::SetKeyboard {
                    id: self.window_id(handle),
                    interactivity: hold_interactivity(keyboard),
                }]
            }
            SurfaceIntent::Focus { .. } => vec![Request::FocusInput(self.terminal_id.clone())],
            SurfaceIntent::Destroy { handle } => {
                let id = self.window_id(handle);
                self.ids.remove(&handle);
                vec![Request::Destroy { id }]
            }
        }
    }
}

/// The interactivity a hold maps to: the two modes pass through, the
/// released hold asks for none.
fn hold_interactivity(hold: KeyboardHold) -> KeyboardInteractivity {
    match hold {
        KeyboardHold::Exclusive => KeyboardInteractivity::Exclusive,
        KeyboardHold::OnDemand => KeyboardInteractivity::OnDemand,
        KeyboardHold::Released => KeyboardInteractivity::None,
    }
}

/// The exclusive-zone request a placement maps to — the same values
/// `surface::exclusive_zone` encodes from the config flag (`-1`
/// covers reserved space, `0` arranges inside it).
fn zone_request(zone: ExclusiveZone) -> i32 {
    match zone {
        ExclusiveZone::CoversReserved => -1,
        ExclusiveZone::InsideReserved => 0,
    }
}

impl SurfaceExecutor for LibcosmicExecutor {
    type Effect = Task<Message>;

    fn execute(&mut self, intent: SurfaceIntent) -> Task<Message> {
        // Compose exactly as today's paths do: the measure pass
        // chains (the anchor heal must run strictly ahead of the
        // probe's creation), every other intent's requests batch.
        let chains = matches!(intent, SurfaceIntent::Measure { .. });
        let tasks: Vec<Task<Message>> = self
            .requests(intent)
            .into_iter()
            .map(Request::into_task)
            .collect();
        if chains {
            tasks
                .into_iter()
                .reduce(Task::chain)
                .unwrap_or_else(Task::none)
        } else {
            Task::batch(tasks)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Edge, HAlign, KeyboardMode, Layer};
    use cosmic::iced::platform_specific::shell::commands::layer_surface::Anchor as ShellAnchor;

    // --- the clipboard anchor ---
    //
    // libcosmic binds its single runtime Clipboard to the first
    // window created and drops that binding — and with it the
    // smithay-clipboard connection holding any copied selection —
    // when the owning surface is destroyed. DropTerm's own surface
    // is the layer shell, which every hide destroys, so a copy could
    // never survive the focus switch to wherever it would be pasted.
    // The anchor is a window created first and never closed, so it
    // owns the binding for the process lifetime. It has two bodies
    // (shell-presence spec): an invisible never-closed normal window
    // while `show_in_dock` is on — the mapped toplevel the dock and
    // switcher list — and a never-destroyed 1×1 layer surface while
    // it is off, which no toplevel listing can see at all.

    #[cfg(target_os = "linux")]
    #[test]
    fn clipboard_anchor_carries_the_app_id() {
        // The dock body is the app's only mapped xdg toplevel — Wayland
        // cannot unmap it — so the shell's window switcher names it by
        // this application_id alone. With the empty default the switcher
        // matched no desktop file and labeled the window "unknown"; the
        // id must stay the app id so it resolves to DropTerm's entry.
        assert_eq!(
            clipboard_anchor_settings().platform_specific.application_id,
            APP_ID
        );
    }

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

    #[test]
    fn the_hidden_anchor_body_matches_show_in_dock_false() {
        // The swap's gate: the layer body is the hidden-mode body, and
        // it never matches a dock-presence config (a flip must swap).
        assert!(anchor_matches(&Anchor::Hidden(window::Id::unique()), false));
        assert!(!anchor_matches(&Anchor::Hidden(window::Id::unique()), true));
    }

    #[test]
    fn the_dock_anchor_body_matches_show_in_dock_true() {
        assert!(anchor_matches(&Anchor::Dock(window::Id::unique()), true));
        assert!(!anchor_matches(&Anchor::Dock(window::Id::unique()), false));
    }

    #[test]
    fn a_lost_anchor_matches_neither_presence() {
        // Lost is no body at all: a presence flip from it must open the
        // configured body rather than no-op.
        assert!(!anchor_matches(&Anchor::Lost, false));
        assert!(!anchor_matches(&Anchor::Lost, true));
    }

    #[test]
    fn the_compositor_closing_the_hidden_anchor_marks_it_lost() {
        // The reported bug: the compositor closed the layer anchor (its
        // output gone) and nothing noticed, so the clipboard re-bound to
        // the terminal surface and died on every hide from then on.
        let id = window::Id::unique();
        let mut anchor = Anchor::Hidden(id);
        assert!(anchor_closed(&mut anchor, id));
        assert_eq!(anchor, Anchor::Lost);
    }

    #[test]
    fn a_foreign_layer_close_leaves_the_anchor_alone() {
        // Every terminal hide and measuring probe ends in the same Done
        // event; only the anchor's own id counts. The old body's Done
        // after a presence flip carries the old id and is ignored too.
        let id = window::Id::unique();
        let mut anchor = Anchor::Hidden(id);
        assert!(!anchor_closed(&mut anchor, window::Id::unique()));
        assert_eq!(anchor, Anchor::Hidden(id));

        let dock = window::Id::unique();
        let mut anchor = Anchor::Dock(dock);
        assert!(!anchor_closed(&mut anchor, dock));
        assert_eq!(anchor, Anchor::Dock(dock));
    }

    #[test]
    fn healing_a_lost_anchor_opens_the_configured_body() {
        let mut anchor = Anchor::Lost;
        let _ = heal_anchor(&mut anchor, false);
        assert!(matches!(anchor, Anchor::Hidden(_)));

        let mut anchor = Anchor::Lost;
        let _ = heal_anchor(&mut anchor, true);
        assert!(matches!(anchor, Anchor::Dock(_)));
    }

    #[test]
    fn healing_a_live_anchor_is_a_no_op() {
        // The show path heals unconditionally; a live body keeps its id
        // (a second body would never win a binding the first still holds).
        let id = window::Id::unique();
        let mut anchor = Anchor::Hidden(id);
        let _ = heal_anchor(&mut anchor, false);
        assert_eq!(anchor, Anchor::Hidden(id));
    }

    #[test]
    fn a_presence_flip_from_a_lost_anchor_opens_the_new_body() {
        let mut anchor = Anchor::Lost;
        let _ = swap_anchor(&mut anchor, true);
        assert!(matches!(anchor, Anchor::Dock(_)));
    }

    /// A geometry of Top edge, half height, full width, centered: on
    /// the 1920×1080 test output that pins the triple to TOP|LEFT|RIGHT
    /// anchors, fill width (`None`), 540 px, zero margins.
    fn plain_geometry() -> Geometry {
        Geometry {
            edge: Edge::Top,
            height_percent: 50,
            width_percent: 100,
            h_align: HAlign::Center,
        }
    }

    fn executor_with(anchor: Anchor) -> LibcosmicExecutor {
        LibcosmicExecutor::new(anchor, false, widget::Id::unique())
    }

    #[test]
    fn measure_with_a_lost_anchor_heals_strictly_ahead_of_the_probe() {
        // Constraint 3's headline: the anchor heal is chained ahead
        // of the measuring probe — the runtime binds its clipboard to
        // the first surface created while unbound, and the probe is a
        // surface every show destroys, so a lost anchor must reopen
        // first or the binding dies with the next hide.
        let mut exec = executor_with(Anchor::Lost);
        let handle = SurfaceHandle::next();
        let requests = exec.requests(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        assert_eq!(requests.len(), 2, "a lost anchor heals, then probes");
        assert!(
            matches!(requests[0], Request::HealAnchor(_)),
            "the heal comes strictly first"
        );
        // The heal opened the hidden body (`show_in_dock = false`).
        assert!(matches!(exec.anchor, Anchor::Hidden(_)));
        let Request::Probe(settings) = &requests[1] else {
            panic!("the probe follows the heal");
        };
        // The probe is exactly today's measuring surface: full-output
        // anchors, DontCare zone, the app-id namespace, and the
        // resolved keyboard mode riding creation.
        assert_eq!(
            settings.keyboard_interactivity,
            KeyboardInteractivity::Exclusive
        );
        assert_eq!(
            settings.anchor,
            ShellAnchor::TOP | ShellAnchor::BOTTOM | ShellAnchor::LEFT | ShellAnchor::RIGHT
        );
        assert_eq!(settings.exclusive_zone, -1);
        assert_eq!(settings.namespace, surface::NAMESPACE);
    }

    #[test]
    fn measure_with_a_live_anchor_creates_the_probe_alone() {
        // A live anchor needs nothing — no heal request, the probe
        // alone, and the anchor state untouched.
        let live = Anchor::Hidden(window::Id::unique());
        let mut exec = executor_with(live.clone());
        let requests = exec.requests(SurfaceIntent::Measure {
            handle: SurfaceHandle::next(),
            keyboard: KeyboardMode::OnDemand,
        });
        assert_eq!(requests.len(), 1);
        assert!(matches!(requests[0], Request::Probe(_)));
        assert_eq!(exec.anchor, live);
    }

    #[test]
    fn measure_registers_the_handle_a_destroy_then_finds() {
        // The handle mapping: the probe's window id is what a later
        // intent on the same handle must command.
        let mut exec = executor_with(Anchor::Hidden(window::Id::unique()));
        let handle = SurfaceHandle::next();
        let Request::Probe(settings) = &exec
            .requests(SurfaceIntent::Measure {
                handle,
                keyboard: KeyboardMode::Exclusive,
            })
            .pop()
            .expect("the live-anchor measure is the probe alone")
        else {
            panic!("unreachable: the pattern above guards it");
        };
        let probe_id = settings.id;
        let requests = exec.requests(SurfaceIntent::Destroy { handle });
        assert!(matches!(
            requests.as_slice(),
            [Request::Destroy { id }] if *id == probe_id
        ));
    }

    #[test]
    fn show_translates_into_the_first_configure_batch_in_order() {
        // Today's `SurfaceOpened` batch, order included: the reshape
        // triple (anchor, size, margin), then layer, keyboard hold,
        // exclusive zone.
        let mut exec = executor_with(Anchor::Hidden(window::Id::unique()));
        let handle = SurfaceHandle::next();
        exec.requests(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        let requests = exec.requests(SurfaceIntent::Show {
            handle,
            geometry: plain_geometry(),
            output: (1920, 1080),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::CoversReserved,
        });
        assert!(matches!(
            requests.first(),
            Some(Request::ShapeTriple { geometry, height, .. })
                if geometry.height_percent == 50
                    && geometry.width_percent == 100
                    && *height == 540
        ));
        assert!(matches!(
            requests.get(1),
            Some(Request::SetLayer { layer, .. }) if *layer == WlrLayer::Overlay
        ));
        assert!(matches!(
            requests.get(2),
            Some(Request::SetKeyboard { interactivity, .. }) if *interactivity == KeyboardInteractivity::Exclusive
        ));
        assert!(matches!(
            requests.get(3),
            Some(Request::SetExclusiveZone { zone, .. }) if *zone == -1
        ));
        assert_eq!(
            requests.len(),
            4,
            "the configure batch is exactly these four"
        );

        // The other placement: arranging inside the reserved space
        // asks for zone 0.
        let requests = exec.requests(SurfaceIntent::Show {
            handle,
            geometry: plain_geometry(),
            output: (1920, 1080),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::InsideReserved,
        });
        assert!(matches!(
            requests.get(3),
            Some(Request::SetExclusiveZone { zone, .. }) if *zone == 0
        ));
    }

    #[test]
    fn shape_translates_percent_heights_through_the_triple() {
        // A settled reshape (config edit, drag release): the height
        // quantizes from the geometry's percent — 50% of 1080 is 540.
        let mut exec = executor_with(Anchor::Hidden(window::Id::unique()));
        let handle = SurfaceHandle::next();
        exec.requests(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        let requests = exec.requests(SurfaceIntent::Shape {
            handle,
            geometry: plain_geometry(),
            output: (1920, 1080),
            height: ShapeHeight::Percent,
        });
        assert!(matches!(
            requests.as_slice(),
            [Request::ShapeTriple { height, .. }] if *height == 540
        ));
    }

    #[test]
    fn shape_translates_live_drag_pixels_through_the_triple() {
        // A live drag apply: the height is the pointer's pixels,
        // whatever the percent would round to.
        let mut exec = executor_with(Anchor::Hidden(window::Id::unique()));
        let handle = SurfaceHandle::next();
        exec.requests(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        let requests = exec.requests(SurfaceIntent::Shape {
            handle,
            geometry: plain_geometry(),
            output: (1920, 1080),
            height: ShapeHeight::Pixels(617),
        });
        assert!(matches!(
            requests.as_slice(),
            [Request::ShapeTriple { height, .. }] if *height == 617
        ));
    }

    #[test]
    fn hold_translates_every_keyboard_hold() {
        // Released is the slide-out start; the two modes are the
        // reversal and the stay-open flip. One request, nothing else.
        let mut exec = executor_with(Anchor::Hidden(window::Id::unique()));
        let handle = SurfaceHandle::next();
        exec.requests(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        for (hold, interactivity) in [
            (KeyboardHold::Released, KeyboardInteractivity::None),
            (KeyboardHold::Exclusive, KeyboardInteractivity::Exclusive),
            (KeyboardHold::OnDemand, KeyboardInteractivity::OnDemand),
        ] {
            let requests = exec.requests(SurfaceIntent::Hold {
                handle,
                keyboard: hold,
            });
            assert!(matches!(
                requests.as_slice(),
                [Request::SetKeyboard { interactivity: want, .. }] if *want == interactivity
            ));
        }
    }

    #[test]
    fn focus_translates_to_the_terminal_input() {
        // The terminal's own text input (IME routing) — the id the
        // executor was built with, not a fresh one.
        let terminal_id = widget::Id::unique();
        let mut exec = LibcosmicExecutor::new(Anchor::Lost, false, terminal_id.clone());
        let requests = exec.requests(SurfaceIntent::Focus {
            handle: SurfaceHandle::next(),
        });
        assert!(matches!(
            requests.as_slice(),
            [Request::FocusInput(id)] if *id == terminal_id
        ));
    }

    #[test]
    fn destroy_translates_and_retires_the_handle() {
        // Destroy is a single request, and the handle leaves the map:
        // a surface lifecycle never reuses a handle (monotonic by
        // construction), so a retired handle has no successor to
        // mis-name.
        let mut exec = executor_with(Anchor::Hidden(window::Id::unique()));
        let handle = SurfaceHandle::next();
        exec.requests(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        let requests = exec.requests(SurfaceIntent::Destroy { handle });
        assert!(matches!(requests.as_slice(), [Request::Destroy { .. }]));
        assert!(exec.ids.is_empty());
    }
}
