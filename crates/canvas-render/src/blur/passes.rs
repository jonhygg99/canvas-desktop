//! La cadena de pasadas GPU por capa: color -> blur horizontal -> blur
//! vertical, saltandose las que no hagan falta. Es donde de verdad se decide
//! si hay que rehornear una capa o si su textura sigue valiendo.

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

use super::params::{blur_params_bytes, BlurParams, ColorParams, MAX_RADIUS};
use super::{BlurEngine, FxScope, FxSync, LayerFx};

impl BlurEngine {
    /// Sincroniza los efectos GPU de una capa de `scope`. Ver `FxSync` para
    /// qué debe hacer el llamador con el resultado.
    ///
    /// `register` registra la textura de salida en vello y devuelve su handle
    /// (se inyecta para no acoplar este módulo al `Renderer`).
    #[allow(clippy::too_many_arguments)]
    pub fn sync_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scope: FxScope,
        layer: LayerId,
        source: &ImageData,
        color: ColorParams,
        radius: f32,
        register: &mut dyn FnMut(wgpu::Texture) -> ImageData,
    ) -> FxSync {
        let blur_active = radius > 0.0;
        let key = (scope, layer);
        if !blur_active && color.is_identity() {
            return match self.cache.remove(&key) {
                Some(b) => FxSync::Removed(b.image),
                None => FxSync::Unchanged,
            };
        }

        let entry = self.cache.entry(key).or_insert_with(|| {
            let (w, h) = (source.width, source.height);
            let size = wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            };
            let tex = |label: &str, usage: wgpu::TextureUsages| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage,
                    view_formats: &[],
                })
            };
            let src = tex(
                "fx src",
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            );
            queue.write_texture(
                src.as_image_copy(),
                source.data.data(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: None,
                },
                size,
            );
            let inter =
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
            let mid_a = tex("fx mid a", inter);
            let mid_b = tex("fx mid b", inter);
            // La salida además debe poder copiarse al atlas de vello (COPY_SRC)
            // y volver a muestrearse (pasada de color sin blur).
            let out = tex(
                "fx out",
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let image = register(out.clone());
            LayerFx {
                src,
                mid_a,
                mid_b,
                out,
                image,
                last: None,
            }
        });

        if entry.last != Some((color, radius)) {
            let color_active = !color.is_identity();
            // Cadena: color (src→mid_a) → blur H (x→mid_b) → blur V (mid_b→out);
            // sin blur, el color pinta directamente en out.
            if color_active {
                let target = if blur_active {
                    &entry.mid_a
                } else {
                    &entry.out
                };
                run_pass(
                    device,
                    queue,
                    &self.color_pipeline,
                    &self.bind_layout,
                    &self.sampler,
                    &entry.src,
                    target,
                    &color.to_bytes(),
                );
            }
            if blur_active {
                let sigma = (radius / 3.0).max(0.1);
                let taps = (radius.ceil() as i32).clamp(1, MAX_RADIUS);
                let blur_input = if color_active {
                    &entry.mid_a
                } else {
                    &entry.src
                };
                run_pass(
                    device,
                    queue,
                    &self.blur_pipeline,
                    &self.bind_layout,
                    &self.sampler,
                    blur_input,
                    &entry.mid_b,
                    &blur_params_bytes(&BlurParams {
                        dir: [1.0, 0.0],
                        sigma,
                        radius: taps,
                    }),
                );
                run_pass(
                    device,
                    queue,
                    &self.blur_pipeline,
                    &self.bind_layout,
                    &self.sampler,
                    &entry.mid_b,
                    &entry.out,
                    &blur_params_bytes(&BlurParams {
                        dir: [0.0, 1.0],
                        sigma,
                        radius: taps,
                    }),
                );
            }
            entry.last = Some((color, radius));
            return FxSync::Rebaked(entry.image.clone());
        }
        FxSync::Unchanged
    }
}

#[allow(clippy::too_many_arguments)]
fn run_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    bind_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    from: &wgpu::Texture,
    to: &wgpu::Texture,
    params: &[u8],
) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fx params"),
        size: params.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, params);

    let from_view = from.create_view(&Default::default());
    let to_view = to.create_view(&Default::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fx bind"),
        layout: bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&from_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("fx encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fx pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &to_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}
