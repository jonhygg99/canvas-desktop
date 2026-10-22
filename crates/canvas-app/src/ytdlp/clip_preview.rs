//! Miniatura y duración de tarjetas, con un único trabajo fuera de egui.
//!
//! La identidad es la huella del archivo, no su ruta: los nombres del
//! staging se reciclan (`clip.mp4`, `clip (2).mp4`…), así que borrar el
//! primero y descargar el segundo deja la MISMA ruta con OTRO clip. Sin
//! huella, el worker del borrado entrega tarde y su miniatura queda
//! archivada como la del nuevo para siempre (nunca se re-pide).
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

/// Huella del archivo (mtime + tamaño): `None` si ya no está en disco.
type Fingerprint = Option<(SystemTime, u64)>;

/// Lado mayor de la textura de una tarjeta (presupuesto de memoria fijo).
/// El tamaño lo calcula `canvas_io::fit_within`, que conserva la FORMA del
/// vídeo: antes se pedía `192×108` fijo y el clip se aplastaba —
/// `image::imageops::thumbnail` de la versión del workspace redimensiona
/// EXACTO al tamaño pedido (no conserva la proporción, pese al nombre), así
/// que un 9:16 acababa con la misma forma que un 16:9.
const PREVIEW_MAX_DIM: u32 = 192;

fn fingerprint(path: &Path) -> Fingerprint {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok().map(|mtime| (mtime, meta.len())))
}

#[derive(Default)]
pub(crate) struct ClipPreviews {
    ready: HashMap<PathBuf, ClipPreview>,
    pending: Option<(PathBuf, Fingerprint, mpsc::Receiver<Decoded>)>,
}

pub(super) struct ClipPreview {
    pub texture: Option<egui::TextureHandle>,
    pub duration: Option<f64>,
    meta: Fingerprint,
}

struct Decoded {
    image: Option<egui::ColorImage>,
    duration: Option<f64>,
}

