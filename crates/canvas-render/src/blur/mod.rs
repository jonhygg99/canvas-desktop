//! Efectos GPU por capa, no destructivos: filtro de color (una pasada) y
//! desenfoque gaussiano (dos pasadas, horizontal y vertical), encadenados
//! color -> blur. La imagen original no se toca; la textura procesada se
//! registra en vello y la escena la usa en su lugar.

use std::collections::HashMap;

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

mod engine;
mod params;
pub mod passes;
mod sync;

pub use params::ColorParams;
pub use sync::SyncLayerRequest;

/// Identifica a QUÉ documento pertenece una capa, para la caché de efectos
/// GPU. Los `LayerId` empiezan en 1 en cada `Document`, así que sin este
/// prefijo la capa 1 del lienzo A y la capa 1 del lienzo B compartirían
/// entrada de caché y se pisarían la textura procesada — un caso real en
/// cuanto haya más de un documento cargado a la vez (baraja del editor).
/// `Default` es la ranura de un único documento (guardado, exportación,
/// ejemplos headless): no necesitan distinguir ámbitos.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct FxScope(pub u64);

/// Lado mayor máximo (en píxeles) con el que una imagen entra en la GPU.
///
/// El atlas de imágenes de vello es CUADRADO con tope duro de 8192 y, cuando
/// una imagen no cabe, la descarta en silencio (`resolve_pending_images`
/// pone su `xy` a `None`: "this image isn't rendered"). Con el tope de 8192,
/// dos imágenes de 3072×4096 llenan el atlas y una tercera (o el horneado
/// compartiendo el atlas con el preview en vivo) se descarta — el lienzo o
/// el PNG guardado salen con la banda de blur plana o la capa ausente (fotos
/// verticales de teléfono ≥ 4030 px de alto).
///
/// Reducir a este tope ANTES de procesar/registrar mantiene todo dentro del
/// atlas y las texturas de efectos pequeñas (~12 MB por capa en vez de
/// ~200 MB a 3072×4096). Es indistinguible en pantalla: esas imágenes se
/// muestran a una fracción de su tamaño natural (el fondo desenfocado se
/// pinta al 0.6× y la foto nítida al 0.26× de la página). Las fotos ≤ 2048
/// no se tocan.
pub(super) const MAX_FX_DIM: u32 = 2048;

/// Copia reducida para la escena de una capa SIN efectos cuya imagen
/// original supera `MAX_FX_DIM`: vello no podría alojarla en su atlas y la
/// descartaría en silencio. Es un `ImageData` CPU normal (no una textura
/// registrada), así que la escena la dibuja como cualquier imagen — solo
/// cambia de qué píxeles se alimenta.
struct DisplayEntry {
    /// `Blob::id()` del ORIGINAL: si cambia (la imagen se editó o reemplazó),
    /// hay que volver a reducir.
    src_blob_id: u64,
    image: ImageData,
}

/// Reduce `source` si su lado mayor supera `max_dim`; `None` si no hace
/// falta. `thumbnail` (filtro de caja) conserva la proporción y la calidad.
/// El resultado es lo que entra en la GPU (pasadas de efectos o atlas de
/// vello), nunca los píxeles del documento.
fn capped_image(source: &ImageData, max_dim: u32) -> Option<ImageData> {
    let long = source.width.max(source.height);
    if long <= max_dim {
        return None;
    }
    let scale = f64::from(max_dim) / f64::from(long);
    let w = ((f64::from(source.width) * scale).round() as u32).max(1);
    let h = ((f64::from(source.height) * scale).round() as u32).max(1);
    let img = image::RgbaImage::from_raw(source.width, source.height, source.data.data().to_vec())?;
    let thumb = image::imageops::thumbnail(&img, w, h);
    Some(crate::image_data_from_rgba(thumb.into_raw(), w, h))
}

