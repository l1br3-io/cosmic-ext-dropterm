// SPDX-License-Identifier: GPL-3.0-only

//! The status band: a 28 px strip at the surface's free edge,
//! adjacent to the grab bar, narrating the **active terminal** —
//! what runs, where it is, and how it is dressed.
//!
//! Derived, not decorated: every color flows through the tab strip's
//! own `pane_colors` derivation, content reads existing in-app state
//! (the cwd cache, the recorded shell, the id-resolved profile, the
//! zoom adjustment), and the lone syscall — the foreground process
//! name — lands in an app-side snapshot refreshed only at event
//! piggyback points, never per-frame (status-bar criterion 5). The
//! widget half (Task 2) renders this module's pure model.
//!
//! Segments are values without labels — a path reads as a path —
//! separated by gaps, never punctuation. The one live element is the
//! process-state disc: filled while a foreground child runs, a hollow
//! ring over the idle shell.

// The pure model is test-pinned ahead of its consumer: Task 2's
// widget and app wiring construct these items at runtime. The
// expectation self-expires the moment they do.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the Task 1 model; Task 2's widget and wiring consume it"
    )
)]

use std::path::Path;

use crate::config::{Edge, TabPosition};

/// The band's height in logical pixels — one notch below the tab
/// strip's 32 px, for callers that reserve room for it outside the
/// widget tree (see `surface::terminal_view_height`).
pub const BAND_HEIGHT: f32 = 28.0;

/// One strip of the surface's vertical column, top to bottom. The
/// stacking helper ([`column_stacking`]) orders all four; the app's
/// column match delegates to it instead of re-deriving the orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strip {
    /// The tab strip (`tab_bar::BAR_HEIGHT`).
    Tabs,
    /// The status band ([`BAND_HEIGHT`]).
    StatusBand,
    /// The terminal pane.
    Terminal,
    /// The grab bar (`grab_bar::GRAB_BAR_HEIGHT`).
    GrabBar,
}

/// The surface column's stacking order: the status band always takes
/// the slot **adjacent to the grab bar** — the free-edge-most band,
/// whatever the tab strip's own pinning (status-bar criterion 1).
/// `Auto` keeps the tab strip on the surface edge opposite the screen
/// edge; a pin places it on its named side regardless of anchor.
// Test-facing until Task 2 mounts the band: runtime re-derives the
// four column arms through `view_window`'s match, and this pin is the
// delegation target the wiring lands on. The expectation self-expires
// the moment runtime code consumes the helper.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the stacking pin; Task 2's column arms delegate to it"
    )
)]
pub const fn column_stacking(edge: Edge, tab_position: TabPosition) -> [Strip; 4] {
    match (edge, tab_position) {
        (Edge::Top, TabPosition::Top) => [
            Strip::Tabs,
            Strip::Terminal,
            Strip::StatusBand,
            Strip::GrabBar,
        ],
        (Edge::Top, TabPosition::Auto | TabPosition::Bottom) => [
            Strip::Terminal,
            Strip::Tabs,
            Strip::StatusBand,
            Strip::GrabBar,
        ],
        (Edge::Bottom, TabPosition::Top | TabPosition::Auto) => [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Tabs,
            Strip::Terminal,
        ],
        (Edge::Bottom, TabPosition::Bottom) => [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Terminal,
            Strip::Tabs,
        ],
    }
}

/// The resolved segments of the active tab's status band, left
/// cluster then right, values without labels — a path reads as a
/// path. The disc's fill state rides `running`; the process name is
/// the foreground child's comm, or the recorded shell's basename when
/// the shell itself owns the foreground (or the comm read failed —
/// the honest fallback, never a blank band).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusSegments {
    /// Whether a foreground child runs — the disc fills.
    pub running: bool,
    /// The foreground process's name, or the shell's basename when
    /// idle/unreadable.
    pub process: String,
    /// The `~`-collapsed cwd, absent when the tab has none.
    pub path: Option<String>,
    /// The tab's id-resolved profile name, absent on plain-shell tabs
    /// (and on dangling ids — never a stale name).
    pub profile: Option<String>,
    /// The signed zoom percent (`zoom_adj × step`), absent at zero.
    pub zoom_percent: Option<i16>,
}

