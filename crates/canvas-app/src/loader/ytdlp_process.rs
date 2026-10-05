//! Salida continua y cancelacion del proceso yt-dlp y sus hijos.
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

pub(super) fn run(
    command: &mut Command,
    cancel: &Arc<AtomicBool>,
    mut line: impl FnMut(&str),
) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let (tx, rx) = mpsc::sync_channel(128);
    read_lines(child.stdout.take().expect("stdout piped"), tx.clone());
    read_lines(child.stderr.take().expect("stderr piped"), tx);
    let mut diagnostics = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            terminate_tree(&mut child);
            let _ = child.wait();
            return Err("Cancelled. Partial files are kept for retry.".into());
        }
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(text) => {
                line(&text);
                if text.starts_with("ERROR:") {
                    diagnostics = text.chars().take(500).collect();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.wait().map_err(|error| error.to_string())?;
                return if status.success() {
                    Ok(())
                } else {
                    Err(if diagnostics.is_empty() {
                        format!("Download failed ({status}).")
                    } else {
                        diagnostics
                    })
                };
            }
        }
    }
}

fn read_lines(pipe: impl Read + Send + 'static, tx: mpsc::SyncSender<String>) {
    std::thread::spawn(move || {
        // yt-dlp emite líneas de progreso pequeñas; la cola tiene un límite.
        for line in BufReader::new(pipe).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

fn terminate_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = canvas_io::media_command("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(unix)]
    {
        let _ = canvas_io::media_command("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .output();
    }
    let _ = child.kill();
}

pub(super) fn progress(line: &str) -> Option<String> {
    if line.starts_with("CANVAS_POST:") {
        return Some("Finalizing video…".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(line.strip_prefix("CANVAS_PROGRESS:")?).ok()?;
    let bytes = value.get("downloaded_bytes")?.as_f64()?;
    let total = value
        .get("total_bytes")
        .and_then(|v| v.as_f64())
        .or_else(|| value.get("total_bytes_estimate").and_then(|v| v.as_f64()));
    let percent = total
        .filter(|v| *v > 0.0)
        .map(|total| format!("{:.1}%", (bytes / total * 100.0).clamp(0.0, 100.0)))
        .unwrap_or_else(|| format!("{:.1} MB", bytes / 1_000_000.0));
    let speed = value
        .get("speed")
        .and_then(|v| v.as_f64())
        .map(|speed| format!(" · {:.1} MB/s", speed / 1_000_000.0))
        .unwrap_or_default();
    let eta = value
        .get("eta")
        .and_then(|v| v.as_f64())
        .map(|eta| format!(" · {}s remaining", eta.ceil() as u64))
        .unwrap_or_default();
    Some(format!("Downloading {percent}{speed}{eta}"))
}

#[cfg(test)]
#[path = "ytdlp_process_tests.rs"]
mod tests;
