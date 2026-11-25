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

// --- the rail's ask/drain state (panel-slot, PS2) ---

use dropterm::core::lua_host::{PanelAnswerRow, PanelViewRow};
use dropterm::core::slots::PanelContent;
use std::collections::HashMap;

/// One laid section of the rail's view (panel-slot): the view's id,
/// its title, and its rows — each row's painted runs with the
/// element id its press carries (the script-named `id`, else the
/// positional index as a string).
#[derive(Debug, Clone, PartialEq)]
pub struct PanelSectionVm {
    pub id: String,
    pub title: String,
    pub rows: Vec<LaidRow>,
}

/// One laid row (panel-slot, style-engine SE4): the composed runs
/// beside the row's own style hooks — the `row` node's inputs, the
/// shell's resolve at the render seam.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidRow {
    /// The row's press id: the script-named `id`, else the
    /// positional index.
    pub press_id: String,
    pub runs: Vec<dropterm::core::status::BandScriptRunVm>,
    /// The row's class hook, whole (the shell splits at resolve).
    pub class: Option<String>,
    /// The row's numeric event id — the `#id`'s string form.
    pub id: Option<u32>,
    /// The row's inline declarations.
    pub style: Vec<dropterm::core::style_engine::Declaration>,
}

/// The right rail's live state (panel-slot): the registry's sync rows
/// (the titled sections' truth), each view's last painted content
/// (the stale-degrade keeps priors), and the one cadence's
/// bookkeeping — the boot ask, the status-refresh pull, and the
/// interval, the stood default's triggers. Pure state: the shell
/// owns the sends, this pins the decisions headless.
#[derive(Debug, Default)]
pub struct PanelRail {
    views: Vec<PanelViewRow>,
    sections: HashMap<String, PanelContent>,
    token: u64,
    queried_at: Option<std::time::Instant>,
    dirty: bool,
    boot_armed: bool,
}

impl PanelRail {
    /// Apply a registry sync: the titled sections' truth. Vanished
    /// views' sections sweep; the sync arms the boot ask (the
    /// registry's first report is the rail's first reason to ask —
    /// the no-views report never fires).
    pub fn note_views(&mut self, views: Vec<PanelViewRow>) {
        let fresh = views != self.views;
        self.views = views;
        let ids: std::collections::HashSet<&str> =
            self.views.iter().map(|row| row.id.as_str()).collect();
        self.sections.retain(|id, _| ids.contains(id.as_str()));
        if fresh {
            self.boot_armed = true;
        }
    }

    /// The status-refresh pull's mark (the stood default's second
    /// trigger — one cadence with the segments').
    pub fn note_dirty(&mut self) {
        self.dirty = true;
    }

    /// The requery decision: an empty registry never fires; the boot
    /// ask, a drained dirty pull, or the interval past the last
    /// dispatch fires one ask — the token bumps and the clock
    /// stamps. Returns the dispatch's token for the send.
    pub fn requery_due(&mut self, now: std::time::Instant) -> Option<u64> {
        if self.views.is_empty() {
            return None;
        }
        let interval = dropterm::core::status::SCRIPT_QUERY_INTERVAL;
        let interval_due = self.queried_at.is_none_or(|at| now >= at + interval);
        if !self.boot_armed && !self.dirty && !interval_due {
            return None;
        }
        self.boot_armed = false;
        self.dirty = false;
        self.queried_at = Some(now);
        self.token += 1;
        Some(self.token)
    }

    /// The ask's view ids, in registration order.
    pub fn due_views(&self) -> Vec<String> {
        self.views.iter().map(|row| row.id.clone()).collect()
    }

    /// Apply a reply: a stale flag or a superseded token drops it
    /// whole; a nil clears that view's content this round (a fresh
    /// hide — the section renders nothing); an omitted id keeps its
    /// prior (the stale-degrade).
    pub fn apply_reply(&mut self, token: u64, stale: bool, rows: Vec<PanelAnswerRow>) {
        if stale || token != self.token {
            return;
        }
        for row in rows {
            match row.content {
                Some(content) => {
                    self.sections.insert(row.id, content);
                }
                None => {
                    self.sections.remove(&row.id);
                }
            }
        }
    }

    /// The press decision (panel-slot PS3): the row press's
    /// slot-event, `None` when the view is not registered — a
    /// reloaded-away view's press is the shell's one log line, the
    /// registry the truth about who answers.
    pub fn press_event(
        &self,
        view_id: &str,
        row_id: &str,
    ) -> Option<dropterm::core::slots::SlotEvent> {
        let registered = self.views.iter().any(|view| view.id == view_id);
        if !registered {
            return None;
        }
        Some(dropterm::core::slots::SlotEvent {
            surface: "panel".to_owned(),
            slot: view_id.to_owned(),
            element_id: row_id.to_owned(),
            kind: "press".to_owned(),
        })
    }

