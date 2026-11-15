// SPDX-License-Identifier: GPL-3.0-only

//! The pasted-images panel on GPUI (gpui-parity py-6, the
//! libcosmic paste-panel spec's port): the band's `▦ N images` chip's
//! door — a right-anchored drawer over the live terminal (the
//! settings drawer's own surface and backdrop rule: the terminal
//! never hides or resizes beneath it), one row per pasted image,
//! newest first, each carrying the precedents' five verbs (Open the
//! standard one, the three transfer verbs quiet, the delete cluster
//! set apart) and — the 2026-11-15 scope ruling this panel ships
//! under — the labeled `Confirm delete` / `Keep` pair, matching the
//! drawer's own delete shape (the libcosmic panel keeps its ✓/✕ pair
//! until cutover; the walk note records the pair).

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use dropterm_core::tabs::PastedImage;
use gpui::prelude::*;
use gpui::{App, Context, Styled, Window, px};
use gpui_component::ActiveTheme as _;
use gpui_component::{h_flex, v_flex};

use crate::app_shell::Shell;

/// The panel's row order (the reference `images_panel_rows`): a view
/// over the pruned ledger, newest first — the most recent paste is
/// the row the eye lands on. Pure.
pub(crate) fn panel_rows(rows: &[PastedImage]) -> Vec<&PastedImage> {
    rows.iter().rev().collect()
}

/// The delete cluster's shape for one row (pure, the ruling's pin):
/// the idle row carries the single destructive `Delete`; the armed
/// row — one at a time, the single-arm seat — carries the labeled
/// `Confirm delete` / `Keep` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeleteCluster {
    Idle,
    Armed,
}

/// Which cluster a row renders: the armed pair stands only on the
/// armed row — `None` (nothing armed) or a different path keeps the
/// idle shape. Pure.
pub(crate) fn delete_cluster(armed: Option<&Path>, path: &Path) -> DeleteCluster {
    if armed == Some(path) {
        DeleteCluster::Armed
    } else {
        DeleteCluster::Idle
    }
}

/// One panel verb, routed by the shell (the precedents' five, the
/// arm/disarm pair, and the panel's doors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PanelVerb {
    /// The row's standard verb — the system viewer.
    Open(PathBuf),
    /// The row's path as text, the focus-free write.
    CopyPath(PathBuf),
    /// The row's bytes back to the clipboard in their sniffed format.
    CopyImage(PathBuf),
    /// The row's path through the terminal's own paste channel.
    Repaste(PathBuf),
    /// The idle cluster's press: arm this row's pair.
    ArmDelete(PathBuf),
    /// The armed pair's destructive leg: drop the row, its thumbnail,
    /// and — best-effort off-thread — its file.
    ConfirmDelete(PathBuf),
    /// The armed pair's disarm: delete nothing.
    KeepDelete,
    /// The header's folder verb: the paste cache in the file manager.
    OpenCache,
    /// The panel's close door: the header's Close and the backdrop's
    /// press ride it (an armed pair never survives the close).
    Close,
}

/// The panel's verb callback shape.
pub(crate) type VerbFn = Rc<dyn Fn(PanelVerb, &mut Window, &mut App)>;

/// One panel row's pure description — everything the render needs
/// beside the thumbnail handle.
pub(crate) struct PanelRow<'a> {
    pub(crate) row: &'a PastedImage,
    pub(crate) cluster: DeleteCluster,
}

/// Decodes one panel thumbnail off the UI thread's clock: the cache
/// file's pixels as a GPUI render image — the decode itself core's
/// own `decode_image` (the one path every image format rides; this
/// crate's `image` dep stays types-only) — `None` when the file
/// cannot be read or decoded (the row renders its bookkeeping alone,
/// the honest floor, never an error surface).
pub(crate) fn decode_thumb(path: &Path) -> Option<Arc<gpui::RenderImage>> {
    let bytes = std::fs::read(path).ok()?;
    let rgba = dropterm_core::terminal::decode_image(&bytes)?;
    let frame = image::Frame::new(rgba);
    Some(Arc::new(gpui::RenderImage::new([frame])))
}