/// Assemble the active tab's segments from the app's existing state
/// plus the process-name snapshot. Every input is plain data — the
/// resolver is pure and headless-testable; the app feeds it
/// `tab_cwds`, the recorded shell, the id-resolved profile name, the
/// zoom adjustment and step, and the comm snapshot.
pub fn resolve_segments(
    home: &Path,
    cwd: Option<&Path>,
    shell: &str,
    profile_name: Option<&str>,
    zoom_adj: i8,
    zoom_step_mul_100: u16,
    foreground: Option<&str>,
) -> StatusSegments {
    StatusSegments {
        running: foreground.is_some(),
        process: foreground.map_or_else(|| shell_basename(shell), str::to_string),
        path: cwd.map(|cwd| collapse_home(cwd, home)),
        profile: profile_name.map(str::to_string),
        zoom_percent: zoom_percent(zoom_adj, zoom_step_mul_100),
    }
}

/// Collapse a `home`-prefixed path to `~` plus its remainder — on
/// component boundaries, so a sibling directory sharing the home's
/// string prefix (`/home/jpt-x` under `/home/jpt`) never collapses.
/// `home` itself collapses to a bare `~`.
pub fn collapse_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home).ok() {
        Some(remainder) if !remainder.as_os_str().is_empty() => {
            format!("~/{}", remainder.display())
        }
        Some(_) => "~".to_string(),
        None => path.display().to_string(),
    }
}

