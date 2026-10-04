use super::*;

#[test]
fn landscape_cover_and_half_scale_follow_ffmpeg_overlay() {
    let p = Framing::default().placement(1920.0, 1080.0).unwrap();
    assert_eq!(
        p,
        Placement {
            x: -1166.0,
            y: 0.0,
            width: 3412.0,
            height: 1920.0
        }
    );
    let f = Framing {
        x_pct: 100.0,
        y_pct: 100.0,
        scale_pct: 50,
    };
    let p = f.placement(1920.0, 1080.0).unwrap();
    assert_eq!(
        p,
        Placement {
            x: -626.0,
            y: 960.0,
            width: 1706.0,
            height: 960.0
        }
    );
}

#[test]
fn hostile_values_and_empty_sources_are_rejected() {
    assert!(!Framing {
        x_pct: f32::NAN,
        ..Framing::default()
    }
    .is_valid());
    assert!(!Framing {
        scale_pct: 49,
        ..Framing::default()
    }
    .is_valid());
    assert!(Framing::default().placement(0.0, 1080.0).is_none());
    assert!(Framing::default()
        .placement(f64::INFINITY, 1080.0)
        .is_none());
}

#[test]
fn double_scale_and_negative_offsets_follow_ffmpeg_clamps() {
    let p = Framing {
        x_pct: -100.0,
        y_pct: -100.0,
        scale_pct: 200,
    }
    .placement(1920.0, 1080.0)
    .unwrap();
    assert_eq!(
        p,
        Placement {
            x: -1793.0,
            y: 0.0,
            width: 6826.0,
            height: 3840.0
        }
    );
    let p = Framing {
        x_pct: -100.0,
        y_pct: -100.0,
        scale_pct: 50,
    }
    .placement(1920.0, 1080.0)
    .unwrap();
    assert_eq!(p.x, 0.0);
    assert_eq!(p.y, 0.0);
}
