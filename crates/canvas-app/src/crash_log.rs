//! Registro de crashes en archivo.
//!
//! Instala un hook de pánico global que, además de encadenar con el hook
//! previo (la consola sigue viendo todo), escribe un informe completo en
//! `<data_dir>/crashes/crash-<unix>-<pid>.log`: mensaje, hilo, ubicación
//! exacta, backtrace, tiempo activo y las últimas líneas de log de la
//! sesión (buffer circular alimentado por el propio suscriptor de
//! `tracing`). Cubre todos los hilos: los panics silenciosos de los hilos
//! worker (loader/galería) también quedan registrados sin matar la UI.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing_subscriber::fmt::MakeWriter;

/// Líneas de log recientes que se conservan para el contexto del informe.
const RING_CAPACITY: usize = 500;
/// Cuántas de esas líneas se vuelcan en cada informe.
const REPORT_TAIL_LINES: usize = 400;
/// Informes que se conservan; los más antiguos se podan.
const KEEP_REPORTS: usize = 20;

static RECENT_LOGS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static SINCE_INSTALL: OnceLock<Instant> = OnceLock::new();

fn recent_logs() -> &'static Mutex<VecDeque<String>> {
    RECENT_LOGS.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING_CAPACITY)))
}

// ─── Buffer circular de logs ─────────────────────────────────────────────

/// Añade al buffer las líneas contenidas en `bytes` (un evento de tracing),
/// descartando las más viejas al superar `RING_CAPACITY`.
fn push_recent_lines(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let mut queue = recent_logs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for line in text.lines() {
        if queue.len() >= RING_CAPACITY {
            queue.pop_front();
        }
        queue.push_back(line.to_owned());
    }
}

/// Últimas `limit` líneas del buffer, en orden cronológico.
fn recent_logs_tail(limit: usize) -> String {
    let queue = recent_logs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let skip = queue.len().saturating_sub(limit);
    queue
        .iter()
        .skip(skip)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Tee stdout + buffer para tracing_subscriber ─────────────────────────

/// Writer para `tracing_subscriber` que duplica cada evento: lo escribe en
/// stdout (como haría el formato por defecto) y lo añade al buffer circular
/// que alimenta los informes de crash.
#[derive(Clone, Copy, Default)]
pub(crate) struct TeeWriter;

impl<'a> MakeWriter<'a> for TeeWriter {
    type Writer = TeeSink;

    fn make_writer(&'a self) -> Self::Writer {
        TeeSink {
            buf: Vec::new(),
            done: false,
        }
    }
}

/// Destino de un evento: acumula y vuelca al terminar (`flush`/`Drop`,
/// según qué invoque primero el suscriptor; `emit` es idempotente).
pub(crate) struct TeeSink {
    buf: Vec<u8>,
    done: bool,
}

impl TeeSink {
    fn emit(&mut self) {
        if self.done || self.buf.is_empty() {
            return;
        }
        let bytes = std::mem::take(&mut self.buf);
        self.done = true;
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let _ = lock.write_all(&bytes);
        let _ = lock.flush();
        push_recent_lines(&bytes);
    }
}

impl std::io::Write for TeeSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.emit();
        Ok(())
    }
}

impl Drop for TeeSink {
    fn drop(&mut self) {
        self.emit();
    }
}

/// Writer listo para `.with_writer(...)` en el suscriptor de `tracing`.
pub(crate) fn tee_writer() -> TeeWriter {
    TeeWriter
}

// ─── Instalación ──────────────────────────────────────────────────────────

/// Directorio real de informes: `<data_dir>/crashes` del perfil del usuario
/// (`directories`), con reserva en el directorio temporal si aquel no está
/// disponible o no se puede crear.
pub(crate) fn crash_dir() -> PathBuf {
    let primary = directories::ProjectDirs::from("com", "canvas-desktop", "Canvas Desktop")
        .map(|dirs| dirs.data_dir().join("crashes"));
    match primary {
        Some(dir) if fs::create_dir_all(&dir).is_ok() => dir,
        _ => {
            let fallback = std::env::temp_dir().join("canvas-desktop-crashes");
            let _ = fs::create_dir_all(&fallback);
            fallback
        }
    }
}

/// Instala el hook con el directorio real y poda informes antiguos.
pub(crate) fn install() {
    install_with_dir(crash_dir());
}

/// Versión parametrizable (para pruebas): registra un hook de pánico que
/// escribe el informe en `dir` y después llama al hook previo.
pub(crate) fn install_with_dir(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    prune_reports(&dir, KEEP_REPORTS);
    let _ = SINCE_INSTALL.set(Instant::now());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread_label = match thread.name() {
            Some(name) => name.to_owned(),
            None => format!("{:?}", thread.id()),
        };
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "payload opaco (Box<dyn Any>)".to_owned()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let summary = PanicSummary {
            thread_label,
            message,
            location,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        write_report_to(&dir, &summary);
        previous(info);
    }));
}

