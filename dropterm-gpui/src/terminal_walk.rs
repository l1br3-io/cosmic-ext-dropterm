// SPDX-License-Identifier: GPL-3.0-only

//! The terminal-grid walk harness (gpui-terminal Task 1): a
//! drawer-shaped layer surface whose whole content is one live
//! terminal pane, opened at an 8×20-class grid and widened once
//! mid-run — the render and resize rows the chunk's manual walk
//! reads (`Term::resize` + SIGWINCH, the check-9 path).
//!
//! Armed with `DROPTERM_CHECK=terminal-grid` beside
//! `DROPTERM_EXIT_AFTER_MS`; the pane's program is `DROPTERM_TERM_CMD`
//! (a shell command line — the walk seeds colored runs, scrollback
//! rows, and a trailing `cat` whose echo proves typed input
//! end-to-end), defaulting to `/bin/sh`. `DROPTERM_TRACE_KEYS=1`
//! prints every keystroke's platform shape (key, modifiers,
//! `key_char`), each pointer press's cell, and each wheel delta — the
//! input walk's instrumentation.
//!
//! Walk finding (2026-11-13, Task 1): the sway rig presents the
//! drawer's own windows (the band, pixel-proven) but never presents
//! this harness's window — across anchors, layers, open timing,
//! first- and second-window orders, redraw cadence, and root
//! content. The pane's paint path is proven by its logged rows
//! (grid, caret, projected row text through the shaper) and by the
//! validation VM's pixels; the harness window's rig silence is the
//! environment's, recorded here rather than chased further.
//!
//! Walk findings (2026-11-14, Task 2's input rows — all
//! environment, refined against the finding above): (1) the rig's
//! never-presenting surface DOES hold keyboard focus — wtype's
//! chords drive the pane's actions and typed echo on sway — so the
//! Task-1 finding narrows to pixels and pointer: the left-press
//! trace never fires there, and the drag/selection rows are the
//! VM's; (2) cosmic-comp drops wtype's virtual-keyboard modifier
//! state — every chord arrives as its bare key — so the VM's chords
//! ride ydotool name-sequences (`ydotool key ctrl+shift+a`) instead;
//! (3) uinput wheel synthesis never becomes a scroll event for ANY
//! app on the validation VM — legacy `REL_WHEEL`, `REL_WHEEL_HI_RES`,
//! and `INPUT_PROP_WHEEL` were all tried, motion and buttons land,
//! and cosmic-term (an independent app) scrolls none of it — so the
//! physical-wheel row rides parity's human-at-console class: the
//! platform's wheel path is verified in gpui-pre-linux source
//! (AxisDiscrete/Value120 → frame flush → `ScrollEvent` to the
//! pointer-focused window), the pane's `on_wheel` is wired to it,
//! and the keyboard scrollback rows drive the same engine
//! `scroll()` call.
//!
//! Walk procedure (2026-11-14, Task 3): the mouse rows seed a
//! `cat -v` program under DECSET 1000(+1006) so the pane's own
//! tracking bytes echo back visibly — the walk reads the encodings
//! off the grid itself; and a data-control offer lives only as long
//! as the serving process, so a clipboard walk's `wl-paste` marker
//! check must run while the pane lives, never after the exit timer.

use std::time::Duration;

use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui::{App, Bounds, Size, WindowBackgroundAppearance, WindowBounds, WindowKind, point, px};

use crate::terminal::view;
use dropterm_core::config::DropTermConfig;

/// The narrow grid the walk opens at (the acceptance's 8×20) and the
/// widened column count it grows to, same rows.
const NARROW_COLS: f32 = 8.;
const WIDE_COLS: f32 = 100.;
const WALK_ROWS: f32 = 20.;

/// `DROPTERM_TERM_CMD`'s spawn: a shell command line run through
/// `/bin/sh -c`; unset means a plain `/bin/sh`.
fn walk_shell(raw: Option<&str>) -> (String, Vec<String>) {
    match raw.map(str::trim).filter(|cmd| !cmd.is_empty()) {
        Some(command) => (
            "/bin/sh".to_string(),
            vec!["-c".to_string(), command.to_string()],
        ),
        None => ("/bin/sh".to_string(), Vec::new()),
    }
}