    /// The laid sections, in registration order: only views standing
    /// in the registry, only sections with content, each row's runs
    /// composed and its press id resolved (the script-named `id`,
    /// else the positional index).
    pub fn sections_laid(
        &self,
        engine: &dropterm::core::style_engine::StyleSet,
    ) -> Vec<PanelSectionVm> {
        self.views
            .iter()
            .filter_map(|view| {
                let content = self.sections.get(&view.id)?;
                let rows = match content {
                    PanelContent::Rows { rows } => rows,
                    PanelContent::Slot(slot) => {
                        // A Slot-shaped answer paints one section of
                        // its own text — one row.
                        let runs = dropterm::core::status::script_runs(slot, engine, None);
                        return Some(PanelSectionVm {
                            id: view.id.clone(),
                            title: view.title.clone(),
                            rows: vec![LaidRow {
                                press_id: String::new(),
                                runs,
                                class: None,
                                id: None,
                                style: Vec::new(),
                            }],
                        });
                    }
                };
                let laid = rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| {
                        let press_id = row
                            .id
                            .map_or_else(|| index.to_string(), |id| id.to_string());
                        let runs = dropterm::core::status::script_runs(
                            &dropterm::core::slots::SlotContent::Styled {
                                spans: row.spans.clone(),
                            },
                            engine,
                            None,
                        );
                        LaidRow {
                            press_id,
                            runs,
                            class: row.class.clone(),
                            id: row.id,
                            style: row.style.clone(),
                        }
                    })
                    .collect();
                Some(PanelSectionVm {
                    id: view.id.clone(),
                    title: view.title.clone(),
                    rows: laid,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod rail_tests {
    use super::*;

    fn view(id: &str) -> PanelViewRow {
        PanelViewRow {
            id: id.to_owned(),
            title: id.to_owned(),
        }
    }

    /// The sync arms the boot ask; the ask's token bumps once; an
    /// empty registry never fires.
    #[test]
    fn the_boot_ask_fires_once_and_an_empty_registry_never() {
        let mut rail = PanelRail::default();
        assert!(
            rail.requery_due(std::time::Instant::now()).is_none(),
            "no views, no ask"
        );
        rail.note_views(vec![view("tasks")]);
        let token = rail
            .requery_due(std::time::Instant::now())
            .expect("the boot ask fires");
        assert_eq!(token, 1);
        assert!(
            rail.requery_due(std::time::Instant::now()).is_none(),
            "the boot ask is spent; the interval has not elapsed"
        );
        assert_eq!(rail.due_views(), vec!["tasks".to_owned()]);
    }

    /// The stale-degrade: a fresh answer replaces, a nil hides this
    /// round, an omitted id keeps its prior, and a stale or
    /// superseded reply drops whole.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one token/stale walk over one rail: the guards' whole shape in sequence"
    )]
    fn replies_apply_under_token_and_stale_guards() {
        use dropterm::core::slots::{SlotContent, Span};
        let mut rail = PanelRail::default();
        rail.note_views(vec![view("tasks")]);
        let token = rail
            .requery_due(std::time::Instant::now())
            .expect("the boot ask");
        // A stale reply drops whole — nothing stored.
        rail.apply_reply(
            token,
            true,
            vec![PanelAnswerRow {
                id: "tasks".to_owned(),
                content: Some(PanelContent::Slot(SlotContent::Text("stale".to_owned()))),
            }],
        );
        assert!(
            rail.sections_laid(&dropterm::core::style_engine::StyleSet::default())
                .is_empty()
        );
        // A superseded token drops too.
        rail.apply_reply(
            token + 7,
            false,
            vec![PanelAnswerRow {
                id: "tasks".to_owned(),
                content: Some(PanelContent::Slot(SlotContent::Text(
                    "superseded".to_owned(),
                ))),
            }],
        );
        assert!(
            rail.sections_laid(&dropterm::core::style_engine::StyleSet::default())
                .is_empty()
        );
        // The matching fresh reply lands.
        rail.apply_reply(
            token,
            false,
            vec![PanelAnswerRow {
                id: "tasks".to_owned(),
                content: Some(PanelContent::Slot(SlotContent::Text(
                    "three tasks".to_owned(),
                ))),
            }],
        );
        let laid = rail.sections_laid(&dropterm::core::style_engine::StyleSet::default());
        assert_eq!(laid.len(), 1);
        assert_eq!(laid[0].id, "tasks");
        assert_eq!(laid[0].rows.len(), 1);
        assert_eq!(laid[0].rows[0].runs[0].text, "three tasks");
        // The next trip hides it (nil): the section renders nothing.
        let next = rail
            .requery_due(std::time::Instant::now() + dropterm::core::status::SCRIPT_QUERY_INTERVAL);
        assert!(next.is_some(), "the interval fires");
        rail.apply_reply(
            next.expect("the token"),
            false,
            vec![PanelAnswerRow {
                id: "tasks".to_owned(),
                content: None,
            }],
        );
        assert!(
            rail.sections_laid(&dropterm::core::style_engine::StyleSet::default())
                .is_empty(),
            "a nil hides this round"
        );
        // A fresh styled answer lays one row per spans row, with the
        // positional press ids.
        let again = rail.requery_due(
            std::time::Instant::now() + 2 * dropterm::core::status::SCRIPT_QUERY_INTERVAL,
        );
        rail.apply_reply(
            again.expect("the token"),
            false,
            vec![PanelAnswerRow {
                id: "tasks".to_owned(),
                content: Some(PanelContent::Rows {
                    rows: vec![
                        dropterm::core::slots::PanelRow {
                            spans: vec![Span {
                                text: "one".to_owned(),
                                class: None,
                                id: None,
                                style: Vec::new(),
                            }],
                            id: Some(9),
                            class: None,
                            style: Vec::new(),
                        },
                        dropterm::core::slots::PanelRow {
                            spans: vec![Span {
                                text: "two".to_owned(),
                                class: None,
                                id: None,
                                style: Vec::new(),
                            }],
                            id: None,
                            class: None,
                            style: Vec::new(),
                        },
                    ],
                }),
            }],
        );
        let laid = rail.sections_laid(&dropterm::core::style_engine::StyleSet::default());
        assert_eq!(laid[0].rows.len(), 2);
        assert_eq!(laid[0].rows[0].press_id, "9", "the script-named id rides");
        assert_eq!(
            laid[0].rows[1].press_id, "1",
            "the positional index stands in"
        );
        assert_eq!(laid[0].rows[0].runs[0].text, "one");
    }

    /// The press routing: a registered view's row press yields the
    /// exact slot-event payload; a reloaded-away view's press yields
    /// nothing — the shell's one line.
    #[test]
    fn the_press_event_names_the_registered_view_and_drops_the_gone() {
        let mut rail = PanelRail::default();
        assert!(
            rail.press_event("tasks", "0").is_none(),
            "an unregistered view's press is None"
        );
        rail.note_views(vec![view("tasks")]);
        let event = rail.press_event("tasks", "3").expect("registered");
        assert_eq!(
            event,
            dropterm::core::slots::SlotEvent {
                surface: "panel".to_owned(),
                slot: "tasks".to_owned(),
                element_id: "3".to_owned(),
                kind: "press".to_owned(),
            },
            "the payload's four keys, exactly"
        );
        rail.note_views(vec![view("git")]);
        assert!(
            rail.press_event("tasks", "3").is_none(),
            "the reload swept the seat — the press is the shell's line"
        );
    }

    /// A vanished view's section sweeps at the sync; the survivors
    /// keep their priors across the gap.
    #[test]
    fn a_vanished_view_sweeps_and_survivors_keep_their_priors() {
        use dropterm::core::slots::{PanelContent, SlotContent};
        let mut rail = PanelRail::default();
        rail.note_views(vec![view("a"), view("b")]);
        let token = rail.requery_due(std::time::Instant::now()).expect("asks");
        rail.apply_reply(
            token,
            false,
            vec![
                PanelAnswerRow {
                    id: "a".to_owned(),
                    content: Some(PanelContent::Slot(SlotContent::Text("a's".to_owned()))),
                },
                PanelAnswerRow {
                    id: "b".to_owned(),
                    content: Some(PanelContent::Slot(SlotContent::Text("b's".to_owned()))),
                },
            ],
        );
        rail.note_views(vec![view("a")]);
        let laid = rail.sections_laid(&dropterm::core::style_engine::StyleSet::default());
        assert_eq!(laid.len(), 1, "b's section swept with its seat");
        assert_eq!(laid[0].id, "a");
        assert_eq!(laid[0].rows[0].runs[0].text, "a's", "the survivor keeps");
    }
}
