use super::*;

#[test]
fn progress_handles_estimates_missing_eta_and_finalization() {
    assert_eq!(
        progress(
            r#"CANVAS_PROGRESS:{"downloaded_bytes":500,"total_bytes":1000,"speed":2000000,"eta":2}"#
        ),
        Some("Downloading 50.0% · 2.0 MB/s · 2s remaining".into())
    );
    assert_eq!(
        progress(r#"CANVAS_PROGRESS:{"downloaded_bytes":500,"total_bytes_estimate":1000}"#),
        Some("Downloading 50.0%".into())
    );
    assert_eq!(
        progress("CANVAS_POST:started"),
        Some("Finalizing video…".into())
    );
    assert_eq!(progress("other text"), None);
}

#[cfg(windows)]
#[test]
fn cancellation_stops_a_live_child_before_its_work_finishes() {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let start = std::time::Instant::now();
    let mut command = canvas_io::media_command("powershell.exe");
    command.args([
        "-NoProfile",
        "-Command",
        "Write-Output 'ready'; Start-Sleep -Seconds 30",
    ]);
    let result = run(&mut command, &cancel, |line| {
        if line == "ready" {
            flag.store(true, Ordering::Relaxed);
        }
    });
    assert!(result.unwrap_err().starts_with("Cancelled"));
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[cfg(windows)]
#[test]
fn streamed_lines_arrive_before_process_exit() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut command = canvas_io::media_command("powershell.exe");
    command.args([
        "-NoProfile",
        "-Command",
        "Write-Output 'first'; Start-Sleep -Milliseconds 300; Write-Output 'last'",
    ]);
    let mut received = Vec::new();
    run(&mut command, &cancel, |line| {
        received.push((line.to_owned(), std::time::Instant::now()))
    })
    .unwrap();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].0, "first");
    assert!(received[1].1.duration_since(received[0].1) >= Duration::from_millis(200));
}
