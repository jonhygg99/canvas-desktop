//! Renderizado de la escena a vello. Sin UI: recibe un device/queue de wgpu
//! (compartido con quien presente en pantalla) y pinta a una textura.

mod blur;
mod scene;

pub use scene::{build_scene, image_data_from_rgba, ImageMap};

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

    /// Sincroniza el desenfoque GPU (no destructivo) de una capa de imagen.
    /// Con radio 0 retira la textura desenfocada y vuelve a la original.
    pub fn sync_layer_blur(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: LayerId,
        source: &ImageData,
        radius: f32,
    ) {
        let renderer = &mut self.renderer;
        let removed = self
            .blur
            .sync_layer(device, queue, layer, source, radius, &mut |texture| {
                renderer.register_texture(texture)
            });
        if let Some(image) = removed {
            renderer.override_image(&image, None);
        }
    }

    /// Imágenes sustitutas (desenfocadas) por capa, para `build_scene`.
    pub fn blur_overrides(&self) -> std::collections::HashMap<LayerId, ImageData> {
        self.blur.overrides()
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
            width,
            height,
            palette::css::DIM_GRAY,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_with_base(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        base_color: vello::peniko::Color,
    ) -> Result<(), RenderError> {
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
    /// aplica aquí de verdad. Devuelve `(rgba, ancho, alto)`.
    pub fn bake_page(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        doc: &canvas_core::Document,
        images: &ImageMap,
        scale: f64,
    ) -> Result<(Vec<u8>, u32, u32), RenderError> {
        let page = doc.page().map_err(|e| RenderError::Bake(e.to_string()))?;
        let width = (page.width * scale).round().max(1.0) as u32;
        let height = (page.height * scale).round().max(1.0) as u32;

        // Asegura las texturas de desenfoque de todas las capas.
        for layer in &page.layers {
            if let Some(source) = images.get(&layer.id) {
                self.sync_layer_blur(device, queue, layer.id, source, layer.effects.blur_radius);
            }
        }

        let blurred = self.blur_overrides();
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
            width,
            height,
            vello::peniko::Color::TRANSPARENT,
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
