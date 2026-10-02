use canvas_io::{ffmpeg_path, media_command, probe_video_size, VideoFrameStream};

#[test]
fn stream_decodes_changing_frames_and_seeks_inside_the_clip() {
    let Some(ffmpeg) = ffmpeg_path() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("moving clip.mp4");
    assert!(media_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=20",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p"
        ])
        .arg(&path)
        .output()
        .unwrap()
        .status
        .success());
    let (w, h, duration) = probe_video_size(&path).unwrap();
    assert_eq!((w, h), (64, 48));
    assert!((duration.unwrap() - 1.0).abs() < 0.1);
    let mut stream = VideoFrameStream::open(&path, 0.0, (w, h), 20).unwrap();
    let first = stream.next_frame().unwrap().unwrap();
    let next = stream.next_frame().unwrap().unwrap();
    assert_eq!(first.rgba.len(), 64 * 48 * 4);
    assert!(first.rgba != next.rgba, "the displayed pixels must advance");
    let mut seek = VideoFrameStream::open(&path, 0.5, (w, h), 20).unwrap();
    assert!(
        first.rgba != seek.next_frame().unwrap().unwrap().rgba,
        "seeking must change the frame"
    );
    assert!(VideoFrameStream::open(&path, 0.0, (0, h), 20).is_err());
}

#[test]
fn stream_reports_a_missing_source() {
    if ffmpeg_path().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut stream =
        VideoFrameStream::open(&dir.path().join("missing.mp4"), 0.0, (64, 48), 20).unwrap();
    assert!(stream.next_frame().is_err());
}

#[cfg(windows)]
#[test]
fn media_child_has_no_console() {
    if std::env::var_os("CANVAS_TEST_MEDIA_CHILD").is_some() {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        assert!(
            unsafe { GetConsoleWindow() }.is_null(),
            "media helper opened a console window"
        );
        return;
    }
    let output = media_command(std::env::current_exe().unwrap())
        .args(["--exact", "media_child_has_no_console"])
        .env("CANVAS_TEST_MEDIA_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
