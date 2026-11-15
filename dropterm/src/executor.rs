// SPDX-License-Identifier: GPL-3.0-only

//! The GPUI executor (gpui-surface spec): turns each surface intent
//! into GPUI window-system actions on the paths the spike chose —
//! the two-surface show (measure, destroy, create shaped), in-place
//! resize where GPUI allows it, recreate where it does not.
//!
//! The decision layer runs unchanged from [`crate::core`]: this
//! executor reconciles each intent against the surfaces it holds.
//! GPUI's layer surfaces fix anchor, margins, layer, zone, and
//! keyboard mode at creation (there is no runtime
//! `set_keyboard_interactivity` at this pin — the wayland backend
//! sends it exactly once, at surface creation), so every intent that
//! changes one of those recreates the window, and every intent that
//! changes only the size resizes in place. [`Action`] is deliberately
//! inspectable so the whole translation is headless-pinnable, the
//! same role the libcosmic executor's private `Request` list plays.

use std::collections::HashMap;
use std::fmt;

use gpui::layer_shell::{self, Anchor, KeyboardInteractivity, Layer as GpuiLayer};
use gpui::{
    AnyWindowHandle, Bounds, Pixels, Size, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, point, px,
};

use dropterm::core::config::{Edge, HAlign, KeyboardMode, Layer, MAX_WIDTH_PERCENT};
use dropterm::core::geometry::{self, Geometry, Margins};
use dropterm::core::intent::{
    ExclusiveZone, KeyboardHold, ShapeHeight, SurfaceExecutor, SurfaceHandle, SurfaceIntent,
};

/// One window-system action the executor derives from an intent: the
/// GPUI mirror of the libcosmic executor's request list. The shell
/// applies these against live windows; deriving them is pure, so the
/// tests pin the translation headlessly.
#[derive(Debug)]
pub(crate) enum Action {
    /// Open the full-output measuring window (the two-surface show's
    /// first pass): anchored to all four edges with no size, so the
    /// compositor grants the whole output and the granted bounds carry
    /// the output's real size — [`cx.displays`] is never read, dodging
    /// the integer-rounded display-metadata debt by construction.
    OpenMeasuring {
        handle: SurfaceHandle,
        options: WindowOptions,
    },
    /// Destroy the handle's live window and open a fresh one with
    /// `options`: the show's second pass (measuring surface becomes
    /// the shaped one), the recreate path for anchor/margin/alignment
    /// edits, and the only way a keyboard hold can change at this pin.
    SwapWindow {
        handle: SurfaceHandle,
        options: WindowOptions,
        reason: SwapReason,
    },
    /// Resize the handle's window in place: the 80 ms drag cadence and
    /// every height/width-only edit. `width: None` means fill (both
    /// horizontal anchors, compositor-controlled width) — the applier
    /// carries the live width across so a fill surface never asks for
    /// a degenerate zero width.
    Resize {
        handle: SurfaceHandle,
        width: Option<u32>,
        height: u32,
    },
    /// Destroy the handle's window now (hide completions, teardown).
    Remove { handle: SurfaceHandle },
    /// Route keyboard into the terminal input — the input focus a
    /// shown surface runs with. The terminal input itself arrives with
    /// the gpui-terminal slice; until then this applies as a logged
    /// stand-in so the intent's presence stays visible in the walks.
    FocusTerminal { handle: SurfaceHandle },
}

/// Why a [`Action::SwapWindow`] recreates the window — the evidence
/// rows name it, and the walks pin each reason's timing class.
#[derive(Debug)]
pub(crate) enum SwapReason {
    /// The two-surface show's second pass: the measuring surface
    /// becomes the shaped one.
    MeasuredShow,
    /// An anchor/margin/alignment edit: creation-fixed placement
    /// changed (open ruling 3's accepted redesign).
    AnchorEdit,
    /// A keyboard hold change: keyboard mode is create-time at this
    /// pin, so releasing or re-grabbing rides a recreate.
    KeyboardHold,
}

impl fmt::Display for SwapReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::MeasuredShow => "measured-show",
            Self::AnchorEdit => "anchor-edit",
            Self::KeyboardHold => "keyboard-hold",
        };
        f.write_str(name)
    }
}

/// The placement a GPUI layer surface fixes at creation: the anchor
/// set, the margins, and whether the width fills (both horizontal
/// anchors, compositor-controlled) or is an explicit pixel request.
/// [`PartialEq`] over this is the resize-vs-recreate reconcile: what
/// creation fixed and only size left to change means a resize can
/// carry the intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    pub anchor: Anchor,
    pub margins: Margins,
    pub fill: bool,
}