/// Dimensiones que tendrá la imagen de trabajo (`capped_image` o la
/// original) SIN materializar la reducción: decidir en cada frame si el
/// juego de texturas cacheado sigue siendo del tamaño correcto debe ser
/// barato, y recalcular el thumbnail no lo es. DEBE coincidir con el
/// cálculo de `capped_image`.
fn capped_dims(source: &ImageData) -> (u32, u32) {
    let long = source.width.max(source.height);
    if long <= MAX_FX_DIM {
        return (source.width, source.height);
    }
    let scale = f64::from(MAX_FX_DIM) / f64::from(long);
    (
        ((f64::from(source.width) * scale).round() as u32).max(1),
        ((f64::from(source.height) * scale).round() as u32).max(1),
    )
}

/// Texturas de una capa con efectos activos.
struct LayerFx {
    /// Imagen de trabajo subida a GPU (una vez): la original o su copia
    /// reducida si supera `MAX_FX_DIM` (ver `capped_image`).
    src: wgpu::Texture,
    /// `Blob::id()` de los píxeles que se subieron a `src`. Si cambia,
    /// los píxeles de origen cambiaron (edición, pegado, reemplazo) y hay
    /// que re-subir `src` y forzar un re-horneado — sin esto, la textura
    /// procesada seguiría mostrando los píxeles antiguos hasta que los
    /// efectos cambiaran, lo cual puede no ocurrir nunca (un slider de
    /// blur quieto + imagen editada = caché eternamente estancada).
    src_blob_id: u64,
    /// Intermedias de la cadena (color y pasada horizontal del blur).
    mid_a: wgpu::Texture,
    mid_b: wgpu::Texture,
    /// Salida final; es la que consume vello.
    out: wgpu::Texture,
    /// Handle de vello que redirige a `out`.
    image: ImageData,
    /// Tick del último `sync_layer` en el que se usó esta entrada. Lo
    /// mantiene `BlurEngine::tick` (monótono); el máximo por scope da el
    /// orden LRU de la evicción del presupuesto GPU del documento activo.
    last_used: u64,
    last: Option<(ColorParams, f32)>,
}

pub struct BlurEngine {
    blur_pipeline: wgpu::RenderPipeline,
    color_pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cache: HashMap<(FxScope, LayerId), LayerFx>,
    /// Copias reducidas para capas sin efectos demasiado grandes (ver
    /// `DisplayEntry`). Fuera de `cache` a propósito: no son texturas de
    /// efectos y no necesitan registro en vello.
    display: HashMap<(FxScope, LayerId), DisplayEntry>,
    /// Reloj monótono de uso de la caché: cada `sync_layer` con efectos
    /// anota `tick` en la entrada y avanza. Alimenta `last_used(scope)`,
    /// el orden LRU del presupuesto GPU (Task 6 del plan de memoria).
    tick: u64,
}

/// Bytes GPU de un juego de texturas de efectos de UNA capa: 4 texturas
/// (`src`, `mid_a`, `mid_b`, `out`) × `w×h` píxeles × 4 bytes/píxel
/// (`Rgba8Unorm`). Todas las texturas de un `LayerFx` comparten el tamaño
/// de la imagen de trabajo (ver `create_fx_entry`), así que el recuento es
/// exacto sin consultar cada textura. Es la unidad de contabilidad del
/// presupuesto GPU del documento activo (`total_bytes`/`bytes_in_scope`).
pub(super) fn fx_bytes(width: u32, height: u32) -> u64 {
    4 * u64::from(width) * u64::from(height) * 4
}

