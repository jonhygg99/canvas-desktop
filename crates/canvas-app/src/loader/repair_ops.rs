//! Reparación de PNG con checksum roto pedida desde la UI: clasifica,
//! repara y SUSTITUYE el archivo en su mismo nombre (la corrupta va a la
//! papelera del sistema, recuperable). El resultado viaja como
//! `AppMsg::PngRepaired` / `AppMsg::RepairFailed`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use eframe::egui;

use super::AppMsg;

/// Hilo de reparación: solo los `ChecksumMismatch` llegan a `repair_png`
/// (lo demás —vacío, truncado— no tiene arreglo y falla rápido con su
/// tipo para que la UI explique en vez de intentarlo a ciegas).
pub fn spawn_repair_png(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let failed = |kind: canvas_io::CorruptionKind, message: String| {
            let _ = tx.send(AppMsg::RepairFailed {
                original: path.clone(),
                kind,
                message,
            });
            ctx.request_repaint();
        };
        let kind = canvas_io::classify(&path);
        if !matches!(kind, canvas_io::CorruptionKind::ChecksumMismatch) {
            failed(
                kind,
                format!("{} cannot be repaired automatically", kind.describe()),
            );
            return;
        }
        let bytes = match canvas_io::repair_png(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                failed(canvas_io::CorruptionKind::ChecksumMismatch, e.to_string());
                return;
            }
        };
        match replace_original_with_repaired(&path, &bytes) {
            Ok(()) => {
                tracing::info!(repaired = %path.display(), "png reparado en su sitio");
                let _ = tx.send(AppMsg::PngRepaired {
                    original: path.clone(),
                    repaired: path,
                });
                ctx.request_repaint();
            }
            Err(message) => {
                failed(canvas_io::CorruptionKind::ChecksumMismatch, message);
            }
        }
    });
}

/// Sustituye el archivo corrupto por los bytes reparados CONSERVANDO su
/// nombre: escribe un temporal, manda el original a la papelera del
/// sistema (recuperable) y renombra el temporal a su sitio. Si algo falla
/// a mitad, el original sigue en la papelera y el temporal huérfano se
/// nombra en el error — nunca se pierde en silencio.
fn replace_original_with_repaired(original: &Path, bytes: &[u8]) -> Result<(), String> {
    let (folder, stem, ext) = repaired_target(original)
        .ok_or_else(|| "could not derive a repaired file name".to_owned())?;
    let temp = canvas_io::reserve_unique_path(&folder, &stem, &ext).map_err(|e| e.to_string())?;
    if let Err(source) = std::fs::write(&temp, bytes) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("could not write \"{}\": {source}", temp.display()));
    }
    if let Err(e) = trash::delete(original) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "could not move the damaged file to the recycle bin: {e}"
        ));
    }
    if let Err(source) = std::fs::rename(&temp, original) {
        return Err(format!(
            "repaired file kept at \"{}\" (rename failed: {source}); the damaged original is in the recycle bin",
            temp.display()
        ));
    }
    Ok(())
}

/// Qué hacer solo con un archivo dañado cuando los automáticos están
/// activos. Pura y testeable: la decisión no toca disco ni la UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoRepairAction {
    Repair,
    Quarantine,
}

/// Acción automática por tipo y ajustes (`None` = no hacer nada). Solo
/// `ChecksumMismatch` se repara; vacíos y truncados solo se apartan.
pub fn auto_action_for(
    kind: canvas_io::CorruptionKind,
    auto_repair_png: bool,
    quarantine_unreadable: bool,
) -> Option<AutoRepairAction> {
    use canvas_io::CorruptionKind as Kind;
    match kind {
        Kind::ChecksumMismatch if auto_repair_png => Some(AutoRepairAction::Repair),
        Kind::Empty | Kind::Truncated if quarantine_unreadable => {
            Some(AutoRepairAction::Quarantine)
        }
        _ => None,
    }
}

/// Carpeta, stem y extensión del temporal de reparación (`51.png` →
/// `51-reparado.png`, con sufijo si colisiona). `None` si la ruta no tiene
/// nombre o extensión.
fn repaired_target(path: &Path) -> Option<(PathBuf, String, String)> {
    let folder = path.parent().map(PathBuf::from)?;
    let stem = path.file_stem()?.to_str()?;
    let ext = path.extension()?.to_str()?;
    Some((folder, format!("{stem}-reparado"), ext.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repaired_sibling_appends_reparado() {
        let (folder, stem, ext) =
            repaired_target(Path::new("D:/fotos/Gloria Trevi/51.png")).expect("nombre");
        assert_eq!(stem, "51-reparado");
        assert_eq!(ext, "png");
        assert!(folder.ends_with("Gloria Trevi"));
    }

    #[test]
    fn repaired_sibling_needs_stem_and_extension() {
        assert!(repaired_target(Path::new("D:/fotos/51")).is_none());
    }

    #[test]
    fn replace_keeps_the_name_and_repairs_the_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = dir.path().join("foto.png");
        std::fs::write(&original, b"corrupto").expect("sembrar original");
        replace_original_with_repaired(&original, b"reparado").expect("sustituir");
        assert_eq!(std::fs::read(&original).expect("leer"), b"reparado");
        assert!(
            !dir.path().join("foto-reparado.png").exists(),
            "el temporal se renombró, no queda suelto"
        );
    }

    #[test]
    fn auto_action_follows_kind_and_settings() {
        use canvas_io::CorruptionKind as Kind;
        // Todo apagado: nunca nada automático (defecto seguro).
        for kind in [
            Kind::Healthy,
            Kind::Empty,
            Kind::Truncated,
            Kind::ChecksumMismatch,
            Kind::Undecodable,
            Kind::Unsupported,
        ] {
            assert_eq!(auto_action_for(kind, false, false), None);
        }
        // Reparar solo checksums; cuarentena solo vacíos/truncados.
        assert_eq!(
            auto_action_for(Kind::ChecksumMismatch, true, false),
            Some(AutoRepairAction::Repair)
        );
        assert_eq!(
            auto_action_for(Kind::Empty, false, true),
            Some(AutoRepairAction::Quarantine)
        );
        assert_eq!(
            auto_action_for(Kind::Truncated, false, true),
            Some(AutoRepairAction::Quarantine)
        );
        assert_eq!(auto_action_for(Kind::Empty, true, false), None);
        assert_eq!(auto_action_for(Kind::ChecksumMismatch, false, true), None);
        assert_eq!(auto_action_for(Kind::Undecodable, true, true), None);
    }
}