/// Compose the layer-shell anchor set and margins for a geometry —
/// the GPUI twin of the libcosmic frontend's `anchor_margins`, same
/// rules: full width keeps both horizontal anchors and zero margins;
/// narrower widths drop the anchor on the aligned side; `Center`
/// keeps both anchors and offsets with equal margins, the odd
/// leftover pixel going right so the total width stays exact.
pub(crate) fn anchor_margins(geom: Geometry, output: (u32, u32)) -> (Anchor, Margins) {
    let vertical = match geom.edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    };
    let (horizontal, margins) = if geom.width_percent >= MAX_WIDTH_PERCENT {
        (Anchor::LEFT | Anchor::RIGHT, Margins::default())
    } else {
        match geom.h_align {
            HAlign::Left => (Anchor::LEFT, Margins::default()),
            HAlign::Right => (Anchor::RIGHT, Margins::default()),
            HAlign::Center => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "output px are < 2^24, exact in f32"
                )]
                let width_px = geometry::surface_width(output.0 as f32, geom.width_percent);
                let slack = output.0.saturating_sub(width_px);
                let left = slack / 2;
                #[expect(
                    clippy::cast_possible_wrap,
                    reason = "center margins are half the output slack (< 2^31 px by any display standard)"
                )]
                (
                    Anchor::LEFT | Anchor::RIGHT,
                    Margins {
                        left: left as i32,
                        right: (slack - left) as i32,
                        ..Margins::default()
                    },
                )
            }
        }
    };
    (vertical | horizontal, margins)
}

/// The placement a geometry asks for on an output.
pub(crate) fn placement_of(geom: Geometry, output: (u32, u32)) -> Placement {
    let (anchor, margins) = anchor_margins(geom, output);
    Placement {
        anchor,
        margins,
        fill: geom.width_percent >= MAX_WIDTH_PERCENT,
    }
}

/// The pixel size a geometry asks for on an output: width `None` at
/// fill, height from the [`ShapeHeight`] language (percent quantized,
/// live drag pixels as-is).
#[expect(
    clippy::cast_precision_loss,
    reason = "output px are < 2^24, exact in f32"
)]
pub(crate) fn sized(
    geometry: Geometry,
    output: (u32, u32),
    height: ShapeHeight,
) -> (Option<u32>, u32) {
    let (width, _) = geometry::requested_size(geometry, output);
    let height = match height {
        ShapeHeight::Percent => geometry::surface_height(output.1 as f32, geometry.height_percent),
        ShapeHeight::Pixels(px) => px,
    };
    (width, height)
}

/// The measuring window's options: anchored to all four edges with no
/// size, riding Overlay like the libcosmic probe, keyboard already
/// resolved for a shown surface riding creation (the map-time focus
/// grant is the one path that focuses the surface on an empty
/// desktop).
pub(crate) fn measuring_options(keyboard: KeyboardMode) -> WindowOptions {
    WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: Size::new(px(0.), px(0.)), // both axes compositor-controlled
        })),
        app_id: Some(dropterm::core::APP_ID.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(layer_shell::LayerShellOptions {
            namespace: dropterm::core::APP_ID.to_string(),
            layer: GpuiLayer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: Some(px(-1.)),
            exclusive_edge: None,
            margin: Some((px(0.), px(0.), px(0.), px(0.))),
            keyboard_interactivity: gpui_keyboard(keyboard),
        }),
        ..Default::default()
    }
}

/// The shaped window's options: the geometry's placement and size,
/// the configured layer, the resolved keyboard mode, and the zone —
/// everything GPUI fixes at creation, in one place. `height` is the
/// [`ShapeHeight`] language so the show (percent) and later in-place
/// edits (pixels or percent) share one derivation.
#[expect(
    clippy::cast_precision_loss,
    reason = "px of a <= 2^24 output share: exact in f32"
)]
pub(crate) fn shaped_options(
    geometry: Geometry,
    output: (u32, u32),
    layer: Layer,
    keyboard: KeyboardInteractivity,
    zone: ExclusiveZone,
    height: ShapeHeight,
) -> WindowOptions {
    let (anchor, margins) = anchor_margins(geometry, output);
    let (width, height_px) = sized(geometry, output, height);
    WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: Size::new(px(width.map_or(0., |w| w as f32)), px(height_px as f32)),
        })),
        app_id: Some(dropterm::core::APP_ID.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(layer_shell::LayerShellOptions {
            namespace: dropterm::core::APP_ID.to_string(),
            layer: gpui_layer(layer),
            anchor,
            exclusive_zone: Some(gpui_zone(zone)),
            exclusive_edge: None,
            margin: Some((
                px(margins.top as f32),
                px(margins.right as f32),
                px(margins.bottom as f32),
                px(margins.left as f32),
            )),
            keyboard_interactivity: keyboard,
        }),
        ..Default::default()
    }
}

/// The core layer enum onto GPUI's (both name the wlr layers).
pub(crate) fn gpui_layer(layer: Layer) -> GpuiLayer {
    match layer {
        Layer::Overlay => GpuiLayer::Overlay,
        Layer::Top => GpuiLayer::Top,
    }
}

