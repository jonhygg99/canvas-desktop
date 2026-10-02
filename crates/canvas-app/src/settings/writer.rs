//! Escritor serial de ajustes con revisiones monotónicas (A10).
//!
//! Antes, cada cambio generaba un hilo con un snapshot completo: si la
//! escritura del snapshot VIEJO terminaba después de la del NUEVO, los
//! ajustes retrocedían (tema, workspaces, créditos…). Ahora hay UN solo
//! hilo escritor que procesa una cola FIFO con revisiones: asignar la
//! revisión, clonar y enviar ocurren bajo un mismo candado, así que las
//! revisiones quedan en el orden de los snapshots; el worker descarta todo
//! lo pendiente salvo lo más nuevo (coalescing de cambios rápidos) y nunca
//! escribe un snapshot viejo después de uno nuevo. `flush_settings` espera
//! la última revisión (la usa `on_exit`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use super::AppSettings;
use crate::lock::LockExt;

/// Revisión + snapshot pendientes de escribir.
type Queued = (u64, AppSettings);

struct Writer {
    tx: mpsc::Sender<Queued>,
    /// Serializa asignar-revisión + clonar + enviar: las revisiones quedan
    /// en el orden de los snapshots (sin esto, dos hilos podrían invertir
    /// el orden entre `fetch_add` y `send`).
    enqueue: Mutex<()>,
    queued: AtomicU64,
    done: Arc<AtomicU64>,
    path: Option<PathBuf>,
}

fn writer() -> &'static Writer {
    static WRITER: OnceLock<Writer> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Queued>();
        let done = Arc::new(AtomicU64::new(0));
        let path = AppSettings::settings_path();
        std::thread::spawn({
            let done = Arc::clone(&done);
            let path = path.clone();
            move || run_writer(rx, done, path)
        });
        Writer {
            tx,
            enqueue: Mutex::new(()),
            queued: AtomicU64::new(0),
            done,
            path,
        }
    })
}

/// Bucle del hilo escritor: toma el primer pendiente y descarta todo lo
/// demás salvo lo más nuevo (coalescing), escribe solo eso y sella su
/// revisión. Como el canal es FIFO y las revisiones crecen en orden, un
/// snapshot viejo nunca se escribe después de uno nuevo. Función libre
/// para poder probarla con un canal y directorio propios.
pub(super) fn run_writer(rx: mpsc::Receiver<Queued>, done: Arc<AtomicU64>, path: Option<PathBuf>) {
    while let Ok((mut rev, mut snap)) = rx.recv() {
        // Coalescing: si llegaron más cambios mientras se escribía, se
        // descarta todo salvo lo más nuevo (el canal es FIFO y las
        // revisiones crecen en orden, así que lo último es lo newest).
        while let Ok((r, s)) = rx.try_recv() {
            (rev, snap) = (r, s);
        }
        if let Some(path) = path.as_deref() {
            write_snapshot(path, &snap);
        }
        done.store(rev, Ordering::SeqCst);
    }
}

/// Encola un snapshot para escribirlo en segundo plano. La UI nunca espera
/// al disco; el orden entre snapshots queda garantizado por construcción.
pub(super) fn queue_settings(snapshot: AppSettings) {
    let w = writer();
    if w.path.is_none() {
        return;
    }
    let _guard = w.enqueue.lock_ok();
    let rev = w.queued.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = w.tx.send((rev, snapshot));
}

/// Espera (como mucho `timeout`) a que el último snapshot encolado quede
/// escrito. Lo usa `on_exit` para no perder la persistencia final. Devuelve
/// si se escribió a tiempo.
pub(crate) fn flush_settings(timeout: Duration) -> bool {
    let w = writer();
    let target = w.queued.load(Ordering::SeqCst);
    let start = Instant::now();
    while w.done.load(Ordering::SeqCst) < target {
        if start.elapsed() >= timeout {
            tracing::warn!("ajustes: la escritura final no terminó a tiempo; se sale igual");
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

/// Serializa y escribe (atómico) un snapshot en `path`. Pura en disco:
/// testeable con un directorio temporal.
pub(super) fn write_snapshot(path: &std::path::Path, snapshot: &AppSettings) {
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::warn!("no se pudo crear el directorio de ajustes: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(snapshot) {
        Ok(bytes) => {
            if let Err(e) = canvas_io::write_atomic(path, &bytes) {
                tracing::warn!("no se pudieron guardar los ajustes: {e}");
            }
        }
        Err(e) => tracing::warn!("no se pudieron serializar los ajustes: {e}"),
    }
}
