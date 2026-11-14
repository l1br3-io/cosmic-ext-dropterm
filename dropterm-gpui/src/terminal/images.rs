// SPDX-License-Identifier: GPL-3.0-only

//! The inline-image view half (gpui-parity Task 2): the pixels the
//! engine's [`ImageStore`] decodes become GPUI [`RenderImage`] frames
//! exactly once per id, and each covered grid cell draws its
//! normalized sub-rect of the shared image — the libcosmic widget's
//! two-pass rule (beneath-layer images under the glyphs, above-layer
//! kitty `z>=0` images over them, still below the cursor and the
//! ghost), re-stated through [`Window::paint_image`]'s
//! bounds-clip-is-the-sub-rect seat. An evicted id draws blank
//! (OQ-3): the cell stands, the handle dies with the pixels through
//! the store's removal report.

use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::term::cell::ImageCell;
use gpui::{Bounds, Pixels, Point, RenderImage, size};

/// The pane's pixels-to-handles cache: `from_rgba` mints a fresh
/// identity per call, so each id's pixels become a [`RenderImage`]
/// exactly once and the cache holds the upload from there — the
/// libcosmic `image_handles` shape (the engine store's own comment
/// names this contract).
#[derive(Default)]
pub(crate) struct ImageCache {
    handles: HashMap<u64, Arc<RenderImage>>,
}

impl ImageCache {
    /// Drops the handles whose pixels left the store (eviction,
    /// removal, clear) — the store's removal report, drained each
    /// paint.
    pub(crate) fn sync_removals(&mut self, removed: Vec<u64>) {
        for id in removed {
            self.handles.remove(&id);
        }
    }

    /// The render image behind an id: cached from the first build,
    /// `None` when the pixels are gone or the builder refuses (the
    /// evicted blank).
    pub(crate) fn handle_for(
        &mut self,
        id: u64,
        pixels: impl FnOnce() -> Option<image::RgbaImage>,
    ) -> Option<Arc<RenderImage>> {
        if let Some(handle) = self.handles.get(&id) {
            return Some(Arc::clone(handle));
        }
        let rgba = pixels()?;
        let frame = image::Frame::new(rgba);
        let handle = Arc::new(RenderImage::new([frame]));
        self.handles.insert(id, Arc::clone(&handle));
        Some(handle)
    }
}

/// The full-image rectangle a covered cell's sub-rect implies
/// (iterm-images spec; the libcosmic `image_draw_rect` port): the
/// cell's own rect rescaled by its share of the image, positioned so
/// the sub-rect lands exactly on the cell — the draw clips to the
/// cell (the clip is the sub-rect). Pure.
pub(crate) fn image_draw_bounds(cell: Bounds<Pixels>, sub: ImageCell) -> Bounds<Pixels> {
    let width = cell.size.width / (sub.x1 - sub.x0);
    let height = cell.size.height / (sub.y1 - sub.y0);
    Bounds {
        origin: Point::new(
            cell.origin.x - sub.x0 * width,
            cell.origin.y - sub.y0 * height,
        ),
        size: size(width, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    fn whole_image() -> ImageCell {
        ImageCell {
            image: 1,
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
        }
    }

    #[test]
    fn a_whole_image_cell_draws_the_image_at_its_own_rect() {
        let cell = rect(100.0, 60.0, 8.0, 16.0);
        assert_eq!(image_draw_bounds(cell, whole_image()), cell);
    }

    #[test]
    fn a_diced_cell_places_the_full_image_behind_it() {
        // The bottom-right cell of a 2x2 dicing: the full image is
        // twice the cell in each axis, offset so the cell's sub-rect
        // lands exactly on it.
        let cell = rect(100.0, 60.0, 8.0, 16.0);
        let sub = ImageCell {
            x0: 0.5,
            y0: 0.5,
            ..whole_image()
        };
        assert_eq!(image_draw_bounds(cell, sub), rect(92.0, 44.0, 16.0, 32.0));
    }

    #[test]
    fn a_cut_off_column_carries_its_true_fraction_of_the_whole() {
        // A clamped placement's cells carry their true fraction of
        // the whole; the derived full width is the cell over its
        // share.
        let cell = rect(0.0, 0.0, 8.0, 16.0);
        let sub = ImageCell {
            x1: 0.25,
            ..whole_image()
        };
        assert_eq!(image_draw_bounds(cell, sub), rect(0.0, 0.0, 32.0, 16.0));
    }

    #[test]
    fn the_cache_builds_each_id_once_and_serves_the_same_handle() {
        let mut cache = ImageCache::default();
        let pixels = image::RgbaImage::new(2, 2);
        let mut builds = 0;
        let first = cache
            .handle_for(7, || {
                builds += 1;
                Some(pixels.clone())
            })
            .expect("the first build lands");
        let second = cache
            .handle_for(7, || {
                builds += 1;
                Some(pixels.clone())
            })
            .expect("the cached handle serves");
        assert_eq!(builds, 1, "pixels become a frame exactly once");
        assert!(Arc::ptr_eq(&first, &second), "one shared handle");
        // A missing id is the evicted blank: no build, no handle.
        assert!(cache.handle_for(9, || None).is_none());
    }

    #[test]
    fn removals_drop_their_handles_and_the_next_ask_rebuilds() {
        let mut cache = ImageCache::default();
        let pixels = image::RgbaImage::new(1, 1);
        let first = cache
            .handle_for(7, || Some(pixels.clone()))
            .expect("the build lands");
        cache.sync_removals(vec![7, 99]);
        let rebuilt = cache
            .handle_for(7, || Some(pixels))
            .expect("pixels re-inserted rebuild");
        assert!(
            !Arc::ptr_eq(&first, &rebuilt),
            "the removal dropped the old handle"
        );
    }
}