/// The exclusive-zone request a placement maps to: `−1` covers
/// reserved space (over the panel), `0` arranges inside it.
pub(crate) fn gpui_zone(zone: ExclusiveZone) -> Pixels {
    match zone {
        ExclusiveZone::CoversReserved => px(-1.),
        ExclusiveZone::InsideReserved => px(0.),
    }
}

/// The keyboard interactivity a config mode maps to at creation.
pub(crate) fn gpui_keyboard(mode: KeyboardMode) -> KeyboardInteractivity {
    match mode {
        KeyboardMode::Exclusive => KeyboardInteractivity::Exclusive,
        KeyboardMode::OnDemand => KeyboardInteractivity::OnDemand,
    }
}

/// The keyboard interactivity a hold maps to: the two modes pass
/// through, the released hold asks for none — the slide-out start.
pub(crate) fn hold_keyboard(hold: KeyboardHold) -> KeyboardInteractivity {
    match hold {
        KeyboardHold::Exclusive => KeyboardInteractivity::Exclusive,
        KeyboardHold::OnDemand => KeyboardInteractivity::OnDemand,
        KeyboardHold::Released => KeyboardInteractivity::None,
    }
}

/// What a [`SurfaceIntent::Shape`] resolves against a live surface:
/// an in-place resize when only the size changed, a recreate when
/// anything creation fixed moved (the planning ruling's split, pure
/// so the choice is unit-tested).
pub(crate) enum ShapePlan {
    ResizeInPlace { width: Option<u32>, height: u32 },
    // Boxed: `WindowOptions` dwarfs the resize arm, and the plan is
    // constructed once per shape intent, never in a hot loop.
    Recreate(Box<WindowOptions>),
}

/// The reconciled plan for a shape intent against the live surface's
/// recorded creation.
pub(crate) fn reconcile(
    live: &Creation,
    geometry: Geometry,
    output: (u32, u32),
    height: ShapeHeight,
) -> ShapePlan {
    // The split is placement-vs-size: everything a resize can carry
    // (both axes' pixels) leaves the creation-fixed anchor, margins,
    // and fill untouched; anything else is open ruling 3's recreate.
    if placement_of(live.geometry, live.output) == placement_of(geometry, output) {
        let (width, height) = sized(geometry, output, height);
        ShapePlan::ResizeInPlace { width, height }
    } else {
        ShapePlan::Recreate(Box::new(shaped_options(
            geometry,
            output,
            live.layer,
            live.keyboard,
            live.zone,
            height,
        )))
    }
}

/// Everything a live window was created with (and the last height
/// applied to it): the record the reconcile reads and the recreate
/// paths rebuild from. Recorded at derivation time, exactly like the
/// libcosmic executor records its window ids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Creation {
    pub geometry: Geometry,
    pub output: (u32, u32),
    /// The height in pixels the window last carried — the recreate
    /// paths' size floor (a hold recreate keeps the surface exactly as
    /// tall as it is).
    pub height_px: u32,
    pub layer: Layer,
    pub zone: ExclusiveZone,
    pub keyboard: KeyboardInteractivity,
}

/// One live surface: the handle's window, and — once the two-surface
/// show's second pass has derived it — what it was created with. A
/// measuring probe carries no creation: only intents that command a
/// shaped surface read it, and those cannot reach a probe (the
/// decision layer's states guard it).
#[derive(Debug, Clone, Copy)]
struct LiveWindow {
    window: AnyWindowHandle,
    creation: Option<Creation>,
}

/// The GPUI executor (see the module docs): maps surface handles to
/// GPUI window handles — adopted when the shell applies an open, so
/// derivation never touches a live window — and translates each
/// intent into ordered [`Action`]s.
#[derive(Debug, Default)]
pub(crate) struct GpuiExecutor {
    live: HashMap<SurfaceHandle, LiveWindow>,
}

impl GpuiExecutor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Registers the window an open action created under its handle —
    /// the apply half of [`Action::OpenMeasuring`] and
    /// [`Action::SwapWindow`]; a swap re-registers the successor
    /// window under the same handle.
    pub(crate) fn adopt(&mut self, handle: SurfaceHandle, window: AnyWindowHandle) {
        match self.live.get_mut(&handle) {
            // A swap's derivation already recorded the successor's
            // creation; adopting only re-points the window.
            Some(live) => live.window = window,
            None => {
                self.live.insert(
                    handle,
                    LiveWindow {
                        window,
                        creation: None,
                    },
                );
            }
        }
    }

    /// The window a handle commands. An intent always names a live
    /// surface — the decision layer allocates the handle at `Measure`
    /// and retires it at `Destroy` — so a miss is a decision-layer
    /// bug, loud here rather than a silently wrong-window command.
    pub(crate) fn window_of(&self, handle: SurfaceHandle) -> AnyWindowHandle {
        self.live
            .get(&handle)
            .expect("an intent names a live surface")
            .window
    }

    /// Retires a handle: the apply half of [`Action::Remove`], after
    /// the window is gone. A surface lifecycle never reuses a handle
    /// (monotonic by construction), so a retired handle has no
    /// successor to mis-name — every later intent naming it is loud.
    pub(crate) fn retire(&mut self, handle: SurfaceHandle) {
        self.live.remove(&handle);
    }

    /// The creation a handle's window was shaped by (the reconcile's
    /// live record); loud on a miss for the same reason as
    /// [`Self::window_of`].
    pub(crate) fn creation_of(&self, handle: SurfaceHandle) -> Creation {
        self.live
            .get(&handle)
            .and_then(|live| live.creation)
            .expect("an intent names a live surface")
    }
}