// ─── Informes ────────────────────────────────────────────────────────────

/// Datos extraídos del pánico dentro del hook (tipos planos para no
/// depender de la variante concreta de `PanicHookInfo` entre versiones).
struct PanicSummary {
    thread_label: String,
    message: String,
    location: Option<String>,
    backtrace: String,
}

/// Escribe el informe como archivo nuevo exclusivo y poda los viejos.
/// Nunca entra en pánico: si algo falla, avisa por stderr y sigue.
fn write_report_to(dir: &Path, summary: &PanicSummary) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let report = format_report(now, summary);

    let pid = std::process::id();
    for attempt in 0..50u32 {
        let name = if attempt == 0 {
            format!("crash-{now}-{pid}.log")
        } else {
            format!("crash-{now}-{pid}-{attempt}.log")
        };
        let path = dir.join(name);
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                if file.write_all(report.as_bytes()).is_err() || file.sync_all().is_err() {
                    let _ = fs::remove_file(&path);
                    eprintln!("crash_log: no se pudo escribir el informe completo");
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                eprintln!("crash_log: informe no escrito: {e}");
                break;
            }
        }
    }
    prune_reports(dir, KEEP_REPORTS);
}

fn format_report(now: u64, s: &PanicSummary) -> String {
    let uptime = SINCE_INSTALL
        .get()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    format!(
        "════ Canvas Desktop — informe de crash ════\n\
         \n\
         Fecha (UTC) : {}\n\
         Marca unix  : {now}\n\
         Versión     : {}\n\
         SO          : {} ({})\n\
         Tiempo activo: {uptime} s\n\
         \n\
         — Pánico —\n\
         Mensaje   : {}\n\
         Ubicación : {}\n\
         Hilo      : {}\n\
         \n\
         — Backtrace —\n\
         {}\n\
         \n\
         — Últimas {REPORT_TAIL_LINES} líneas de log —\n\
         {}\n",
        format_utc(now),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        s.message,
        s.location.as_deref().unwrap_or("(desconocida)"),
        s.thread_label,
        s.backtrace,
        recent_logs_tail(REPORT_TAIL_LINES),
    )
}

/// Convierte segundos UNIX a «AAAA-MM-DD HH:MM:SS» (UTC) sin dependencias:
/// algoritmo civil-from-days (Howard Hinnant, dominio público).
fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m as u32, d)
}

/// Deja solo los `keep` informes más nuevos (los nombres llevan la marca
/// unix como prefijo, así que el orden lexicográfico es cronológico).
fn prune_reports(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "log"))
        .collect();
    files.sort();
    if files.len() > keep {
        for old in &files[..files.len() - keep] {
            let _ = fs::remove_file(old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("canvas-crash-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn report_contains_message_location_thread_and_context() {
        let dir = temp_dir("informe");
        install_with_dir(dir.clone());
        let result = std::panic::catch_unwind(|| panic!("boom-de-prueba"));
        assert!(result.is_err());

        let found = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter_map(|p| {
                let content = fs::read_to_string(&p).ok()?;
                content.contains("boom-de-prueba").then_some((p, content))
            })
            .next();
        let (_, content) = found.expect("el informe con el mensaje se escribió");
        assert!(content.contains(env!("CARGO_PKG_VERSION")));
        assert!(content.contains(std::env::consts::OS));
        assert!(content.contains("Ubicación"));
        // La ubicación apunta a este mismo archivo de pruebas/módulo.
        assert!(content.contains("crash_log.rs"), "{content}");
        assert!(content.contains("Hilo"));
        assert!(content.contains("Backtrace"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_only_the_newest_reports() {
        let dir = temp_dir("poda");
        for i in 0..25u32 {
            fs::write(dir.join(format!("crash-{i:08}-42.log")), "x").unwrap();
        }
        prune_reports(&dir, KEEP_REPORTS);
        let rest: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(rest.len(), KEEP_REPORTS);
        assert!(rest.iter().any(|n| n.starts_with("crash-00000024-")));
        assert!(!rest.iter().any(|n| n.starts_with("crash-00000004-")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ring_buffer_keeps_only_capacity_lines() {
        recent_logs()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let last = RING_CAPACITY + 119;
        for i in 0..=last {
            push_recent_lines(format!("línea-{i}\n").as_bytes());
        }
        let tail = recent_logs_tail(RING_CAPACITY);
        assert!(tail.contains(&format!("línea-{last}")));
        // La primera línea ya fue descartada por el límite de capacidad.
        assert!(!tail.contains("línea-119"));
        assert!(tail.ends_with(&format!("línea-{last}")));
    }

    #[test]
    fn utc_formatter_matches_known_instants() {
        // 1970-01-01T00:00:00Z y 2026-08-26T00:00:00Z (20913 días).
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(86_400), "1970-01-02 00:00:00");
        assert_eq!(format_utc(1_787_702_400), "2026-08-26 00:00:00");
    }
}
