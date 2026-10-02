use super::*;

#[cfg(windows)]
#[test]
fn cancellation_interrupts_a_blocked_frame_read() {
    let mut child = media_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stream = VideoFrameStream {
        child: Arc::new(Mutex::new(child)),
        stdout,
        path: PathBuf::from("blocked-test.mp4"),
        size: (1, 1),
        frame_bytes: 4,
    };
    let cancel = stream.cancellation();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tx.send(stream.next_frame()).unwrap();
    });
    assert!(rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
    cancel.cancel();
    assert!(rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap()
        .is_err());
}
