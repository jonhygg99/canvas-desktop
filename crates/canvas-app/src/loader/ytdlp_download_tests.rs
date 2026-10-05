use super::*;
use std::io::{BufRead, BufReader, Write};

#[test]
fn a_real_local_download_reports_progress_and_returns_a_playable_clip() {
    let Some(ytdlp) = api::ytdlp_path() else {
        return;
    };
    let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    let status = canvas_io::media_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x90:rate=25",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());
    let data = std::fs::read(source).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            let Ok((mut client, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            };
            client
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(client.try_clone().unwrap());
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
            }
            let header = format!("HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
            let _ = client.write_all(header.as_bytes());
            if !request.starts_with("HEAD ") {
                let _ = client.write_all(&data);
            }
        }
    });
    let url = format!("http://{address}/sample.mp4");
    let dest = dir.path().join("downloads");
    std::fs::create_dir(&dest).unwrap();
    let request = YtdlpDownloadRequest {
        urls: vec![url.clone()],
        start: None,
        end: None,
        mute: false,
        dest,
        cancel: Arc::new(AtomicBool::new(false)),
        target: None,
    };
    let mut outcome = YtdlpItemOutcome {
        url: url.clone(),
        target: None,
        clips: Vec::new(),
        error: None,
    };
    let progress = std::cell::RefCell::new(Vec::new());
    let result = download_one(&ytdlp, &url, &request, &mut outcome, |text| {
        progress.borrow_mut().push(text)
    });
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    result.unwrap();
    assert_eq!(outcome.clips.len(), 1);
    assert!(!outcome.clips[0].title.is_empty());
    let (_, _, duration) = canvas_io::probe_video_size(&outcome.clips[0].path).unwrap();
    assert!(duration.unwrap() >= 0.9);
    assert!(progress.borrow().iter().any(|text| text.contains('%')));
}

#[test]
fn new_download_names_do_not_steal_a_partial_from_another_url() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("clip.f137.mp4.part"), [1]).unwrap();
    let target = api::reserve_clip_path(dir.path(), "clip").unwrap();
    assert_eq!(target.file_name().unwrap(), "clip (2).mp4");
}
