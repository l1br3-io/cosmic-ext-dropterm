// SPDX-License-Identifier: GPL-3.0-only

//! The toolkit-neutral surface intent vocabulary (surface-intents
//! spec): what the surface lifecycle's decision layer wants the
//! drop-down surface to *be*, never how a toolkit makes it so.
//!
//! Every intent is declarative desired-state — the desired surface
//! (output choice, geometry, layer, keyboard hold, exclusive zone,
//! visibility) — and each executor reconciles desired against current
//! and chooses to mutate or recreate. A vocabulary mirroring
//! libcosmic's requests (`set_anchor`, `set_margin`, `set_layer`)
//! would be unimplementable on GPUI, where those are create-only;
//! these say what the surface should look like, and the libcosmic
//! executor derives today's exact request sequence from them. Intents
//! carry a [`SurfaceHandle`] that the executor maps to the toolkit's
//! window id, so iced's ids leave the decision layer entirely. The
//! trait lives inside the binary by design: no `dropterm-platform`
//! crate and nothing GPUI-shaped before the spike rules.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{KeyboardMode, Layer};
use crate::geometry::Geometry;

/// Toolkit-neutral identity of the drop-down surface: allocated by the
/// decision layer when a surface's lifecycle begins, threaded through
/// every intent about it, and mapped to the toolkit's window id by the
/// executor — the one indirection that keeps `window::Id` out of the
/// decision layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceHandle(u64);

impl SurfaceHandle {
    /// A handle never handed out before in this process: handles are
    /// monotonic and never reused, so a stale intent can never name a
    /// successor surface.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// The keyboard hold a surface desires: the two configured modes a
/// shown surface runs, plus the released state a slide-out starts
/// from — the previously focused window regains the keyboard while the
/// slide plays, so `Released` is a real desired state, not an absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardHold {
    /// `Exclusive`: only this surface takes keyboard input.
    Exclusive,
    /// `OnDemand`: the surface shares focus like a normal window.
    OnDemand,
    /// No keyboard hold at all.
    Released,
}

impl From<KeyboardMode> for KeyboardHold {
    fn from(mode: KeyboardMode) -> Self {
        match mode {
            KeyboardMode::Exclusive => Self::Exclusive,
            KeyboardMode::OnDemand => Self::OnDemand,
        }
    }
}

/// The exclusive-zone placement a shown surface desires: covering
/// other surfaces' reserved space (over the panel) or arranging inside
/// the usable area below panels and docks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusiveZone {
    /// Cover reserved space — over the panel (libcosmic's `−1`).
    CoversReserved,
    /// Arrange inside the usable area, below panels and docks (`0`).
    InsideReserved,
}

impl ExclusiveZone {
    /// The placement the `respect_panel` config asks for.
    pub fn from_panel_respect(respect_panel: bool) -> Self {
        if respect_panel {
            Self::InsideReserved
        } else {
            Self::CoversReserved
        }
    }
}

/// The height a [`SurfaceIntent::Shape`] desires: the geometry's
/// percent of the output (quantized — the settled language of config
/// and release), or live drag pixels (unquantized by design mid-drag;
/// percent quantization happens exactly once, at release).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeHeight {
    /// `geometry.height_percent` of the output height.
    Percent,
    /// This many pixels, whatever the percent would round to.
    Pixels(u32),
}

/// One declarative desired-state for the drop-down surface: the
/// vocabulary the decision layer emits and every executor translates
/// into its own window-system commands, request order included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceIntent {
    /// Desired: a surface measuring the active output — the show's
    /// first pass, stretched full-output to read the output's size,
    /// mapping with `keyboard` already resolved for a shown surface
    /// (the map-time focus grant is the one path that focuses the
    /// surface on an empty desktop).
    Measure {
        handle: SurfaceHandle,
        keyboard: KeyboardMode,
    },
    /// Desired: the measured surface shown — shaped at `geometry` on
    /// `output`, riding `layer`, holding `keyboard`, placed by `zone`.
    /// Today's libcosmic translation is the first-configure batch: the
    /// anchor/size/margin triple (always all three), then layer,
    /// keyboard hold, and exclusive zone.
    Show {
        handle: SurfaceHandle,
        geometry: Geometry,
        output: (u32, u32),
        layer: Layer,
        keyboard: KeyboardMode,
        zone: ExclusiveZone,
    },
    /// Desired: the shown surface reshaped in place at `geometry` on
    /// `output` — the config-edit reshape and every live drag apply.
    /// The libcosmic translation is the same triple
    /// [`SurfaceIntent::Show`] shapes with, height from [`ShapeHeight`].
    Shape {
        handle: SurfaceHandle,
        geometry: Geometry,
        output: (u32, u32),
        height: ShapeHeight,
    },
    /// Desired: keyboard routed into the terminal input — the input
    /// focus a shown surface runs with (first configure, a mid-slide
    /// reversal, a layer-focus event).
    Focus { handle: SurfaceHandle },
    /// Desired: the surface's keyboard hold is `keyboard` and nothing
    /// else changes — released at slide-out start, re-held at the
    /// resolved mode by a reversal, downgraded to `OnDemand` by a live
    /// stay-open flip.
    Hold {
        handle: SurfaceHandle,
        keyboard: KeyboardHold,
    },
    /// Desired: the surface gone now — destroyed without animation.
    /// The animated hide starts as a [`SurfaceIntent::Hold`] with
    /// [`KeyboardHold::Released`] and its completion lands here.
    Destroy { handle: SurfaceHandle },
}