impl GpuiExecutor {
    /// Records (or updates) a handle's creation — the successor's
    /// record lands at derivation, so a later intent reconciles
    /// against what is being applied, never a stale window.
    fn record(&mut self, handle: SurfaceHandle, creation: Creation) {
        match self.live.get_mut(&handle) {
            Some(live) => live.creation = Some(creation),
            None => {
                self.live.insert(
                    handle,
                    LiveWindow {
                        window: self.window_of(handle),
                        creation: Some(creation),
                    },
                );
            }
        }
    }

    /// The show's derivation: the liveness assert (the probe window
    /// was adopted at its open), the successor's recorded creation,
    /// and the swap action whose options carry the configured shape.
    fn execute_show(
        &mut self,
        handle: SurfaceHandle,
        geometry: Geometry,
        output: (u32, u32),
        layer: Layer,
        keyboard: KeyboardMode,
        zone: ExclusiveZone,
    ) -> Vec<Action> {
        let _ = self.window_of(handle);
        let interactivity = gpui_keyboard(keyboard);
        let (_, height_px) = sized(geometry, output, ShapeHeight::Percent);
        self.record(
            handle,
            Creation {
                geometry,
                output,
                height_px,
                layer,
                zone,
                keyboard: interactivity,
            },
        );
        vec![Action::SwapWindow {
            handle,
            options: shaped_options(
                geometry,
                output,
                layer,
                interactivity,
                zone,
                ShapeHeight::Percent,
            ),
            reason: SwapReason::MeasuredShow,
        }]
    }

    /// The shape's derivation: the reconcile split decides resize vs
    /// recreate, and the successor's record lands either way.
    fn execute_shape(
        &mut self,
        handle: SurfaceHandle,
        geometry: Geometry,
        output: (u32, u32),
        height: ShapeHeight,
    ) -> Vec<Action> {
        let creation = self.creation_of(handle);
        match reconcile(&creation, geometry, output, height) {
            ShapePlan::ResizeInPlace {
                width,
                height: height_px,
            } => {
                let mut succeeded = creation;
                succeeded.geometry = geometry;
                succeeded.output = output;
                succeeded.height_px = height_px;
                self.record(handle, succeeded);
                vec![Action::Resize {
                    handle,
                    width,
                    height: height_px,
                }]
            }
            ShapePlan::Recreate(options) => {
                let (_, height_px) = sized(geometry, output, height);
                let mut succeeded = creation;
                succeeded.geometry = geometry;
                succeeded.output = output;
                succeeded.height_px = height_px;
                self.record(handle, succeeded);
                vec![Action::SwapWindow {
                    handle,
                    options: *options,
                    reason: SwapReason::AnchorEdit,
                }]
            }
        }
    }

    /// The hold's derivation: the live mode is a no-op, any change
    /// recreates at the new interactivity with the surface's shape
    /// standing (keyboard mode is create-time at this pin).
    fn execute_hold(&mut self, handle: SurfaceHandle, keyboard: KeyboardHold) -> Vec<Action> {
        let creation = self.creation_of(handle);
        let interactivity = hold_keyboard(keyboard);
        if interactivity == creation.keyboard {
            // The hold the surface already runs: nothing moves, no
            // recreate for its own sake.
            return Vec::new();
        }
        let mut succeeded = creation;
        succeeded.keyboard = interactivity;
        self.record(handle, succeeded);
        vec![Action::SwapWindow {
            handle,
            options: shaped_options(
                succeeded.geometry,
                succeeded.output,
                succeeded.layer,
                interactivity,
                succeeded.zone,
                ShapeHeight::Pixels(succeeded.height_px),
            ),
            reason: SwapReason::KeyboardHold,
        }]
    }
}

impl SurfaceExecutor for GpuiExecutor {
    type Effect = Vec<Action>;

