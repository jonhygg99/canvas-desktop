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

pub use params::ColorParams;
pub use passes::SyncLayerRequest;

/// Identifica a QUÉ documento pertenece una capa, para la caché de efectos
/// GPU. Los `LayerId` empiezan en 1 en cada `Document`, así que sin este
/// prefijo la capa 1 del lienzo A y la capa 1 del lienzo B compartirían
/// entrada de caché y se pisarían la textura procesada — un caso real en
/// cuanto haya más de un documento cargado a la vez (baraja del editor).
/// `Default` es la ranura de un único documento (guardado, exportación,
/// ejemplos headless): no necesitan distinguir ámbitos.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct FxScope(pub u64);

/// Texturas de una capa con efectos activos.
struct LayerFx {
    /// Imagen original subida a GPU (una vez).
    src: wgpu::Texture,
    /// Intermedias de la cadena (color y pasada horizontal del blur).
    mid_a: wgpu::Texture,
    mid_b: wgpu::Texture,
    /// Salida final; es la que consume vello.
    out: wgpu::Texture,
    /// Handle de vello que redirige a `out`.
    image: ImageData,
    last: Option<(ColorParams, f32)>,
}

pub struct BlurEngine {
    blur_pipeline: wgpu::RenderPipeline,
    color_pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cache: HashMap<(FxScope, LayerId), LayerFx>,
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
    /// Ya no queda ningún efecto activo: el llamador debe des-registrarla.
    Removed(ImageData),
}
