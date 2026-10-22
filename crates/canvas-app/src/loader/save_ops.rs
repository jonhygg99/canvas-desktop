//! Guardar el archivo abierto en el editor: raster (con su sidecar opcional)
//! o diseño autónomo, y los diálogos «Guardar como…» que les corresponden.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_io::{CanvasPayload, ImageMetadata};
use eframe::egui;

use super::{AppMsg, ImageInsertDest};

/// Reserva atómicamente (`create_new`) un nombre libre en `folder` para una
/// ranura provisional que el usuario acaba de empezar a editar: con `stem`
/// (slug de la carpeta) reserva `{stem}-NN.{ext}`; sin él, el `N.{ext}`
/// heredado. En un hilo por disciplina: en una unidad de red con cientos
/// de lienzos numerados ya creados, el bucle de reserva sí se nota, y la UI
/// no espera nunca al disco.
pub fn spawn_reserve_canvas_path(
    folder: PathBuf,
    stem: Option<String>,
    slot: u64,
    ext: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = stem.map_or_else(
            || canvas_io::reserve_numbered_path(&folder, &ext),
            |stem| canvas_io::reserve_stem_numbered_path(&folder, &stem, &ext),
        );
        let _ = tx.send(AppMsg::CanvasPathReserved {
            folder,
            slot,
            result,
        });
        ctx.request_repaint();
    });
}

/// Diálogo «Guardar como…» para un diseño: un único filtro `.canvas`, a
/// diferencia de `spawn_pick_save_path` que ofrece los cinco formatos
/// rasterizables de Guardar.
pub fn spawn_pick_design_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    let suffix = format!(".{}", canvas_io::CANVAS_EXTENSION);
    let suggested = suggested.map(|name| {
        if name.to_ascii_lowercase().ends_with(&suffix) {
            name
        } else {
            format!("{name}{suffix}")
        }
    });
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save design as…")
            .add_filter("Canvas design", &[canvas_io::CANVAS_EXTENSION]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}

/// Datos que `spawn_save` necesita para codificar y escribir una imagen
/// en un hilo de trabajo. Agrupa los 6 parámetros de píxeles/formato que
/// siempre van juntos, reduciendo la firma de 10 a 5.
pub struct SaveInput {
    pub path: PathBuf,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub jpeg_quality: u8,
    pub metadata: Option<ImageMetadata>,
    pub new_source: bool,
    pub sidecar: Option<CanvasPayload>,
    /// Ranura dueña de esta escritura (no siempre la activa: la cola de
    /// escritura guarda también lienzos de fondo) y captura del historial con
    /// la que se lanzó, para no marcar como guardadas las ediciones que
    /// lleguen durante la escritura.
    pub ticket: SaveTicket,
}

/// A quién pertenece una escritura y en qué revisión se lanzó. Viaja de ida
/// (petición) y de vuelta (respuesta) para que el resultado se aplique a SU
/// ranura aunque el usuario haya cambiado de lienzo entre medias.
#[derive(Debug, Clone, Copy)]
pub struct SaveTicket {
    pub target: ImageInsertDest,
    pub capture: (usize, u64),
}

/// Codifica y escribe (atómico) en un hilo de trabajo; el RGBA ya viene
/// horneado de la GPU.
pub fn spawn_save(input: SaveInput, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let SaveInput {
            path,
            rgba,
            width,
            height,
            jpeg_quality,
            metadata,
            new_source,
            sidecar,
            ticket,
        } = input;
        if let Some(payload) = &sidecar {
            if let Err(error) = payload.validate_for_save(&path) {
                let _ = tx.send(AppMsg::Saved {
                    path,
                    result: Err(error),
                    new_source,
                    ticket,
                });
                ctx.request_repaint();
                return;
            }
        }
        // La miniatura embebida del sidecar se reduce de este mismo RGBA
        // (ya horneado a tamaño completo): no hace falta una segunda pasada
        // de GPU solo para la celda de la galería.
        let preview = sidecar
            .is_some()
            .then(|| canvas_io::make_preview(&rgba, width, height))
            .flatten();
        // `save_rgba` devuelve los bytes EXACTOS que quedaron en disco (ya
        // con metadatos reinsertados): el hash del sidecar se calcula sobre
        // eso directamente, sin releer el archivo — en un PNG de 30 MP eso
        // ahorra decenas de MB de I/O en cada `Ctrl+S`.
        let save_result =
            canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, metadata.as_ref());
        let result = match save_result {
            Ok(bytes) => match sidecar {
                Some(mut payload) => {
                    payload.preview = preview;
                    // A03: un fallo del sidecar NO es un guardado correcto.
                    // Antes se registraba como warning y se devolvía
                    // `Ok(())`, así que la UI marcaba el historial como
                    // guardado y podía cerrar/navegar con las capas
                    // editables perdidas. Ahora viaja como error explícito
                    // de guardado parcial: la imagen quedó en disco pero
                    // el documento sigue pendiente en memoria.
                    if let Err(e) = canvas_io::write_sidecar(&path, &bytes, &payload) {
                        Err(canvas_io::IoError::Message {
                                message: format!(
                                    "Image saved to \"{}\" but editable layers could not be written: {e}. The document is still marked as unsaved; fix the sidecar location and save again.",
                                    path.display(),
                                ),
                            })
                    } else {
                        Ok(())
                    }
                }
                // Sidecar desactivado: retira el que hubiera para no
                // dejar uno obsoleto que luego avise de hash cambiado.
                None => {
                    if let Err(e) = canvas_io::delete_sidecar(&path) {
                        // Un sidecar que NO se pudo borrar deja las capas
                        // editables en disco: es un guardado parcial, no un
                        // éxito — mismo trato que el fallo de `write_sidecar`.
                        Err(canvas_io::IoError::Message {
                            message: format!(
                                "Image saved to \"{}\" but its old editable layers could not be removed: {e}. The document is still marked as unsaved.",
                                path.display()
                            ),
                        })
                    } else {
                        Ok(())
                    }
                }
            },
            Err(e) => Err(e),
        };
        let _ = tx.send(AppMsg::Saved {
            path,
            result,
            new_source,
            ticket,
        });
        ctx.request_repaint();
    });
}