/// Turns surface intents into one toolkit's window-system commands.
///
/// The decision layer emits intents; an executor reconciles each
/// against the surfaces it currently holds — mutating or recreating as
/// its toolkit allows — and yields that toolkit's continuation. The
/// libcosmic executor yields its runtime tasks; the recording
/// executor yields nothing and remembers the sequence.
pub trait SurfaceExecutor {
    /// What applying one intent yields in this toolkit.
    type Effect;

    /// Applies one intent, returning the toolkit's continuation.
    fn execute(&mut self, intent: SurfaceIntent) -> Self::Effect;
}

/// The headless executor: records every intent it executes, in order,
/// and does nothing else — the second implementation that justifies
/// the [`SurfaceExecutor`] trait, and the test harness the whole
/// surface lifecycle runs against.
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    intents: Vec<SurfaceIntent>,
}

impl RecordingExecutor {
    /// The executed intents, in execution order.
    pub fn recorded(&self) -> &[SurfaceIntent] {
        &self.intents
    }
}

impl SurfaceExecutor for RecordingExecutor {
    type Effect = ();

    fn execute(&mut self, intent: SurfaceIntent) {
        self.intents.push(intent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Edge, HAlign};

    #[test]
    fn a_show_then_hide_lifecycle_records_its_intent_sequence() {
        // The contract the decision layer will emit once it migrates
        // (Tasks 3–4), expressed through the recording executor: a
        // show measures, the first configure shows the surface and
        // routes keyboard into the terminal input, a hide with slides
        // releases the keyboard hold at slide-out start, and the
        // slide's completion destroys the surface. One handle threads
        // the whole cycle — the executor maps it to the toolkit's
        // window id, the decision layer never sees one.
        let mut exec = RecordingExecutor::default();
        let handle = SurfaceHandle::next();
        let geometry = Geometry {
            edge: Edge::Top,
            height_percent: 50,
            width_percent: 100,
            h_align: HAlign::Center,
        };
        exec.execute(SurfaceIntent::Measure {
            handle,
            keyboard: KeyboardMode::Exclusive,
        });
        exec.execute(SurfaceIntent::Show {
            handle,
            geometry,
            output: (1920, 1080),
            layer: Layer::Overlay,
            keyboard: KeyboardMode::Exclusive,
            zone: ExclusiveZone::CoversReserved,
        });
        exec.execute(SurfaceIntent::Focus { handle });
        exec.execute(SurfaceIntent::Hold {
            handle,
            keyboard: KeyboardHold::Released,
        });
        exec.execute(SurfaceIntent::Destroy { handle });
        assert_eq!(
            exec.recorded(),
            &[
                SurfaceIntent::Measure {
                    handle,
                    keyboard: KeyboardMode::Exclusive,
                },
                SurfaceIntent::Show {
                    handle,
                    geometry,
                    output: (1920, 1080),
                    layer: Layer::Overlay,
                    keyboard: KeyboardMode::Exclusive,
                    zone: ExclusiveZone::CoversReserved,
                },
                SurfaceIntent::Focus { handle },
                SurfaceIntent::Hold {
                    handle,
                    keyboard: KeyboardHold::Released,
                },
                SurfaceIntent::Destroy { handle },
            ]
        );
    }

    #[test]
    fn keyboard_holds_derive_from_the_config_modes() {
        // The configured modes are the holds a shown surface runs; the
        // released state has no config twin — it exists for slide-out
        // start alone.
        assert_eq!(
            KeyboardHold::from(KeyboardMode::Exclusive),
            KeyboardHold::Exclusive
        );
        assert_eq!(
            KeyboardHold::from(KeyboardMode::OnDemand),
            KeyboardHold::OnDemand
        );
    }

    #[test]
    fn exclusive_zone_derives_from_the_panel_flag() {
        // `respect_panel` on arranges inside the reserved space; off
        // covers it — the panel-overriding default.
        assert_eq!(
            ExclusiveZone::from_panel_respect(false),
            ExclusiveZone::CoversReserved
        );
        assert_eq!(
            ExclusiveZone::from_panel_respect(true),
            ExclusiveZone::InsideReserved
        );
    }
}
