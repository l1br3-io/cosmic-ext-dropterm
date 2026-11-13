// SPDX-License-Identifier: GPL-3.0-only

//! The app shell (gpui-scaffold Task 1): the `LayerShell` drawer window,
//! seeded from the spike harness (`spikes/gpui-layer-shell` and
//! `spikes/kit-consumer` on the spike branch — reference only, never
//! merged as code) and rewritten as this crate's shell. The spike's
//! evidence-line shapes are carried over (`dropterm-gpui:` prefixed)
//! so the rig and VM walks read the same rows they always did.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    App, Bounds, Context, Pixels, Size, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, div, layer_shell, layer_shell::KeyboardInteractivity, point, prelude::*, px, rems,
    rgba, white,
};

/// The drawer's window shape: full width, bottom edge, over panels —
/// the spike's `drawer_options`, re-identified through core's
/// [`dropterm_core::APP_ID`] (one identity const behind the
/// layer-shell namespace and the wayland app id, exactly the strings
/// the libcosmic binary derives).
pub(crate) fn drawer_options(height: Pixels, kb: KeyboardInteractivity) -> gpui::WindowOptions {
    gpui::WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: Size::new(px(0.), height), // width compositor-controlled: L|R anchored
        })),
        app_id: Some(dropterm_core::APP_ID.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(layer_shell::LayerShellOptions {
            namespace: dropterm_core::APP_ID.to_string(),
            layer: layer_shell::Layer::Top,
            anchor: layer_shell::Anchor::LEFT
                | layer_shell::Anchor::RIGHT
                | layer_shell::Anchor::BOTTOM,
            exclusive_zone: Some(px(-1.)),
            exclusive_edge: None,
            margin: Some((px(0.), px(0.), px(0.), px(0.))),
            keyboard_interactivity: kb,
        }),
        ..Default::default()
    }
}

/// The evidence walks' deterministic exit (a scaffold affordance, not
/// a feature): `DROPTERM_EXIT_AFTER_MS`, read once at boot. Unset —
/// and every non-positive or unparsable value — means a real app's
/// lifetime (run until the session ends); a positive value quits
/// through the event loop after the window has mapped and rendered, so
/// "maps and exits clean" is a logged, replayable row on the rig and
/// the VM. The real quit UX arrives with the surface slices.
pub(crate) fn exit_after_from_env(raw: Option<&str>) -> Option<Duration> {
    let ms: u64 = raw?.trim().parse().ok()?;
    (ms > 0).then(|| Duration::from_millis(ms))
}

/// Auto-quit timer: the shell's deterministic exit path (see
/// [`exit_after_from_env`]).
pub(crate) fn quit_after<T: 'static>(cx: &mut Context<T>, after: Duration) {
    cx.spawn(async move |_, cx| {
        cx.background_executor().timer(after).await;
        let () = cx.update(|cx| cx.quit());
    })
    .detach();
}

/// The placeholder band the scaffold renders: a clock face proving the
/// render loop stays alive (the notify loop below keeps frames
/// flowing), carried from the spike's panel. The real surface arrives
/// with the gpui-surface slice.
fn band() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(rems(4.5))
        .text_color(white())
        .bg(rgba(0x0000_0044))
        .child(clock_text())
}

fn clock_text() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        (now / 3600) % 24,
        (now / 60) % 60,
        now % 60
    )
}

/// The drawer's root view: logs mapped-bounds evidence rows (the
/// spike's shape — first frame, every change) and keeps the clock
/// rendering.
pub(crate) struct Shell {
    pub(crate) open: Instant,
    frames: u64,
    last_bounds: Option<Bounds<Pixels>>,
}

impl Shell {
    pub(crate) fn new(open: Instant) -> Self {
        Self {
            open,
            frames: 0,
            last_bounds: None,
        }
    }

