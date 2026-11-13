use super::*;

const OUTPUT: (u32, u32) = (1920, 1080);

fn vertical_of(edge: Edge) -> Anchor {
    match edge {
        Edge::Top => Anchor::TOP,
        Edge::Bottom => Anchor::BOTTOM,
    }
}

#[test]
fn full_width_keeps_both_anchors_and_zero_margins_whatever_the_alignment() {
    // At full width the alignment is moot — both anchors, zero
    // margins.
    for edge in [Edge::Top, Edge::Bottom] {
        for h_align in [HAlign::Left, HAlign::Center, HAlign::Right] {
            let geom = Geometry {
                edge,
                height_percent: 40,
                width_percent: 100,
                h_align,
            };
            let (anchor, margins) = anchor_margins(geom, OUTPUT);
            assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT | Anchor::RIGHT);
            assert_eq!(margins, Margins::default());
        }
    }
}

#[test]
fn left_alignment_drops_the_right_anchor_below_full_width() {
    for edge in [Edge::Top, Edge::Bottom] {
        let geom = Geometry {
            edge,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Left,
        };
        let (anchor, margins) = anchor_margins(geom, OUTPUT);
        assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT);
        assert_eq!(margins, Margins::default());
    }
}

#[test]
fn right_alignment_drops_the_left_anchor_below_full_width() {
    for edge in [Edge::Top, Edge::Bottom] {
        let geom = Geometry {
            edge,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Right,
        };
        let (anchor, margins) = anchor_margins(geom, OUTPUT);
        assert_eq!(anchor, vertical_of(edge) | Anchor::RIGHT);
        assert_eq!(margins, Margins::default());
    }
}

#[test]
fn center_alignment_keeps_both_anchors_with_equal_margins() {
    // 1920 * 60% = 1152 px, leaving (1920 - 1152) / 2 = 384 per side.
    for edge in [Edge::Top, Edge::Bottom] {
        let geom = Geometry {
            edge,
            height_percent: 40,
            width_percent: 60,
            h_align: HAlign::Center,
        };
        let (anchor, margins) = anchor_margins(geom, OUTPUT);
        assert_eq!(anchor, vertical_of(edge) | Anchor::LEFT | Anchor::RIGHT);
        assert_eq!(margins.left, 384);
        assert_eq!(margins.right, 384);
        assert_eq!(margins.top, 0);
        assert_eq!(margins.bottom, 0);
    }
}

#[test]
fn center_margin_split_floors_left_and_keeps_total_width_exact() {
    // 1001 * 41% = 410.41 -> 410 px, leaving 591 px of slack: 295
    // left (floor), 296 right, so left + width + right == 1001.
    let geom = Geometry {
        edge: Edge::Top,
        height_percent: 40,
        width_percent: 41,
        h_align: HAlign::Center,
    };
    let (_, margins) = anchor_margins(geom, (1001, 800));
    assert_eq!(margins.left, 295);
    assert_eq!(margins.right, 296);
    let width = surface_width(1001.0, 41);
    #[expect(
        clippy::cast_sign_loss,
        reason = "fixture margins are non-negative px by construction"
    )]
    let total_width = margins.left as u32 + width + margins.right as u32;
    assert_eq!(total_width, 1001);
}