/// Nombre raster que propone el diálogo: el lienzo no sabe guardar ni SVG
/// ni vídeo como tal — propone el mismo nombre en `.png` (póster) en vez
/// del formato que el guardado rechazaría tras hornear. Pura (testeable).
fn raster_suggestion(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".svg") {
        return format!("{}.png", &name[..name.len() - 4]);
    }
    let path = std::path::Path::new(name);
    if canvas_io::is_video_file(path) {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_owned());
        return format!("{stem}.png");
    }
    name.to_owned()
}

/// Nombre que propone el diálogo de exportar tramo: el clip recortado no es
/// el original, así que se sugiere `<origen>-trim.mp4` (si ya acaba en
/// `-trim`, se respeta). Pura (testeable).
pub fn video_export_suggestion(name: &str) -> String {
    let stem = std::path::Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_owned());
    if stem.to_ascii_lowercase().ends_with("-trim") {
        format!("{stem}.mp4")
    } else {
        format!("{stem}-trim.mp4")
    }
}

/// Diálogo «Guardar como…» para un lienzo de vídeo: lo que se guarda es el
/// tramo recortado (In/Out), nunca el mp4 original, así que el diálogo filtra
/// contenedores de vídeo y propone `<origen>-trim.mp4`. La ruta elegida
/// vuelve por el mismo `SaveAsPicked` de siempre: la extensión final decide
/// la rama en `handle_save`.
pub fn spawn_pick_video_export_path(
    suggested: Option<String>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    let suggested = suggested.map(|name| video_export_suggestion(&name));
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save trimmed clip as…")
            .add_filter("MP4 video", &["mp4"])
            .add_filter("Video", &["mp4", "mov", "mkv", "webm"]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}

pub fn spawn_pick_save_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    let suggested = suggested.map(|name| raster_suggestion(&name));
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save as…")
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("WebP", &["webp"])
            .add_filter("GIF", &["gif"])
            .add_filter("BMP", &["bmp"]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_export_dialog_suggests_a_trim_name() {
        // Lo que se guarda es el recorte, no el original: el nombre lo dice.
        assert_eq!(video_export_suggestion("1.mp4"), "1-trim.mp4");
        assert_eq!(
            video_export_suggestion("G:/vids/mi clip.MP4"),
            "mi clip-trim.mp4"
        );
        assert_eq!(video_export_suggestion("1-trim.mp4"), "1-trim.mp4");
        assert_eq!(video_export_suggestion("foto.png"), "foto-trim.mp4");
    }

    #[test]
    fn save_dialog_suggests_a_poster_for_unsaveable_sources() {
        // El SVG ya se reescribía; el vídeo caía al diálogo como `1.mp4` y el
        // guardado fallaba tras hornear (`UnsupportedFormat`).
        assert_eq!(raster_suggestion("clip.svg"), "clip.png");
        assert_eq!(raster_suggestion("1.mp4"), "1.png");
        assert_eq!(raster_suggestion("G:/vids/1.MP4"), "1.png");
        assert_eq!(raster_suggestion("foto.png"), "foto.png");
        assert_eq!(raster_suggestion("foto.jpg"), "foto.jpg");
    }

    #[test]
    fn invalid_sidecar_never_rewrites_the_raster() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original.png");
        std::fs::write(&path, b"original image").unwrap();
        let payload = CanvasPayload {
            document: canvas_core::Document::new(20_001.0, 1.0),
            images: vec![(1, vec![255; 20_001 * 4], 20_001, 1)],
            background_layer: None,
            preview: None,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_save(
            SaveInput {
                path: path.clone(),
                rgba: vec![10, 20, 30, 255],
                width: 1,
                height: 1,
                jpeg_quality: 92,
                metadata: None,
                new_source: false,
                sidecar: Some(payload),
                ticket: SaveTicket {
                    target: ImageInsertDest {
                        generation: 0,
                        slot_id: 0,
                    },
                    capture: (0, 0),
                },
            },
            tx,
            egui::Context::default(),
        );
        let AppMsg::Saved { result, .. } =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
        else {
            panic!("save completion");
        };
        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original image");
    }
}