    fn execute(&mut self, intent: SurfaceIntent) -> Vec<Action> {
        match intent {
            SurfaceIntent::Measure { handle, keyboard } => {
                vec![Action::OpenMeasuring {
                    handle,
                    options: measuring_options(keyboard),
                }]
            }
            SurfaceIntent::Show {
                handle,
                geometry,
                output,
                layer,
                keyboard,
                zone,
            } => self.execute_show(handle, geometry, output, layer, keyboard, zone),
            SurfaceIntent::Shape {
                handle,
                geometry,
                output,
                height,
            } => self.execute_shape(handle, geometry, output, height),
            SurfaceIntent::Hold { handle, keyboard } => self.execute_hold(handle, keyboard),
            SurfaceIntent::Focus { handle } => vec![Action::FocusTerminal { handle }],
            SurfaceIntent::Destroy { handle } => {
                // Liveness asserted here (a decision bug is loud);
                // retirement itself is apply-time — the applier still
                // needs the window to remove it, mirroring how
                // adoption is apply-time for opens.
                let _ = self.window_of(handle);
                vec![Action::Remove { handle }]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropterm::core::config::Edge;

    /// A headless window identity: GPUI's `WindowId` converts from a
    /// u64 and `WindowHandle::new` wraps one without a live window,
    /// so the handle-map rules test without a compositor.
    fn fake_window(id: u64) -> AnyWindowHandle {
        struct ProbeView;
        impl gpui::Render for ProbeView {
            fn render(
                &mut self,
                _window: &mut gpui::Window,
                _cx: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                gpui::div()
            }
        }
        gpui::WindowHandle::<ProbeView>::new(gpui::WindowId::from(id)).into()
    }

    /// The default-ish geometry: Top edge, full width, centered.
    fn plain_geometry() -> Geometry {
        Geometry {
            edge: Edge::Top,
            height_percent: 40,
            width_percent: 100,
            h_align: HAlign::Center,
        }
    }

    // --- the two-surface show's option derivations ---

    #[test]
    fn the_measuring_window_is_the_full_output_stretch() {
        // Never `cx.displays()`: anchored to all four edges with no
        // size, the compositor grants the whole output and the granted
        // bounds carry its real (fractional-safe) size. The probe
        // rides Overlay like the libcosmic one, zone -1, with the
        // shown surface's keyboard mode already resolved at creation.
        let options = measuring_options(KeyboardMode::Exclusive);
        let WindowKind::LayerShell(ls) = options.kind else {
            panic!("the measuring window is a layer surface");
        };
        assert_eq!(
            ls.anchor,
            Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT
        );
        assert_eq!(ls.layer, GpuiLayer::Overlay);
        assert_eq!(ls.exclusive_zone, Some(px(-1.)));
        assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::Exclusive);
        assert_eq!(ls.namespace, dropterm::core::APP_ID);
        let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
            panic!("the probe carries windowed bounds");
        };
        assert_eq!(bounds.size.width, px(0.));
        assert_eq!(bounds.size.height, px(0.));
    }

    #[test]
    fn the_measuring_keyboard_mode_rides_creation() {
        // `shown_keyboard_mode` resolves before `Measure` — the
        // map-time focus grant is the empty-desktop path — so the
        // probe must carry it at creation, OnDemand included.
        let WindowKind::LayerShell(ls) = measuring_options(KeyboardMode::OnDemand).kind else {
            panic!("the measuring window is a layer surface");
        };
        assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::OnDemand);
    }