/// One quiet text verb — the transfer verbs' own look (the muted
/// label, the press routed).
fn quiet_verb(
    id: gpui::SharedString,
    label: &'static str,
    make: PanelVerb,
    verb: &VerbFn,
    color: gpui::Hsla,
) -> impl gpui::IntoElement {
    let verb = Rc::clone(verb);
    gpui::div()
        .id(id)
        .text_color(color)
        .child(label)
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |_event, window: &mut Window, cx: &mut App| {
                verb(make.clone(), window, cx);
            },
        )
}

/// The panel's content: the row list, scrollable — or the empty state
/// when the tab carries no pastes.
pub(crate) fn panel_content(
    rows: Vec<PanelRow<'_>>,
    thumbs: &std::collections::HashMap<PathBuf, Arc<gpui::RenderImage>>,
    verb: &VerbFn,
    cx: &App,
) -> gpui::AnyElement {
    let mut list = v_flex()
        .id("panel-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .gap_4()
        .p_4();
    if rows.is_empty() {
        return list
            .gap_1()
            .child("No pasted images in this tab")
            .child(
                gpui::div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Images you paste land here, newest first."),
            )
            .into_any_element();
    }
    for panel_row in rows {
        let row = panel_row.row;
        // The identity line: the thumbnail (when its decode landed)
        // beside the stamp and dimensions, the whole line the Open
        // click target — the subject is its own target for its
        // standard verb.
        let mut text = v_flex()
            .gap_1()
            .child(gpui::div().child(dropterm_core::tabs::stamp_display(&row.stamp)));
        if let Some((width, height)) = row.dims {
            text = text.child(
                gpui::div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{width}×{height}")),
            );
        }
        let mut identity = h_flex().gap_3();
        if let Some(handle) = thumbs.get(&row.path) {
            identity = identity.child(gpui::img(Arc::clone(handle)).h(px(64.)));
        }
        identity = identity.child(text);
        let open = Rc::clone(verb);
        let open_path = row.path.clone();
        let identity = identity
            .id(gpui::SharedString::from(format!("panel-row-{}", row.stamp)))
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |_event, window: &mut Window, cx: &mut App| {
                    open(PanelVerb::Open(open_path.clone()), window, cx);
                },
            );
        // The verb line: Open the one standard verb, the three
        // transfer verbs quiet, the delete cluster riding the filling
        // gap to the far edge — the irreversible verb set apart from
        // the recoverable group.
        let mut actions = h_flex().gap_3();
        for (label, make) in [
            ("Open", PanelVerb::Open(row.path.clone())),
            ("Copy path", PanelVerb::CopyPath(row.path.clone())),
            ("Copy image", PanelVerb::CopyImage(row.path.clone())),
            ("Re-paste", PanelVerb::Repaste(row.path.clone())),
        ] {
            actions = actions.child(quiet_verb(
                gpui::SharedString::from(format!("panel-verb-{label}-{}", row.stamp)),
                label,
                make,
                verb,
                cx.theme().muted_foreground,
            ));
        }
        actions = actions.child(gpui::div().flex_1());
        // The cluster: the idle row's single destructive Delete; the
        // armed row's labeled pair (the ruling's shape — the drawer's
        // own delete idiom).
        let cluster = match panel_row.cluster {
            DeleteCluster::Idle => {
                vec![(
                    "Delete",
                    cx.theme().danger,
                    PanelVerb::ArmDelete(row.path.clone()),
                )]
            }
            DeleteCluster::Armed => vec![
                (
                    "Confirm delete",
                    cx.theme().danger,
                    PanelVerb::ConfirmDelete(row.path.clone()),
                ),
                ("Keep", cx.theme().muted_foreground, PanelVerb::KeepDelete),
            ],
        };
        for (label, color, make) in cluster {
            actions = actions.child(quiet_verb(
                gpui::SharedString::from(format!("panel-delete-{label}-{}", row.stamp)),
                label,
                make,
                verb,
                color,
            ));
        }
        list = list.child(v_flex().gap_2().child(identity).child(actions));
    }
    list.into_any_element()
}

