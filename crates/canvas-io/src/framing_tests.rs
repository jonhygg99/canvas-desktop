use super::*;

#[test]
fn video_trim_roundtrips_atomically_in_the_original_assets_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let asset = dir.path().join("clip.mp4");
    std::fs::write(&asset, b"original-video").unwrap();
    let trim = VideoTrim {
        start_s: 2.0,
        end_s: 8.0,
    };
    let path = write_framing_with_trim(&asset, Framing::default(), Some(trim)).unwrap();
    assert_eq!(
        read_framing_with_trim(&asset).unwrap(),
        Some((Framing::default(), Some(trim)))
    );
    assert_eq!(read_framing(&asset).unwrap(), Some(Framing::default()));
    let before = std::fs::read(&path).unwrap();
    for trim in [
        VideoTrim {
            start_s: 8.0,
            end_s: 2.0,
        },
        VideoTrim {
            start_s: f64::NAN,
            end_s: 8.0,
        },
    ] {
        assert!(write_framing_with_trim(&asset, Framing::default(), Some(trim)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    write_framing(
        &asset,
        Framing {
            x_pct: 24.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        read_framing_with_trim(&asset).unwrap().unwrap().1,
        Some(trim)
    );
    assert_eq!(std::fs::read(&asset).unwrap(), b"original-video");
}

#[test]
fn portable_sidecar_preserves_source_and_distinguishes_bad_data() {
    let dir = tempfile::tempdir().unwrap();
    let asset = dir.path().join("diseño.png");
    std::fs::write(&asset, b"original").unwrap();
    assert_eq!(read_framing(&asset).unwrap(), None);
    let f = Framing {
        x_pct: 12.0,
        y_pct: -20.0,
        scale_pct: 75,
    };
    let path = write_framing(&asset, f).unwrap();
    assert_eq!(path, dir.path().join(".framing/diseño.png.json"));
    assert_eq!(read_framing(&asset).unwrap(), Some(f));
    let before = std::fs::read(&path).unwrap();
    assert!(write_framing(&asset, Framing { scale_pct: 0, ..f }).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, br#"{"schemaVersion":2,"width":1080,"height":1920,"framing":{"xPct":0,"yPct":0,"scalePct":100}}"#).unwrap();
    assert!(read_framing(&asset).is_err());
    assert_eq!(std::fs::read(&asset).unwrap(), b"original");
}

#[test]
fn sidecar_rejects_wrong_dimensions_corrupt_or_oversized_json() {
    let dir = tempfile::tempdir().unwrap();
    let asset = dir.path().join("clip.png");
    let path = write_framing(&asset, Framing::default()).unwrap();
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    json["futureField"] = serde_json::json!(true);
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(read_framing(&asset).unwrap(), Some(Framing::default()));
    json["width"] = serde_json::json!(1920);
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(read_framing(&asset).is_err());
    std::fs::write(&path, b"{").unwrap();
    assert!(read_framing(&asset).is_err());
    std::fs::write(&path, vec![b' '; 65537]).unwrap();
    assert!(read_framing(&asset).is_err());
}