impl ClipPreviews {
    pub(super) fn update(&mut self, paths: &[PathBuf], ctx: &egui::Context) {
        // Fuera de la lista o con OTRO archivo bajo la misma ruta (ver
        // `fingerprint`): se poda y abajo se pide el vigente.
        self.ready
            .retain(|path, preview| paths.contains(path) && fingerprint(path) == preview.meta);
        if let Some((path, meta, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(frame) => {
                    // Solo vale si el archivo sigue siendo el que se pidió:
                    // el worker de un clip borrado puede entregar cuando su
                    // ruta ya es de otro clip. Sin esto, esa miniatura vieja
                    // quedaba archivada y la nueva no se pedía nunca.
                    if paths.contains(path) && fingerprint(path) == *meta {
                        self.ready.insert(
                            path.clone(),
                            ClipPreview {
                                duration: frame.duration,
                                texture: frame.image.map(|image| {
                                    ctx.load_texture(
                                        format!("download-card-{}", path.display()),
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    )
                                }),
                                meta: *meta,
                            },
                        );
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Terminal (no reintenta en bucle): solo para tarjetas
                    // que siguen visibles. Con la meta PEDIDA, no la viva:
                    // si la ruta ya es de otro clip, la poda la retira y se
                    // pide el vigente en vez de clavar un vacío ajeno.
                    if paths.contains(path) {
                        self.ready.insert(
                            path.clone(),
                            ClipPreview {
                                texture: None,
                                duration: None,
                                meta: *meta,
                            },
                        );
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }
        if let Some(path) = paths.iter().find(|path| !self.ready.contains_key(*path)) {
            self.request(path, ctx);
        }
    }

    pub(super) fn get(&self, path: &Path) -> Option<&ClipPreview> {
        self.ready.get(path)
    }

    fn request(&mut self, path: &Path, ctx: &egui::Context) {
        let path = path.to_owned();
        let meta = fingerprint(&path);
        let (tx, rx) = mpsc::channel();
        self.pending = Some((path.clone(), meta, rx));
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        std::thread::spawn(move || {
            let duration = canvas_io::probe_video_size(&path)
                .ok()
                .and_then(|(_, _, d)| d);
            let image = canvas_io::load_video_frame(&path, 0.0)
                .ok()
                .and_then(|image| {
                    let (tw, th) =
                        canvas_io::fit_within(image.width, image.height, PREVIEW_MAX_DIM);
                    let pixels = image::RgbaImage::from_raw(image.width, image.height, image.rgba)?;
                    let small = image::imageops::thumbnail(&pixels, tw, th);
                    Some(egui::ColorImage::from_rgba_unmultiplied(
                        [small.width() as usize, small.height() as usize],
                        small.as_raw(),
                    ))
                });
            if tx.send(Decoded { image, duration }).is_ok() {
                ctx.request_repaint_of(viewport);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `clip.mp4` con un contenido: el tamaño distinto basta para que la
    /// huella cambie aunque el mtime cayera en el mismo tick.
    fn clip(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join("clip.mp4");
        std::fs::write(&path, bytes).expect("sembrar clip");
        path
    }

    fn pending_with(previews: &mut ClipPreviews, path: &Path, meta: Fingerprint, frame: Decoded) {
        let (tx, rx) = mpsc::channel();
        tx.send(frame).expect("entregar");
        previews.pending = Some((path.to_owned(), meta, rx));
    }

    fn empty_frame() -> Decoded {
        Decoded {
            image: None,
            duration: Some(7.0),
        }
    }

    #[test]
    fn untouched_file_keeps_its_preview() {
        let dir = tempfile::tempdir().expect("temp");
        let path = clip(dir.path(), b"primer clip largo");
        let ctx = egui::Context::default();
        let mut previews = ClipPreviews::default();
        pending_with(&mut previews, &path, fingerprint(&path), empty_frame());
        previews.update(std::slice::from_ref(&path), &ctx);
        let preview = previews.get(&path).expect("vale");
        assert_eq!(preview.duration, Some(7.0));
    }

    /// El caso del informe: se borra el primero y el segundo descarga con
    /// el MISMO nombre antes de que el worker del primero entregue. Esa
    /// miniatura vieja se suelta; la del vigente se pide de nuevo.
    #[test]
    fn replaced_file_drops_the_in_flight_preview() {
        let dir = tempfile::tempdir().expect("temp");
        let path = clip(dir.path(), b"primer clip largo");
        let ctx = egui::Context::default();
        let mut previews = ClipPreviews::default();
        let old_meta = fingerprint(&path);
        // Borrado + segundo download reciclando `clip.mp4`.
        let path = clip(dir.path(), b"segundo");
        assert_ne!(fingerprint(&path), old_meta);
        // El worker del primero entrega tarde.
        pending_with(&mut previews, &path, old_meta, empty_frame());
        previews.update(std::slice::from_ref(&path), &ctx);
        assert!(
            previews.get(&path).is_none(),
            "la miniatura del clip borrado no vale para el nuevo"
        );
    }

    /// Entrada ya archivada cuyo archivo cambió por debajo (p. ej. mensaje
    /// de borrado perdido): se poda y se re-pide en vez de enseñar lo viejo.
    #[test]
    fn replaced_file_prunes_the_settled_preview() {
        let dir = tempfile::tempdir().expect("temp");
        let path = clip(dir.path(), b"primer clip largo");
        let ctx = egui::Context::default();
        let mut previews = ClipPreviews::default();
        previews.ready.insert(
            path.clone(),
            ClipPreview {
                texture: None,
                duration: Some(7.0),
                meta: fingerprint(&path),
            },
        );
        let path = clip(dir.path(), b"segundo");
        previews.update(std::slice::from_ref(&path), &ctx);
        assert!(
            previews.get(&path).is_none(),
            "la entrada vieja no sobrevive al reemplazo"
        );
    }

    /// La textura de la tarjeta conserva la forma del vídeo: el 9:16 que se
    /// descarga no llega ya aplastado a un lienzo de 16:9 (la tarjeta decide
    /// su marco con `clip_cards::thumb_size`).
    #[test]
    fn a_vertical_frame_keeps_its_shape_in_the_card_texture() {
        let pixels = image::RgbaImage::from_pixel(1080, 1920, image::Rgba([1, 2, 3, 255]));
        let (tw, th) = canvas_io::fit_within(1080, 1920, PREVIEW_MAX_DIM);
        let small = image::imageops::thumbnail(&pixels, tw, th);
        let aspect = small.width() as f32 / small.height() as f32;
        assert!(
            (aspect - 1080.0 / 1920.0).abs() < 0.01,
            "la miniatura {0}x{1} no conserva el 9:16",
            small.width(),
            small.height()
        );
        assert!(small.height() > small.width(), "un 9:16 se ve vertical");
        assert_eq!(
            small.height(),
            PREVIEW_MAX_DIM,
            "el lado mayor sigue siendo el presupuesto"
        );
    }

    /// Formatos que la tarjeta tiene que servir sin deformar ni dispararse de
    /// memoria: el lado MAYOR siempre es 192 y la forma no cambia.
    #[test]
    fn card_textures_keep_every_format_within_the_budget() {
        let cases = [
            ((1920, 1080), (192, 108)),
            ((1080, 1920), (108, 192)),
            ((1080, 1080), (192, 192)),
            ((1080, 1350), (154, 192)),
            ((1080, 1440), (144, 192)),
        ];
        for ((w, h), expected) in cases {
            let dims = canvas_io::fit_within(w, h, PREVIEW_MAX_DIM);
            assert_eq!(dims, expected, "{w}x{h}");
            assert_eq!(dims.0.max(dims.1), PREVIEW_MAX_DIM);
            let aspect = f64::from(dims.0) / f64::from(dims.1);
            let real = f64::from(w) / f64::from(h);
            assert!(
                (aspect / real - 1.0).abs() < 0.01,
                "{w}x{h}: forma {aspect:.3} vs {real:.3}"
            );
        }
        // Un clip diminuto no se agranda (el presupuesto es un TECHO).
        assert_eq!(canvas_io::fit_within(96, 54, PREVIEW_MAX_DIM), (96, 54));
        assert_eq!(canvas_io::fit_within(0, 0, PREVIEW_MAX_DIM), (1, 1));
    }

    #[test]
    fn departed_cards_leave_no_preview_behind() {
        let dir = tempfile::tempdir().expect("temp");
        let path = clip(dir.path(), b"primer clip largo");
        let ctx = egui::Context::default();
        let mut previews = ClipPreviews::default();
        previews.ready.insert(
            path.clone(),
            ClipPreview {
                texture: None,
                duration: Some(7.0),
                meta: fingerprint(&path),
            },
        );
        previews.update(&[], &ctx);
        assert!(previews.get(&path).is_none());
    }
}