/// The panel's overlay: the live band beneath a backdrop and the
/// right-anchored panel — the settings drawer's own composition (the
/// drawer width rule included), the backdrop's press and the header's
/// Close the two doors. The width is the drawer's configured rule at
/// its default seat (the drag handle is the settings drawer's own —
/// the panel borrows the surface, not the key).
pub(crate) fn overlay(
    band: gpui::AnyElement,
    rows: Vec<PanelRow<'_>>,
    thumbs: &std::collections::HashMap<PathBuf, Arc<gpui::RenderImage>>,
    verb: &VerbFn,
    surface_width: f32,
    cx: &mut Context<Shell>,
) -> gpui::AnyElement {
    let width = dropterm_core::settings::drawer_width(None, surface_width);
    let verb_header = Rc::clone(verb);
    let verb_backdrop = Rc::clone(verb);
    let verb_close = Rc::clone(verb);
    let panel = gpui::div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(width))
        .flex()
        .flex_col()
        .bg(cx.theme().background)
        .child(
            h_flex()
                .h_10()
                .flex_none()
                .justify_between()
                .px_4()
                .child(gpui::div().text_size(px(16.)).child("Pasted images"))
                .child(
                    h_flex().gap_4().child(
                        gpui::div()
                            .id("panel-open-cache")
                            .text_color(cx.theme().muted_foreground)
                            .child("Open folder")
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                move |_event, window: &mut Window, cx: &mut App| {
                                    verb_header(PanelVerb::OpenCache, window, cx);
                                },
                            ),
                    ),
                ),
        )
        .child(
            h_flex().flex_none().justify_end().px_4().child(
                gpui::div()
                    .id("panel-close")
                    .text_color(cx.theme().muted_foreground)
                    .child("Close")
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        move |_event, window: &mut Window, cx: &mut App| {
                            verb_close(PanelVerb::Close, window, cx);
                        },
                    ),
            ),
        )
        .child(panel_content(rows, thumbs, verb, cx));
    gpui::div()
        .absolute()
        .inset_0()
        .child(band)
        .child(
            gpui::div()
                .absolute()
                .inset_0()
                .id("images-panel-backdrop")
                .bg(gpui::black().opacity(0.5))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    move |_event, window: &mut Window, cx: &mut App| {
                        verb_backdrop(PanelVerb::Close, window, cx);
                    },
                )
                .child(panel),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, stamp: &str) -> PastedImage {
        PastedImage {
            path: PathBuf::from(path),
            stamp: stamp.to_string(),
            dims: None,
        }
    }

    #[test]
    fn panel_rows_list_newest_first() {
        // The ledger appends on paste; the panel reads it reversed —
        // the most recent paste is the row the eye lands on.
        let ledger = [row("/c/one", "1"), row("/c/two", "2"), row("/c/three", "3")];
        let rows = panel_rows(&ledger);
        assert_eq!(
            rows.iter()
                .map(|row| row.stamp.as_str())
                .collect::<Vec<_>>(),
            vec!["3", "2", "1"]
        );
    }

    #[test]
    fn the_labeled_pair_stands_only_on_the_armed_row() {
        // The single-arm seat (the ruling's pin): the armed row
        // carries the pair, every other row — and nothing armed —
        // keeps the idle Delete.
        let one = Path::new("/cache/paste-1.png");
        let other = Path::new("/cache/paste-2.png");
        assert_eq!(delete_cluster(None, one), DeleteCluster::Idle);
        assert_eq!(delete_cluster(Some(other), one), DeleteCluster::Idle);
        assert_eq!(delete_cluster(Some(one), one), DeleteCluster::Armed);
    }
}
