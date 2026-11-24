// SPDX-License-Identifier: GPL-3.0-only

//! The host-owned panel containers (panel-containers spec): named
//! regions the shell declares and positions — the fixed-workbench
//! moment. Two kinds: an edge rail per side (the dormant seam
//! `panel-slot` docks into, config-gated off until a tenant lands)
//! and overlay panels (the images panel's genus). Pure state: the
//! shell owns the transitions, this module pins them headless. No
//! plugin surface lives here — `panel-slot` registers views into
//! these once the containers stand.

/// The edge an edge-rail container rides.
// PC1 ships the model whole, unwired (the chunk's own task law);
// PC2's re-parent and PC3's rail consume these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailEdge {
    Right,
}

/// The rail's width floor — a rail the user cannot shrink into
/// nothing.
pub const MIN_RAIL_WIDTH: u32 = 160;

/// The rail's declared width — the shared per-edge memory's birth
/// value (panel-containers PC3); nothing resizes it until a tenant
/// needs to.
pub const DEFAULT_RAIL_WIDTH: u32 = 280;

/// The open state's focus-return claim: an opaque token the shell
/// maps to the real handle that held the keyboard at open. The
/// model carries it back at close — the shell hands the keyboard
/// there (the modal repairs' own shape, one abstraction up).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusClaim(pub String);

/// One edge-rail container: declared, sized by the shared per-edge
/// memory (the stood default — one memory per edge, not per
/// consumer), dormant until `panel-slot` fills it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRail {
    pub edge: RailEdge,
    /// The rail's width in px — the edge's size memory, surviving
    /// open/close whole.
    pub width: u32,
}

/// One overlay panel's state (the images panel's genus): the open
/// door, the armed delete row (the single-arm seat — an armed pair
/// never survives a close), and the focus claim taken at open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverlayPanel {
    pub open: bool,
    pub armed: Option<std::path::PathBuf>,
    pub focus_claim: Option<FocusClaim>,
}

/// A host-owned panel container. Declared by the shell, never by a
/// script; `panel-slot` registers views into these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelContainer {
    Edge(EdgeRail),
    Overlay(OverlayPanel),
}

impl PanelContainer {
    /// The declared right rail at a width — PC3's dormant container,
    /// the shell's one construction of the Edge kind.
    pub fn right_rail(width: u32) -> Self {
        PanelContainer::Edge(EdgeRail {
            edge: RailEdge::Right,
            width: width.max(MIN_RAIL_WIDTH),
        })
    }

    /// The overlay's armed delete row, read-only (the render's
    /// cluster choice). `None` for a rail — no delete seat there.
    pub fn armed(&self) -> Option<&std::path::Path> {
        match self {
            PanelContainer::Overlay(overlay) => overlay.armed.as_deref(),
            PanelContainer::Edge(_) => None,
        }
    }

    /// The container's open door: an overlay that takes the keyboard
    /// records where it sat (`Some`); the images panel's genus never
    /// does (`None` — the pane keeps the keyboard under its veil). An
    /// edge rail's door is the config gate, never this transition.
    pub fn open(&mut self, claim: Option<FocusClaim>) {
        if let PanelContainer::Overlay(overlay) = self
            && !overlay.open
        {
            overlay.open = true;
            overlay.focus_claim = claim;
        }
    }

    /// The container's close door: the open drops, the armed pair
    /// dies with it (an armed pair never survives the close), and
    /// the focus claim comes back for the shell's handback. Closing
    /// a closed container returns nothing.
    pub fn close(&mut self) -> Option<FocusClaim> {
        match self {
            PanelContainer::Overlay(overlay) => {
                if !overlay.open {
                    return None;
                }
                overlay.open = false;
                overlay.armed = None;
                overlay.focus_claim.take()
            }
            PanelContainer::Edge(_) => None,
        }
    }

    /// Whether the container stands open (an overlay's door; an edge
    /// rail stands by its gate — never this model's).
    pub fn is_open(&self) -> bool {
        match self {
            PanelContainer::Overlay(overlay) => overlay.open,
            PanelContainer::Edge(_) => false,
        }
    }
}

impl EdgeRail {
    /// Resize the rail: the edge's memory, clamped to the floor.
    /// The memory survives open/close whole — there is no close for
    /// a rail, only the gate. Nothing calls it until a tenant needs
    /// a resize seam (panel-slot); the pin holds the clamp rule.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the resize seam waits for its tenant")
    )]
    pub fn resize(&mut self, width: u32) {
        self.width = width.max(MIN_RAIL_WIDTH);
    }

    /// The rail's width as the painter's px value — the u32 memory's
    /// one conversion, the cast's home.
    pub fn width_px(&self) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a px width from a u32 memory: f32's mantissa is exact for any rail width"
        )]
        let px = self.width as f32;
        px
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overlay's open→close round trip: the claim comes back,
    /// the armed pair dies with the close (the images panel's own
    /// rule), and the door shuts.
    #[test]
    fn an_overlay_close_returns_the_claim_and_kills_the_armed_pair() {
        let mut panel = PanelContainer::Overlay(OverlayPanel::default());
        panel.open(Some(FocusClaim("pane-42".to_owned())));
        assert!(panel.is_open());
        if let PanelContainer::Overlay(overlay) = &mut panel {
            overlay.armed = Some(std::path::PathBuf::from("/tmp/a.png"));
        }
        let claim = panel.close().expect("an open panel closes");
        assert_eq!(claim, FocusClaim("pane-42".to_owned()));
        assert!(!panel.is_open());
        if let PanelContainer::Overlay(overlay) = &panel {
            assert!(overlay.armed.is_none(), "the armed pair never survives");
            assert!(overlay.focus_claim.is_none(), "the claim is spent");
        }
    }

    /// Closing a closed container returns nothing and changes
    /// nothing — the idempotent door.
    #[test]
    fn closing_a_closed_container_is_a_no_op() {
        let mut panel = PanelContainer::Overlay(OverlayPanel::default());
        assert!(panel.close().is_none());
        assert!(!panel.is_open());
    }

    /// Opening an open panel keeps the first claim — a re-open does
    /// not steal the keyboard's origin.
    #[test]
    fn reopening_keeps_the_first_claim() {
        let mut panel = PanelContainer::Overlay(OverlayPanel::default());
        panel.open(Some(FocusClaim("first".to_owned())));
        panel.open(Some(FocusClaim("second".to_owned())));
        let claim = panel.close().expect("still open");
        assert_eq!(claim, FocusClaim("first".to_owned()));
    }

    /// The rail's size memory: clamped to the floor, surviving
    /// whole — there is no close for a rail.
    #[test]
    fn the_rails_memory_clamps_and_survives() {
        let mut rail = PanelContainer::Edge(EdgeRail {
            edge: RailEdge::Right,
            width: 320,
        });
        assert!(!rail.is_open(), "a rail stands by its gate");
        if let PanelContainer::Edge(edge) = &mut rail {
            edge.resize(480);
        }
        if let PanelContainer::Edge(edge) = &rail {
            assert_eq!(edge.width, 480);
        }
        if let PanelContainer::Edge(edge) = &mut rail {
            edge.resize(10);
        }
        if let PanelContainer::Edge(edge) = &rail {
            assert_eq!(edge.width, MIN_RAIL_WIDTH, "the floor holds");
        }
        assert!(rail.close().is_none(), "a rail has no close door");
        if let PanelContainer::Edge(edge) = &rail {
            assert_eq!(edge.width, MIN_RAIL_WIDTH, "the memory survives");
        }
    }
}