    #[test]
    fn the_shaped_window_lands_the_configured_geometry() {
        // Top edge, full width, 40% of a 1280×720 output: TOP|LEFT|RIGHT
        // anchored, flush margins, fill width (0 = compositor-controlled
        // height-side is exact pixels), Overlay layer, zone -1, Exclusive.
        let options = shaped_options(
            plain_geometry(),
            (1280, 720),
            Layer::Overlay,
            KeyboardInteractivity::Exclusive,
            ExclusiveZone::CoversReserved,
            ShapeHeight::Percent,
        );
        let WindowKind::LayerShell(ls) = options.kind else {
            panic!("the shaped window is a layer surface");
        };
        assert_eq!(ls.anchor, Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        assert_eq!(
            ls.margin,
            Some((px(0.), px(0.), px(0.), px(0.))),
            "full width composes flush margins"
        );
        assert_eq!(ls.layer, GpuiLayer::Overlay);
        assert_eq!(ls.exclusive_zone, Some(px(-1.)));
        assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::Exclusive);
        let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
            panic!("the shaped window carries windowed bounds");
        };
        assert_eq!(bounds.size.width, px(0.), "fill width asks for none");
        assert_eq!(bounds.size.height, px(288.), "40% of 720");
    }

    #[test]
    fn the_shaped_window_narrows_by_alignment() {
        // Bottom edge, 50% width, left-aligned on 1920×1080: the right
        // anchor drops, the height is half the output, Top layer over
        // Overlay, inside the reserved space.
        let mut geometry = plain_geometry();
        geometry.edge = Edge::Bottom;
        geometry.width_percent = 50;
        geometry.h_align = HAlign::Left;
        geometry.height_percent = 50;
        let options = shaped_options(
            geometry,
            (1920, 1080),
            Layer::Top,
            KeyboardInteractivity::OnDemand,
            ExclusiveZone::InsideReserved,
            ShapeHeight::Percent,
        );
        let WindowKind::LayerShell(ls) = options.kind else {
            panic!("the shaped window is a layer surface");
        };
        assert_eq!(ls.anchor, Anchor::BOTTOM | Anchor::LEFT);
        assert_eq!(ls.layer, GpuiLayer::Top);
        assert_eq!(ls.exclusive_zone, Some(px(0.)));
        assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::OnDemand);
        let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
            panic!("the shaped window carries windowed bounds");
        };
        assert_eq!(bounds.size.width, px(960.), "50% of 1920");
        assert_eq!(bounds.size.height, px(540.), "50% of 1080");
    }

    #[test]
    fn a_centered_narrow_width_composes_equal_margins() {
        // Center keeps both anchors and splits the slack, the odd
        // pixel right — the libcosmic rule verbatim: 60% of 1280 is
        // 768, slack 512, margins (256, 256); 61% is 781, slack 499,
        // margins (249, 250).
        let mut geometry = plain_geometry();
        geometry.width_percent = 60;
        let (_, margins) = anchor_margins(geometry, (1280, 720));
        assert_eq!(
            margins,
            Margins {
                left: 256,
                right: 256,
                ..Margins::default()
            }
        );
        geometry.width_percent = 61;
        let (_, margins) = anchor_margins(geometry, (1280, 720));
        assert_eq!(margins.left, 249);
        assert_eq!(margins.right, 250);
    }

    #[test]
    fn shape_height_pixels_override_the_percent() {
        // The drag language: live pixels land whatever the percent
        // would round to.
        let options = shaped_options(
            plain_geometry(),
            (1280, 720),
            Layer::Overlay,
            KeyboardInteractivity::Exclusive,
            ExclusiveZone::CoversReserved,
            ShapeHeight::Pixels(617),
        );
        let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
            panic!("the shaped window carries windowed bounds");
        };
        assert_eq!(bounds.size.height, px(617.));
    }

    // --- the reconcile split (the planning ruling) ---

    /// A creation record shaped by `geometry` on 1280×720.
    fn created_with(geometry: Geometry) -> Creation {
        Creation {
            geometry,
            output: (1280, 720),
            height_px: geometry::surface_height(720., geometry.height_percent),
            layer: Layer::Overlay,
            zone: ExclusiveZone::CoversReserved,
            keyboard: KeyboardInteractivity::Exclusive,
        }
    }

    #[test]
    fn a_height_only_edit_resizes_in_place() {
        // The config-edit reshape: same anchor, margins, and fill —
        // only the height percent moved — so the plan is a resize at
        // the newly quantized height, full width carried as fill.
        let live = created_with(plain_geometry());
        let mut edited = plain_geometry();
        edited.height_percent = 60;
        let ShapePlan::ResizeInPlace { width, height } =
            reconcile(&live, edited, (1280, 720), ShapeHeight::Percent)
        else {
            panic!("a height-only edit resizes in place");
        };
        assert_eq!(width, None, "full width stays fill");
        assert_eq!(height, 432, "60% of 720");
    }

    #[test]
    fn live_drag_pixels_resize_in_place() {
        // The drag cadence's language lands the same way: the
        // placement did not move, only the height's pixels.
        let live = created_with(plain_geometry());
        let ShapePlan::ResizeInPlace { width, height } = reconcile(
            &live,
            plain_geometry(),
            (1280, 720),
            ShapeHeight::Pixels(500),
        ) else {
            panic!("a drag resize resizes in place");
        };
        assert_eq!(width, None);
        assert_eq!(height, 500);
    }

    #[test]
    fn a_left_aligned_width_edit_resizes_in_place() {
        // Left-anchored 80% -> 60%: the anchor set, margins, and fill
        // are unchanged (both are explicit-width LEFT surfaces), so
        // the resize carries the new pixel width — check 2's cadence
        // class, width axis included.
        let mut geometry = plain_geometry();
        geometry.width_percent = 80;
        geometry.h_align = HAlign::Left;
        let live = created_with(geometry);
        let mut edited = geometry;
        edited.width_percent = 60;
        let ShapePlan::ResizeInPlace { width, height } =
            reconcile(&live, edited, (1280, 720), ShapeHeight::Percent)
        else {
            panic!("a same-anchored width edit resizes in place");
        };
        assert_eq!(width, Some(768), "60% of 1280");
        assert_eq!(height, 288);
    }

    #[test]
    fn an_edge_edit_recreates() {
        // Anchor is creation-fixed: Top -> Bottom cannot ride a
        // resize (open ruling 3's accepted redesign, one extra show).
        let live = created_with(plain_geometry());
        let mut edited = plain_geometry();
        edited.edge = Edge::Bottom;
        assert!(matches!(
            reconcile(&live, edited, (1280, 720), ShapeHeight::Percent),
            ShapePlan::Recreate(_)
        ));
    }

    #[test]
    fn an_alignment_edit_recreates() {
        // Center -> Left changes the anchor set (and drops margins);
        // Left -> Right changes it too. At full width the alignment is
        // moot — both keep L|R and flush margins — so the edit only
        // moves placement at a narrow width.
        let mut narrow = plain_geometry();
        narrow.width_percent = 80;
        let live = created_with(narrow);
        let mut edited = narrow;
        edited.h_align = HAlign::Left;
        assert!(matches!(
            reconcile(&live, edited, (1280, 720), ShapeHeight::Percent),
            ShapePlan::Recreate(_)
        ));
        let mut left = narrow;
        left.h_align = HAlign::Left;
        let live_left = created_with(left);
        let mut right = left;
        right.h_align = HAlign::Right;
        assert!(matches!(
            reconcile(&live_left, right, (1280, 720), ShapeHeight::Percent),
            ShapePlan::Recreate(_)
        ));
    }

    #[test]
    fn a_fill_flip_recreates() {
        // 100% -> 80% drops a horizontal anchor; 80% -> 100% adds one
        // back and zeroes the margins. Both move what creation fixed.
        let live = created_with(plain_geometry());
        let mut edited = plain_geometry();
        edited.width_percent = 80;
        edited.h_align = HAlign::Left;
        assert!(matches!(
            reconcile(&live, edited, (1280, 720), ShapeHeight::Percent),
            ShapePlan::Recreate(_)
        ));
        let live_narrow = created_with(edited);
        let mut widened = edited;
        widened.width_percent = 100;
        widened.h_align = HAlign::Center;
        assert!(matches!(
            reconcile(&live_narrow, widened, (1280, 720), ShapeHeight::Percent),
            ShapePlan::Recreate(_)
        ));
    }

    #[test]
    fn a_centered_width_edit_recreates() {
        // Center 60% -> 50% keeps the anchor set but moves the
        // margins — creation-fixed too, so the ruling's recreate arm
        // takes it (not a resize that would strand the old margins).
        let mut geometry = plain_geometry();
        geometry.width_percent = 60;
        let live = created_with(geometry);
        let mut edited = geometry;
        edited.width_percent = 50;
        assert!(matches!(
            reconcile(&live, edited, (1280, 720), ShapeHeight::Percent),
            ShapePlan::Recreate(_)
        ));
    }

    // --- the handle map ---

    #[test]
    fn measure_opens_the_measuring_window_then_show_swaps_it() {
        // The two-surface show through the executor's own eyes: the
        // measure pass opens (nothing to resolve yet), the shell
        // adopts the probe's window, and the show pass resolves that
        // window for removal while the shaped window's options carry
        // the configured geometry. Adopting the successor remaps the
        // same handle to the new window.
        let mut exec = GpuiExecutor::new();
        let handle = SurfaceHandle::next();
        let actions = exec.execute(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        assert!(matches!(
            actions.as_slice(),
            [Action::OpenMeasuring { handle: h, .. }] if *h == handle
        ));
        let probe = fake_window(1);
        exec.adopt(handle, probe);
        assert_eq!(exec.window_of(handle), probe);

        let actions = exec.execute(SurfaceIntent::Show {
            handle,
            geometry: plain_geometry(),
            output: (1280, 720),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::CoversReserved,
        });
        match actions.as_slice() {
            [
                Action::SwapWindow {
                    handle: h, reason, ..
                },
            ] => {
                assert_eq!(*h, handle);
                assert!(matches!(reason, SwapReason::MeasuredShow));
            }
            other => panic!("the show swaps the measured window: {other:?}"),
        }
        let shaped = fake_window(2);
        exec.adopt(handle, shaped);
        assert_eq!(exec.window_of(handle), shaped, "the swap remaps the handle");
        // The creation record reads back the shaped geometry.
        assert_eq!(exec.creation_of(handle).geometry, plain_geometry());
    }

    #[test]
    fn destroy_derives_the_remove_and_retirement_is_apply_time() {
        // Destroy's derivation asserts liveness and yields the remove;
        // the handle retires only when the applier has removed the
        // window (it still needs the map to find it). A later intent
        // naming the retired handle is loud.
        let mut exec = GpuiExecutor::new();
        let handle = SurfaceHandle::next();
        exec.execute(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        let probe = fake_window(7);
        exec.adopt(handle, probe);
        let actions = exec.execute(SurfaceIntent::Destroy { handle });
        assert!(matches!(
            actions.as_slice(),
            [Action::Remove { handle: h }] if *h == handle
        ));
        // Pre-retirement (the applier's window lookup): still live.
        assert_eq!(exec.window_of(handle), probe);
        exec.retire(handle);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { exec.window_of(handle) }))
                .is_err()
        );
    }

    #[test]
    #[should_panic(expected = "an intent names a live surface")]
    fn an_intent_naming_an_unadopted_handle_panics_loudly() {
        // The loud-failure contract, mirroring the libcosmic executor:
        // a Show derived before the measuring window was adopted is a
        // decision-layer bug — loud, never a silently wrong-window
        // command.
        let mut exec = GpuiExecutor::new();
        let handle = SurfaceHandle::next();
        exec.execute(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        let _ = exec.execute(SurfaceIntent::Show {
            handle,
            geometry: plain_geometry(),
            output: (1280, 720),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::CoversReserved,
        });
    }

    #[test]
    fn a_released_hold_recreates_at_no_keyboard() {
        // Keyboard mode is create-time at this pin: a released hold
        // (slide-out start) rides a recreate at `None`, and the
        // creation record follows — the reversal's re-hold recreates
        // right back at the resolved mode.
        let mut exec = GpuiExecutor::new();
        let handle = SurfaceHandle::next();
        exec.execute(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        exec.adopt(handle, fake_window(3));
        exec.execute(SurfaceIntent::Show {
            handle,
            geometry: plain_geometry(),
            output: (1280, 720),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::CoversReserved,
        });
        exec.adopt(handle, fake_window(4));
        let actions = exec.execute(SurfaceIntent::Hold {
            handle,
            keyboard: KeyboardHold::Released,
        });
        match actions.as_slice() {
            [
                Action::SwapWindow {
                    reason, options, ..
                },
            ] => {
                assert!(matches!(reason, SwapReason::KeyboardHold));
                let WindowKind::LayerShell(ls) = options.kind.clone() else {
                    panic!("the swap recreates a layer surface");
                };
                assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::None);
                // Everything else about the surface stands: same
                // anchor, same size as the live height.
                let WindowKind::LayerShell(live_ls) = shaped_options(
                    plain_geometry(),
                    (1280, 720),
                    Layer::Overlay,
                    KeyboardInteractivity::Exclusive,
                    ExclusiveZone::CoversReserved,
                    ShapeHeight::Percent,
                )
                .kind
                else {
                    panic!("unreachable: the pattern above guards it");
                };
                assert_eq!(ls.anchor, live_ls.anchor);
                assert_eq!(options.window_bounds, live_ls_options_bounds());
            }
            other => panic!("a hold change swaps the window: {other:?}"),
        }
        assert_eq!(
            exec.creation_of(handle).keyboard,
            KeyboardInteractivity::None
        );
        // The re-hold recreates back.
        let actions = exec.execute(SurfaceIntent::Hold {
            handle,
            keyboard: KeyboardHold::Exclusive,
        });
        assert!(matches!(actions.as_slice(), [Action::SwapWindow { .. }]));
        assert_eq!(
            exec.creation_of(handle).keyboard,
            KeyboardInteractivity::Exclusive
        );
    }

    #[test]
    fn a_hold_at_the_live_mode_is_a_no_op() {
        // Re-applying the hold the surface already runs changes
        // nothing: no recreate for its own sake (the ~139 ms swap cost
        // is only paid when the keyboard actually moves).
        let mut exec = GpuiExecutor::new();
        let handle = SurfaceHandle::next();
        exec.execute(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        exec.adopt(handle, fake_window(5));
        exec.execute(SurfaceIntent::Show {
            handle,
            geometry: plain_geometry(),
            output: (1280, 720),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::CoversReserved,
        });
        exec.adopt(handle, fake_window(6));
        let actions = exec.execute(SurfaceIntent::Hold {
            handle,
            keyboard: KeyboardHold::Exclusive,
        });
        assert!(actions.is_empty());
    }

    #[test]
    fn focus_yields_the_terminal_input_action() {
        // The intent's presence is structural now; the terminal input
        // it routes to arrives with gpui-terminal.
        let mut exec = GpuiExecutor::new();
        let actions = exec.execute(SurfaceIntent::Focus {
            handle: SurfaceHandle::next(),
        });
        assert!(matches!(actions.as_slice(), [Action::FocusTerminal { .. }]));
    }

    /// The window bounds of the same geometry shaped at Exclusive —
    /// the hold recreate must keep the surface's size exactly.
    fn live_ls_options_bounds() -> Option<WindowBounds> {
        shaped_options(
            plain_geometry(),
            (1280, 720),
            Layer::Overlay,
            KeyboardInteractivity::Exclusive,
            ExclusiveZone::CoversReserved,
            ShapeHeight::Percent,
        )
        .window_bounds
    }
}