    /// Notify loop keeps frames (and the clock) flowing.
    fn keep_alive(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let _ = this.update(cx, |_, cx| cx.notify());
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }
        })
        .detach();
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let b = window.bounds();
        if self.last_bounds.as_ref() != Some(&b) {
            println!(
                "dropterm-gpui: frame {} mapped origin=({},{}) size={:.0}x{:.0} at +{}ms",
                self.frames,
                b.origin.x.as_f32(),
                b.origin.y.as_f32(),
                b.size.width.as_f32(),
                b.size.height.as_f32(),
                self.open.elapsed().as_millis()
            );
            self.last_bounds = Some(b);
        }
        if self.frames == 1 {
            println!(
                "dropterm-gpui: OPEN->FIRST_FRAME {}ms",
                self.open.elapsed().as_millis()
            );
            Self::keep_alive(cx);
        }
        band()
    }
}

/// Boot the shell: open the drawer window on the app and run the
/// event loop to its clean exit.
pub(crate) fn boot(cx: &mut App, exit_after: Option<Duration>) {
    let open = Instant::now();
    println!(
        "dropterm-gpui: opening the drawer (L|R|B, layer Top, zone -1, kb None) as {}",
        dropterm_core::APP_ID
    );
    match cx.open_window(
        drawer_options(px(200.), KeyboardInteractivity::None),
        |_, cx| {
            cx.new(|cx| {
                if let Some(after) = exit_after {
                    quit_after(cx, after);
                }
                Shell::new(open)
            })
        },
    ) {
        Ok(_) => println!("dropterm-gpui: drawer window opened"),
        Err(err) => {
            eprintln!("dropterm-gpui: open_window failed: {err:?}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    use gpui::{WindowKind, px};

    #[test]
    fn drawer_options_carries_the_drawer_shape_and_the_app_identity() {
        let opts = drawer_options(px(320.), KeyboardInteractivity::Exclusive);
        // One identity const behind the wayland app id and the
        // layer-shell namespace — the same strings the libcosmic
        // binary derives, so compositor rules (COSMIC's blur, sway's
        // criteria) see the same surface.
        assert_eq!(opts.app_id.as_deref(), Some(dropterm_core::APP_ID));
        let WindowKind::LayerShell(ls) = opts.kind else {
            panic!("the drawer is a layer-shell surface");
        };
        assert_layer_shape(&ls);
        assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::Exclusive);
    }

    #[test]
    fn drawer_options_passes_the_keyboard_mode_through() {
        let WindowKind::LayerShell(ls) = drawer_options(px(200.), KeyboardInteractivity::None).kind
        else {
            panic!("the drawer is a layer-shell surface");
        };
        assert_layer_shape(&ls);
        assert_eq!(ls.keyboard_interactivity, KeyboardInteractivity::None);
    }

    /// The drawer shape every variant shares: Top layer (over panels),
    /// anchored left+right+bottom (full width, bottom edge), zone -1
    /// (over everything, reserving nothing), flush margins.
    fn assert_layer_shape(ls: &LayerShellOptions) {
        assert_eq!(ls.namespace, dropterm_core::APP_ID);
        assert_eq!(ls.layer, Layer::Top);
        assert!(ls.anchor.contains(Anchor::LEFT));
        assert!(ls.anchor.contains(Anchor::RIGHT));
        assert!(ls.anchor.contains(Anchor::BOTTOM));
        assert_eq!(ls.exclusive_zone, Some(px(-1.)));
        assert_eq!(
            ls.margin,
            Some((px(0.), px(0.), px(0.), px(0.))),
            "the scaffold docks flush; margins arrive with the geometry slice"
        );
    }

    #[test]
    fn exit_after_parses_positive_milliseconds_only() {
        use std::time::Duration;
        assert_eq!(
            exit_after_from_env(Some("4000")),
            Some(Duration::from_millis(4000))
        );
        assert_eq!(
            exit_after_from_env(Some("  4000\t")),
            Some(Duration::from_secs(4))
        );
        // Unset, empty, non-numeric, and non-positive all mean "run
        // like a real app" — zero especially, so a stray `=0` can't
        // quit at boot.
        assert_eq!(exit_after_from_env(None), None);
        assert_eq!(exit_after_from_env(Some("")), None);
        assert_eq!(exit_after_from_env(Some("soon")), None);
        assert_eq!(exit_after_from_env(Some("0")), None);
        assert_eq!(exit_after_from_env(Some("-5")), None);
    }
}