/// Elide a `~`-collapsed path from its left, keeping the tail:
/// `…/l1br3/ced2` — what the user is inside of survives. Paths of
/// `max_chars` or fewer pass through untouched; `max_chars` of zero
/// elides to the empty string.
pub fn elide_path_left(path: &str, max_chars: usize) -> String {
    if path.chars().count() <= max_chars {
        return path.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let mut kept: Vec<char> = path.chars().rev().take(max_chars - 1).collect();
    kept.reverse();
    kept.insert(0, '\u{2026}');
    kept.into_iter().collect()
}

/// The signed zoom percent a `zoom_adj` of `step_mul_100`-sized steps
/// adds: `adj × step_mul_100`, saturating into `i16`. Zero adjustment
/// is `None` — the segment hides until the font is not at its base
/// size.
pub fn zoom_percent(zoom_adj: i8, step_mul_100: u16) -> Option<i16> {
    (zoom_adj != 0).then(|| {
        let product = i32::from(zoom_adj) * i32::from(step_mul_100);
        i16::try_from(product.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
            .expect("clamped into i16 range before the conversion")
    })
}

/// The recorded shell's basename — the idle process segment's honest
/// name (`fish`, `zsh`), never the full path the spawn recorded.
pub fn shell_basename(shell: &str) -> String {
    Path::new(shell).file_name().map_or_else(
        || shell.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> std::path::PathBuf {
        std::path::PathBuf::from("/home/jpt")
    }

    // --- segment assembly (running/idle × profile × zoom × cwd) ---

    #[test]
    fn a_running_child_names_the_process_and_fills_the_disc() {
        let segments = resolve_segments(
            &home(),
            Some(Path::new("/home/jpt/src/oss/l1br3/ced2")),
            "/usr/bin/fish",
            Some("dev"),
            2,
            100,
            Some("cargo"),
        );
        assert!(segments.running);
        assert_eq!(segments.process, "cargo");
        assert_eq!(segments.path.as_deref(), Some("~/src/oss/l1br3/ced2"));
        assert_eq!(segments.profile.as_deref(), Some("dev"));
        assert_eq!(segments.zoom_percent, Some(200));
    }

    #[test]
    fn an_idle_tab_falls_back_to_the_shell_basename_and_a_hollow_disc() {
        let segments = resolve_segments(
            &home(),
            Some(Path::new("/home/jpt")),
            "/usr/bin/fish",
            None,
            0,
            100,
            None,
        );
        assert!(!segments.running);
        assert_eq!(segments.process, "fish");
        assert_eq!(segments.path.as_deref(), Some("~"));
        assert_eq!(segments.profile, None);
        assert_eq!(segments.zoom_percent, None);
    }

    #[test]
    fn an_unreadable_comm_falls_back_like_idle() {
        // A failed /proc read is indistinguishable from idle in its
        // inputs — the band stays honest either way.
        let segments = resolve_segments(
            &home(),
            Some(Path::new("/srv")),
            "/bin/bash",
            Some("ops"),
            1,
            100,
            None,
        );
        assert!(!segments.running);
        assert_eq!(segments.process, "bash");
        assert_eq!(segments.profile.as_deref(), Some("ops"));
    }

    #[test]
    fn a_missing_cwd_drops_only_the_path_segment() {
        let segments = resolve_segments(
            &home(),
            None,
            "/usr/bin/zsh",
            Some("dev"),
            1,
            100,
            Some("vim"),
        );
        assert_eq!(segments.path, None);
        assert!(segments.running);
        assert_eq!(segments.process, "vim");
        assert_eq!(segments.profile.as_deref(), Some("dev"));
        assert_eq!(segments.zoom_percent, Some(100));
    }

    #[test]
    fn a_dangling_profile_id_hides_the_segment() {
        // The app resolves the id before feeding the resolver; a
        // missing profile arrives as `None`, never a stale name.
        let segments = resolve_segments(&home(), None, "/usr/bin/zsh", None, 0, 100, None);
        assert_eq!(segments.profile, None);
        assert_eq!(segments.zoom_percent, None);
    }

    // --- the ~ collapse ---

    #[test]
    fn a_home_prefixed_path_collapses_to_the_tilde() {
        assert_eq!(
            collapse_home(Path::new("/home/jpt/src/oss"), &home()),
            "~/src/oss"
        );
    }

    #[test]
    fn home_itself_collapses_to_a_bare_tilde() {
        assert_eq!(collapse_home(&home(), &home()), "~");
    }

    #[test]
    fn a_foreign_path_stays_verbatim() {
        assert_eq!(collapse_home(Path::new("/srv/web"), &home()), "/srv/web");
    }

    #[test]
    fn a_sibling_sharing_the_home_prefix_does_not_collapse() {
        // Component boundaries, not string prefixes: `/home/jpt-x`
        // does not live inside `/home/jpt`.
        assert_eq!(
            collapse_home(Path::new("/home/jpt-x/thing"), &home()),
            "/home/jpt-x/thing"
        );
    }

    // --- left elision ---

    #[test]
    fn short_paths_pass_through_untouched() {
        assert_eq!(elide_path_left("~/src", 100), "~/src");
        assert_eq!(elide_path_left("~/src", 5), "~/src");
    }

    #[test]
    fn long_paths_keep_their_tail_under_a_leading_ellipsis() {
        assert_eq!(elide_path_left("~/src/oss/l1br3/ced2", 12), "…/l1br3/ced2");
        assert_eq!(
            elide_path_left("/srv/web/prod/current", 13),
            "…prod/current"
        );
    }

    #[test]
    fn degenerate_widths_elide_to_the_floor() {
        assert_eq!(elide_path_left("~/src/oss", 0), "");
        assert_eq!(elide_path_left("~/src/oss", 1), "…");
    }

    // --- zoom arithmetic ---

    #[test]
    fn zoom_percent_multiplies_the_step_signed() {
        assert_eq!(zoom_percent(2, 100), Some(200));
        assert_eq!(zoom_percent(-1, 100), Some(-100));
        assert_eq!(zoom_percent(3, 50), Some(150));
    }

    #[test]
    fn zoom_percent_hides_at_zero_adjustment() {
        assert_eq!(zoom_percent(0, 100), None);
        assert_eq!(zoom_percent(0, 0), None);
    }

    #[test]
    fn zoom_percent_saturates_rather_than_wrapping() {
        assert_eq!(zoom_percent(i8::MAX, u16::MAX), Some(i16::MAX));
        assert_eq!(zoom_percent(i8::MIN, u16::MAX), Some(i16::MIN));
    }

    // --- the stacking rule ---

    #[test]
    fn top_anchor_with_a_top_pinned_strip_keeps_the_band_beside_the_grab_bar() {
        assert_eq!(
            column_stacking(Edge::Top, TabPosition::Top),
            [
                Strip::Tabs,
                Strip::Terminal,
                Strip::StatusBand,
                Strip::GrabBar
            ]
        );
    }

    #[test]
    fn top_anchor_with_an_auto_or_bottom_strip_stacks_the_strip_beside_the_grab_bar() {
        // Auto keeps the strip opposite the screen edge — surface
        // bottom under a Top anchor, beside the grab bar; an explicit
        // Bottom pin lands the same way.
        let expected = [
            Strip::Terminal,
            Strip::Tabs,
            Strip::StatusBand,
            Strip::GrabBar,
        ];
        assert_eq!(column_stacking(Edge::Top, TabPosition::Auto), expected);
        assert_eq!(column_stacking(Edge::Top, TabPosition::Bottom), expected);
    }

    #[test]
    fn bottom_anchor_with_an_auto_or_top_strip_stacks_the_band_under_the_grab_bar() {
        // The grab bar leads the column at the Bottom anchor's free
        // edge (the surface top); the band takes the slot right under
        // it.
        let expected = [
            Strip::GrabBar,
            Strip::StatusBand,
            Strip::Tabs,
            Strip::Terminal,
        ];
        assert_eq!(column_stacking(Edge::Bottom, TabPosition::Auto), expected);
        assert_eq!(column_stacking(Edge::Bottom, TabPosition::Top), expected);
    }

    #[test]
    fn bottom_anchor_with_a_bottom_pinned_strip_keeps_the_band_beside_the_grab_bar() {
        assert_eq!(
            column_stacking(Edge::Bottom, TabPosition::Bottom),
            [
                Strip::GrabBar,
                Strip::StatusBand,
                Strip::Terminal,
                Strip::Tabs,
            ]
        );
    }

    #[test]
    fn the_band_is_adjacent_to_the_grab_bar_in_every_stacking() {
        for (edge, position) in [
            (Edge::Top, TabPosition::Auto),
            (Edge::Top, TabPosition::Top),
            (Edge::Top, TabPosition::Bottom),
            (Edge::Bottom, TabPosition::Auto),
            (Edge::Bottom, TabPosition::Top),
            (Edge::Bottom, TabPosition::Bottom),
        ] {
            let stacking = column_stacking(edge, position);
            let band = stacking
                .iter()
                .position(|strip| *strip == Strip::StatusBand)
                .expect("the band stacks in every order");
            let grab = stacking
                .iter()
                .position(|strip| *strip == Strip::GrabBar)
                .expect("the grab bar stacks in every order");
            assert_eq!(
                band.abs_diff(grab),
                1,
                "band must sit beside the grab bar: {edge:?}/{position:?} -> {stacking:?}"
            );
            // The grab bar stays at the free edge — the surface edge
            // opposite the screen anchor — wherever the strips pin.
            let expected_grab = if matches!(edge, Edge::Bottom) {
                0
            } else {
                stacking.len() - 1
            };
            assert_eq!(
                grab, expected_grab,
                "grab bar pins to the free edge: {edge:?}/{position:?} -> {stacking:?}"
            );
        }
    }

    // --- the idle name ---

    #[test]
    fn the_shell_basename_strips_the_directory() {
        assert_eq!(shell_basename("/usr/bin/fish"), "fish");
        assert_eq!(shell_basename("zsh"), "zsh");
    }
}
