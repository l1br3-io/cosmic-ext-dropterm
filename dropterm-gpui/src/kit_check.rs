// SPDX-License-Identifier: GPL-3.0-only

//! The C2b kill check (gpui-scaffold Task 2): the carried patch's
//! verification, re-runnable on every Kit bump exactly as the comeback
//! checklist demands — Root + programmatic Popover and Tooltip held on
//! the layer surface through the full 80 ms reshape cadence, with
//! `on_open_change` as the close probe (silence in the log = the
//! overlays survived every reshape) and the mapped-bounds rows doubling
//! as the flush-edges evidence (a requested height must echo back
//! exactly — 2×20 px inflation means the carried const regressed).
//!
//! Armed with `DROPTERM_CHECK=kit-c2b` beside the shell's own
//! `DROPTERM_EXIT_AFTER_MS`; the procedure lives in
//! docs/development.md's carried-patches section.

use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, Context, Pixels, Size, Window, div, layer_shell::KeyboardInteractivity,
    prelude::*, px, rgba, white,
};
use gpui_component::{
    Root as KitRoot, button::Button, menu::PopupMenu, popover::Popover, tooltip::Tooltip,
};

use crate::app_shell;

/// The drag cadence the border finding was measured against (kit-pass
/// C2b); the kill check drives its reshape cycle at exactly this beat.
pub(crate) const CADENCE_MS: u64 = 80;

/// The reshape cycle: climb 200→320 and back at the cadence. The peak
/// is the flush-edges row (a 1280×320 drawer negotiates 1280×320), and
/// the bookends leave the surface at its resting height.
pub(crate) const KIT_HEIGHTS: [f32; 7] = [200., 240., 280., 320., 280., 240., 200.];

/// Boot the kill-check scenario: the Kit initialized, the drawer opened
/// at the cycle's peak height with the window's root view as the Kit's
/// `Root` — the manual wrap the kit-pass C3 ruling names (the embedder
/// owns the window, Root hosts the overlays).
pub(crate) fn boot(cx: &mut App, exit_after: Option<Duration>) {
    gpui_component::init(cx);
    let open = Instant::now();
    println!(
        "dropterm-gpui: CHECK-C2b Root + Kit overlays on the layer surface, {CADENCE_MS}ms cadence"
    );
    match cx.open_window(
        app_shell::drawer_options(px(KIT_HEIGHTS[0]), KeyboardInteractivity::None),
        |window, cx| {
            let chrome = cx.new(|cx| {
                if let Some(after) = exit_after {
                    app_shell::quit_after(cx, after);
                }
                KitChrome {
                    open,
                    frames: 0,
                    last_bounds: None,
                    last_resize: None,
                    resize_index: 0,
                    tooltip: None,
                }
            });
            cx.new(|cx| KitRoot::new(chrome, window, cx))
        },
    ) {
        Ok(_) => println!("dropterm-gpui: kill-check window opened"),
        Err(err) => {
            eprintln!("dropterm-gpui: kill-check open failed: {err:?}");
            std::process::exit(1);
        }
    }
}

/// The chrome under `Root`: drives the reshape cadence from render
/// while a Kit overlay pair stays on the surface — a Popover-hosted
/// `PopupMenu` opened programmatically (`default_open`; no pointer exists
/// in the harness environments) and a built Tooltip view. The
/// `on_open_change` probe prints if the overlay ever closes, so its
/// silence in a walk log is the kill check's pass row.
struct KitChrome {
    open: Instant,
    frames: u64,
    last_bounds: Option<Bounds<Pixels>>,
    last_resize: Option<Instant>,
    resize_index: usize,
    tooltip: Option<gpui::AnyView>,
}

impl Render for KitChrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let b = window.bounds();
        if self.last_bounds.as_ref() != Some(&b) {
            println!(
                "dropterm-gpui: kit frame {} drew size={:.0}x{:.0} at +{}ms",
                self.frames,
                b.size.width.as_f32(),
                b.size.height.as_f32(),
                self.open.elapsed().as_millis()
            );
            self.last_bounds = Some(b);
        }
        if self.frames == 1 {
            self.tooltip =
                Some(Tooltip::new("C2b tooltip on a layer-shell surface").build(window, cx));
            self.last_resize = Some(Instant::now());
        }
        // The cadence, driven from render (the window's root view is
        // Root, so a spawned task cannot reach this view's resize path
        // the way the reshape scenario did).
        if self.resize_index < KIT_HEIGHTS.len() {
            let since = self
                .last_resize
                .map_or(u128::MAX, |t| t.elapsed().as_millis());
            if since >= u128::from(CADENCE_MS) {
                let w = window.viewport_size().width;
                let h = KIT_HEIGHTS[self.resize_index];
                window.resize(Size::new(w, px(h)));
                println!(
                    "dropterm-gpui: kit resize #{} requested {:.0}x{h:.0} at +{}ms (overlay open)",
                    self.resize_index,
                    w.as_f32(),
                    self.open.elapsed().as_millis()
                );
                self.resize_index += 1;
                self.last_resize = Some(Instant::now());
            }
        } else if self.last_resize.is_some() {
            self.last_resize = None;
            println!("dropterm-gpui: kit cadence complete — overlays survived, holding for quit");
        }
        // Redraw ticks so render keeps firing at the cadence's
        // granularity instead of waiting on configures.
        if self.frames == 1 {
            cx.spawn(async move |this, cx| {
                loop {
                    let _ = this.update(cx, |_, cx| cx.notify());
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                }
            })
            .detach();
        }
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_6()
            .text_color(white())
            .bg(rgba(0x0000_0055))
            .child(
                Popover::new("kit-c2b-menu")
                    .trigger(Button::new("kit-c2b-btn").label("Kit menu"))
                    .content(|_, window, cx| {
                        PopupMenu::build(window, cx, |menu, _, _| {
                            menu.label("C2b: overlay menu on layer-shell")
                                .separator()
                                .label("held open through reshapes")
                        })
                    })
                    .default_open(true)
                    .on_open_change(|open, _, _| {
                        println!("dropterm-gpui: kit popover open_changed -> {open}");
                    }),
            )
            .children(self.tooltip.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cadence_is_the_drag_cadence() {
        // 80 ms is the drag-cadence budget the border finding was
        // measured against; the kill check must keep driving at it.
        assert_eq!(CADENCE_MS, 80);
    }

    #[test]
    fn the_height_cycle_climbs_to_the_flush_edges_row_and_returns() {
        // The cycle's peak (320) is the flush-edges row: a 1280×320
        // drawer must negotiate 1280×320, not 1320×360. The cycle also
        // starts and ends at the resting height so the check leaves the
        // surface where it found it.
        assert_eq!(KIT_HEIGHTS.first(), Some(&200.));
        assert_eq!(KIT_HEIGHTS.last(), Some(&200.));
        assert!(KIT_HEIGHTS.contains(&320.));
        // Monotone up to the peak, monotone back down — a drag-shaped
        // cycle, never a jitter pattern.
        let peak = KIT_HEIGHTS
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0)))
            .expect("non-empty")
            .0;
        for pair in KIT_HEIGHTS[..peak].windows(2) {
            assert!(pair[0] < pair[1], "climbs to the peak: {KIT_HEIGHTS:?}");
        }
        for pair in KIT_HEIGHTS[peak..].windows(2) {
            assert!(pair[0] > pair[1], "descends from the peak: {KIT_HEIGHTS:?}");
        }
    }
}
