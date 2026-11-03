//! Global AX coordinates: origin is the primary display's top-left, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn screen_for(screens: &[Rect], anchor: Rect) -> Option<usize> {
    let (x, y) = (anchor.x + anchor.w / 2., anchor.y + anchor.h / 2.);
    screens
        .iter()
        .position(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
        .or_else(|| {
            screens
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let distance = |r: &Rect| {
                        (x - x.clamp(r.x, r.x + r.w)).powi(2)
                            + (y - y.clamp(r.y, r.y + r.h)).powi(2)
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .map(|(i, _)| i)
        })
}

pub fn icon_rect(screen: Rect, anchor: Rect, size: f64, gap: f64, margin: f64) -> Rect {
    let x = (anchor.x + anchor.w - size).clamp(
        screen.x + margin,
        (screen.x + screen.w - size - margin).max(screen.x + margin),
    );
    let above = anchor.y - size - gap;
    let y = if above >= screen.y + margin {
        above
    } else {
        anchor.y + anchor.h + gap
    };
    let y = y.clamp(
        screen.y + margin,
        (screen.y + screen.h - size - margin).max(screen.y + margin),
    );
    Rect {
        x,
        y,
        w: size,
        h: size,
    }
}

pub fn cocoa_y(primary_height: f64, global_top: f64) -> f64 {
    primary_height - global_top
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_screen_does_not_clamp_the_icon_onto_the_laptop() {
        let screens = [
            Rect {
                x: 0.,
                y: 0.,
                w: 1512.,
                h: 982.,
            },
            Rect {
                x: 1512.,
                y: -98.,
                w: 1920.,
                h: 1080.,
            },
        ];
        let selection = Rect {
            x: 2592.,
            y: 376.,
            w: 24.,
            h: 18.,
        };
        let index = screen_for(&screens, selection).unwrap();
        assert_eq!(index, 1);
        let r = icon_rect(screens[index], selection, 24., 4., 8.);
        assert_eq!(r.x, 2592.);
        assert_eq!(r.y, 348.);
        assert!(r.x >= 1512. && r.x + r.w <= 3432.);
        assert_eq!(cocoa_y(982., r.y), 634.);
    }
    #[test]
    fn negative_origins_and_all_screen_edges_are_respected() {
        for s in [
            Rect {
                x: -1920.,
                y: 0.,
                w: 1920.,
                h: 1080.,
            },
            Rect {
                x: 0.,
                y: -1080.,
                w: 1920.,
                h: 1080.,
            },
            Rect {
                x: 0.,
                y: 982.,
                w: 1920.,
                h: 1080.,
            },
        ] {
            for (x, y) in [(s.x, s.y), (s.x + s.w - 1., s.y + s.h - 1.)] {
                let a = Rect { x, y, w: 1., h: 1. };
                assert_eq!(screen_for(&[s], a), Some(0));
                let r = icon_rect(s, a, 24., 4., 8.);
                assert!(r.x >= s.x + 8. && r.x + r.w <= s.x + s.w - 8.);
                assert!(r.y >= s.y + 8. && r.y + r.h <= s.y + s.h - 8.);
            }
        }
    }
    #[test]
    fn removed_screen_anchor_uses_nearest_remaining_display() {
        let s = Rect {
            x: 0.,
            y: 0.,
            w: 1512.,
            h: 982.,
        };
        let a = Rect {
            x: 2600.,
            y: 300.,
            w: 10.,
            h: 18.,
        };
        assert_eq!(screen_for(&[s], a), Some(0));
        let r = icon_rect(s, a, 24., 4., 8.);
        assert_eq!(r.x, 1480.);
        assert_eq!(screen_for(&[], a), None);
    }
}
