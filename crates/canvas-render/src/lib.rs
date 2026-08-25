//! Renderizado de la escena a vello. Sin UI: recibe un device/queue de wgpu
//! (compartido con quien presente en pantalla) y pinta a una textura.

mod blur;
mod scene;

pub use blur::{ColorParams, FxScope, SyncLayerRequest};
pub use scene::{append_document, build_scene, image_data_from_rgba, text_lines, ImageMap};

/// Dimensiones y color base para `render_with_base`, agrupadas para reducir
/// la firma de 8 a 6 parámetros.
pub struct RenderDims {
    pub width: u32,
    pub height: u32,
    pub base_color: vello::peniko::Color,
}

use blur::BlurEngine;
use canvas_core::LayerId;
use thiserror::Error;
use vello::peniko::color::palette;
use vello::peniko::ImageData;
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("no se pudo crear el renderizador vello: {0}")]
    CreateRenderer(String),
    #[error("fallo al renderizar la escena: {0}")]
    Render(String),
    #[error("fallo al hornear el documento: {0}")]
    Bake(String),
}

/// Renderizador del lienzo sobre un device wgpu ajeno (el de la ventana).
pub struct CanvasRenderer {
    renderer: Renderer,
    blur: BlurEngine,
}

impl CanvasRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        let renderer = Renderer::new(device, RendererOptions::default())
            .map_err(|e| RenderError::CreateRenderer(e.to_string()))?;
        Ok(Self {
            renderer,
            blur: BlurEngine::new(device),
        })
    }

    /// Sincroniza los efectos GPU (no destructivos) de una capa de imagen:
    /// filtro de color + desenfoque, encadenados. Sin efectos activos retira
    /// la textura procesada; una imagen de origen que supere el tope del
    /// atlas de vello deja en su lugar una copia reducida para la escena (ver
    /// `blur::MAX_FX_DIM`), el resto vuelve a la original. `scope` distingue
    /// a qué documento pertenece la capa (ver `FxScope`); un solo documento
    /// cargado puede usar `FxScope::default()`.
    pub fn sync_layer_effects(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scope: FxScope,
        layer: LayerId,
        source: &ImageData,
        effects: &canvas_core::Effects,
    ) {
        let renderer = &mut self.renderer;
        let outcome = self.blur.sync_layer(
            device,
            queue,
            &blur::passes::SyncLayerRequest {
                scope,
                layer,
                source,
                color: ColorParams::from(effects),
                radius: effects.blur_radius,
            },
            &mut |texture| renderer.register_texture(texture),
        );
        match outcome {
            blur::FxSync::Unchanged => {}
            // Re-horneado: la textura registrada cambió de contenido sin
            // volver a registrarse — sin esto vello sigue usando la copia
            // que hizo en su atlas de imágenes la primera vez.
            blur::FxSync::Rebaked(image) => renderer.mark_override_image_dirty(&image),
            blur::FxSync::Removed(image) => {
                renderer.override_image(&image, None);
            }
        }
    }

    /// Imágenes sustitutas (procesadas) por capa de `scope`, para `build_scene`.
    pub fn blur_overrides(&self, scope: FxScope) -> std::collections::HashMap<LayerId, ImageData> {
        self.blur.overrides(scope)
    }

    /// Libera las texturas de efectos de `scope` (un lienzo descargado de la
    /// baraja): las retira de la caché Y las des-registra de vello. Sin lo
    /// segundo, el atlas de imágenes las mantendría vivas en GPU
    /// indefinidamente aunque el documento ya no esté cargado.
    pub fn forget_scope(&mut self, scope: FxScope) {
        for image in self.blur.forget_scope(scope) {
            self.renderer.override_image(&image, None);
        }
    }

    /// Crea una textura destino compatible con vello (`Rgba8Unorm` +
    /// `STORAGE_BINDING`) que además puede muestrearse desde la UI.
    pub fn create_target_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    pub fn render_to_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.render_with_base(
            device,
            queue,
            scene,
            target,
            RenderDims {
                width,
                height,
                base_color: palette::css::DIM_GRAY,
            },
        )
    }

    fn render_with_base(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        target: &wgpu::TextureView,
        render_dims: RenderDims,
    ) -> Result<(), RenderError> {
        let RenderDims {
            width,
            height,
            base_color,
        } = render_dims;
        self.renderer
            .render_to_texture(
                device,
                queue,
                scene,
                target,
                &RenderParams {
                    base_color,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| RenderError::Render(e.to_string()))
    }

    /// Hornea la página a un mapa de bits RGBA8 (aplana capas y efectos).
    /// Es la ruta de guardado/exportación; el desenfoque no destructivo se
    /// aplica aquí de verdad. Devuelve `(rgba, ancho, alto)`. `scope`
    /// distingue a qué documento pertenecen las capas (ver `FxScope`): sin
    /// él, hornear un lienzo de la baraja distinto del activo contaminaría
    /// los efectos de este último.
    pub fn bake_page(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scope: FxScope,
        doc: &canvas_core::Document,
        images: &ImageMap,
        scale: f64,
    ) -> Result<(Vec<u8>, u32, u32), RenderError> {
        let page = doc.page().map_err(|e| RenderError::Bake(e.to_string()))?;
        let width = (page.width * scale).round().max(1.0) as u32;
        let height = (page.height * scale).round().max(1.0) as u32;

        // Asegura las texturas de efectos de todas las capas.
        for layer in &page.layers {
            if let Some(source) = images.get(&layer.id) {
                self.sync_layer_effects(device, queue, scope, layer.id, source, &layer.effects);
            }
        }

        let blurred = self.blur_overrides(scope);
        let scene = build_scene(
            doc,
            images,
            &blurred,
            vello::kurbo::Affine::scale(scale),
            false,
        );

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bake target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        self.render_with_base(
            device,
            queue,
            &scene,
            &view,
            RenderDims {
                width,
                height,
                base_color: vello::peniko::Color::TRANSPARENT,
            },
        )?;

        let rgba = read_texture_rgba(device, queue, &target, width, height)?;
        Ok((rgba, width, height))
    }
}

/// Copia una textura RGBA8 a CPU (con el padding de filas de wgpu deshecho).
fn read_texture_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let padded_row = (width * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bake readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bake copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| RenderError::Bake(format!("esperando a la GPU: {e:?}")))?;
    rx.recv()
        .map_err(|_| RenderError::Bake("el mapeo del buffer no respondió".into()))?
        .map_err(|e| RenderError::Bake(format!("mapeo de lectura falló: {e:?}")))?;

    let data = slice.get_mapped_range();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * padded_row) as usize;
        rgba.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    drop(data);
    buffer.unmap();
    Ok(rgba)
}