/// Resultado de `BlurEngine::sync_layer`, para que el llamador sepa si tiene
/// que avisar a vello. Vello copia la textura registrada a su atlas de
/// imágenes SOLO al registrarla (`Renderer::register_texture`) — mutar la
/// textura después (un re-horneado con un radio/color nuevo) no la vuelve a
/// copiar sola; sin `mark_override_image_dirty` en cada re-horneado, la
/// pantalla se queda pegada en el primer valor horneado para siempre.
pub enum FxSync {
    /// Nada que hacer: sin caché o sin cambios desde el último frame.
    Unchanged,
    /// Se (re)horneó: el llamador debe marcar esta imagen "dirty" en vello.
    Rebaked(ImageData),
    /// La capa conserva sus efectos pero su imagen de trabajo cambió de
    /// TAMAÑO y el juego completo de texturas se recreó: hay que des-registrar
    /// la textura anterior (`retired`) del renderer compartido además de
    /// marcar sucia la nueva. Sin este relevo, reescribir píxeles de otra
    /// resolución en las texturas viejas desborda su tamaño y wgpu mata la
    /// app con un error de validación (crash real 2026-08-26, pegar una foto
    /// de 2154×2170 sobre una capa que ya tenía efectos).
    Replaced {
        /// Handle de vello de la textura sustituida, a des-registrar.
        retired: ImageData,
        /// Handle de vello de la textura nueva, ya registrada.
        image: ImageData,
    },
    /// Ya no queda ningún efecto activo: el llamador debe des-registrarla.
    Removed(ImageData),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_image_leaves_images_within_the_limit_alone() {
        let small = crate::image_data_from_rgba(vec![0u8; 100 * 50 * 4], 100, 50);
        assert!(capped_image(&small, MAX_FX_DIM).is_none());
        // El tope exacto tampoco se toca.
        let edge = crate::image_data_from_rgba(vec![0u8; 2048 * 1024 * 4], 2048, 1024);
        assert!(capped_image(&edge, MAX_FX_DIM).is_none());
    }

    #[test]
    fn capped_image_downscales_preserving_aspect() {
        let mut rgba = Vec::with_capacity(3000 * 2000 * 4);
        for y in 0..2000u32 {
            for x in 0..3000u32 {
                rgba.extend_from_slice(&[
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x + y) % 256) as u8,
                    255,
                ]);
            }
        }
        let img = crate::image_data_from_rgba(rgba, 3000, 2000);
        let capped = capped_image(&img, MAX_FX_DIM).expect("debe reducirse");
        assert_eq!((capped.width, capped.height), (2048, 1365));

        // El promedio se conserva aproximadamente (el filtro de caja promedia
        // como la media global de los patrones modulares de arriba).
        let avg = |d: &[u8]| -> [u32; 3] {
            let mut acc = [0u32; 3];
            let mut n = 0u32;
            for px in d.chunks_exact(4) {
                for i in 0..3 {
                    acc[i] += u32::from(px[i]);
                }
                n += 1;
            }
            [acc[0] / n, acc[1] / n, acc[2] / n]
        };
        let src_avg = avg(img.data.data());
        let cap_avg = avg(capped.data.data());
        for i in 0..3 {
            assert!(
                (src_avg[i] as i64 - cap_avg[i] as i64).abs() < 16,
                "promedio {i}: fuente {} vs reducida {}",
                src_avg[i],
                cap_avg[i]
            );
        }
    }

    #[test]
    fn capped_dims_matches_capped_image_for_various_sizes() {
        // El criterio barato que decide el relevo de texturas debe dar
        // SIEMPRE las mismas dimensiones que la reducción real.
        for (w, h) in [
            (100u32, 50u32), // pequeña: sin tocar
            (2048, 1024),    // justo en el tope: sin tocar
            (3000, 2000),    // ancho > alto
            (2154, 2170),    // la foto del crash real → 2033×2048
            (2170, 2154),    // la misma girada
            (5000, 100),     // panorama extremo
        ] {
            let img = crate::image_data_from_rgba(vec![0u8; (w * h * 4) as usize], w, h);
            let expected = match capped_image(&img, MAX_FX_DIM) {
                Some(capped) => (capped.width, capped.height),
                None => (w, h),
            };
            assert_eq!(capped_dims(&img), expected, "discrepancia con {w}×{h}");
        }
    }

    #[test]
    fn capped_dims_reports_the_crash_photo_working_size() {
        // 2154×2170 reducida al tope da 2033×2048: exactamente el par que
        // chocaba con la textura de 2000 px del informe de crash.
        let img = crate::image_data_from_rgba(vec![0u8; 2154 * 2170 * 4], 2154, 2170);
        assert_eq!(capped_dims(&img), (2033, 2048));
    }

    #[test]
    fn fx_bytes_counts_four_rgba_textures() {
        // 4 texturas × w×h px × 4 bytes/px: el juego completo de efectos de
        // una capa (src, mid_a, mid_b, out).
        assert_eq!(fx_bytes(1, 1), 16);
        assert_eq!(fx_bytes(100, 50), 80_000);
        assert_eq!(fx_bytes(2048, 1024), 16 * 2048 * 1024);
    }
}