/// The drawer-shaped stand-in the pane walks in: a top band at an
/// explicit pixel size, over panels, keyboard exclusive — the layer
/// request the drawer's executor would land for this geometry.
fn pane_window_options(width: u32, height: u32) -> gpui::WindowOptions {
    #[expect(
        clippy::cast_precision_loss,
        reason = "window px of a surface request: < 2^24, exact in f32"
    )]
    let (w, h) = (width as f32, height as f32);
    gpui::WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: Size::new(px(w), px(h)),
        })),
        app_id: Some(dropterm_core::APP_ID.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: dropterm_core::APP_ID.to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::LEFT,
            exclusive_zone: Some(px(-1.)),
            exclusive_edge: None,
            margin: Some((px(0.), px(0.), px(0.), px(0.))),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
        }),
        ..Default::default()
    }
}

/// Boot the walk: spawn the engine terminal (its PTY runs the walk
/// command), open the pane's window at the narrow grid, and widen it
/// once mid-run.
pub(crate) fn boot(cx: &mut App, exit_after: Option<Duration>) {
    let config = DropTermConfig::load_from(&dropterm_core::config::store::store_dir());

    // The Kit's theme global (gpui-parity Task 1): the completion
    // menu's panel reads the popover tokens, so the harness
    // initializes the same global the app shell does (the pickers'
    // test pattern) — the menu paints identically here.
    gpui_component::init(cx);

    // The shared construction path (the shell wiring's own, Task 4):
    // one boot_pane for the harness and the drawer alike. The walk
    // seeds its PTY through `DROPTERM_TERM_CMD`'s shell line.
    let (shell, args) = walk_shell(std::env::var("DROPTERM_TERM_CMD").ok().as_deref());
    let Some((terminal, pane)) = view::boot_pane(
        cx,
        &config,
        Some(view::PaneSpawn {
            program: shell,
            args,
            cwd: None,
        }),
        None,
        dropterm_core::tabs::TabId(0),
        None,
    ) else {
        std::process::exit(1);
    };

    // The measured cell sizes the narrow grid: the acceptance's
    // literal 8×20, one cell per column and row.
    let cell = terminal.lock().expect("engine lock").size();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "cells are positive px well below u32::MAX; ceil keeps the last cell whole"
    )]
    let narrow = (
        (NARROW_COLS * cell.cell_width).ceil() as u32,
        (WALK_ROWS * cell.cell_height).ceil() as u32,
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "cells are positive px well below u32::MAX; ceil keeps the last cell whole"
    )]
    let wide = ((WIDE_COLS * cell.cell_width).ceil() as u32, narrow.1);
    println!(
        "dropterm-gpui: CHECK terminal-grid opening at {narrow:?} (target 8x20), widening to \
         {wide:?} mid-run (cell {:.2}x{:.2})",
        cell.cell_width, cell.cell_height
    );

    let window: gpui::AnyWindowHandle =
        match cx.open_window(pane_window_options(narrow.0, narrow.1), |_, _| pane) {
            Ok(window) => window.into(),
            Err(err) => {
                eprintln!("dropterm-gpui: terminal-grid window open failed: {err:?}");
                std::process::exit(1);
            }
        };

    // The widen step: one resize mid-run, the check-9 path driven
    // the way the drawer's reshape drives it (a window resize).
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(2_500))
            .await;
        cx.update(|cx| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "window px of a surface request: < 2^24, exact in f32"
            )]
            let (w, h) = (wide.0 as f32, wide.1 as f32);
            let _ = window.update(cx, |_, window, _| {
                window.resize(Size::new(px(w), px(h)));
            });
        });
        println!("dropterm-gpui: CHECK terminal-grid widened to {wide:?}");
    })
    .detach();

    // The deterministic exit, the scaffold's affordance.
    if let Some(after) = exit_after {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(after).await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::walk_shell;

    #[test]
    fn the_walk_shell_wraps_a_command_line_in_sh_c() {
        // The walk's seeding rides one shell command line; unset or
        // blank falls back to a plain interactive /bin/sh.
        assert_eq!(
            walk_shell(Some("printf colored")),
            (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), "printf colored".to_string()]
            )
        );
        assert_eq!(walk_shell(Some("  ")), ("/bin/sh".to_string(), Vec::new()));
        assert_eq!(walk_shell(None), ("/bin/sh".to_string(), Vec::new()));
    }
}
