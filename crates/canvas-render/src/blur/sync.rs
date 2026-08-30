//! Sincronización y rehorneado GPU de los efectos de una capa.

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

use super::params::{blur_params_bytes, BlurParams, ColorParams, MAX_RADIUS};
use super::{
    capped_dims, capped_image, BlurEngine, DisplayEntry, FxScope, FxSync, LayerFx, MAX_FX_DIM,
};

pub struct SyncLayerRequest<'a> {
    pub scope: FxScope,
    pub layer: LayerId,
    pub source: &'a ImageData,
    pub color: ColorParams,
    pub radius: f32,
}

pub(super) struct PassInput<'a> {
    pub pipeline: &'a wgpu::RenderPipeline,
    pub bind_layout: &'a wgpu::BindGroupLayout,
    pub sampler: &'a wgpu::Sampler,
    pub from: &'a wgpu::Texture,
    pub to: &'a wgpu::Texture,
    pub params: &'a [u8],
}

impl BlurEngine {
    fn sync_display(&mut self, request: &SyncLayerRequest<'_>) {
        let key = (request.scope, request.layer);
        match capped_image(request.source, MAX_FX_DIM) {
            Some(capped) => {
                let stale = self
                    .display
                    .get(&key)
                    .is_none_or(|e| e.src_blob_id != request.source.data.id());
                if stale {
                    self.display.insert(
                        key,
                        DisplayEntry {
                            src_blob_id: request.source.data.id(),
                            image: capped,
                        },
                    );
                }
            }
            None => {
                self.display.remove(&key);
            }
        }
    }

    pub fn sync_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: &SyncLayerRequest<'_>,
        register: &mut dyn FnMut(wgpu::Texture) -> ImageData,
    ) -> FxSync {
        let blur_active = request.radius > 0.0;
        let color_active = !request.color.is_identity();
        let key = (request.scope, request.layer);
        if !blur_active && !color_active {
            self.sync_display(request);
            return match self.cache.remove(&key) {
                Some(b) => FxSync::Removed(b.image),
                None => FxSync::Unchanged,
            };
        }
        self.display.remove(&key);
        let mut retired = None;
        let stale_size = self.cache.get(&key).is_some_and(|entry| {
            entry.src_blob_id != request.source.data.id()
                && capped_dims(request.source) != (entry.src.width(), entry.src.height())
        });
        if stale_size {
            if let Some(old) = self.cache.remove(&key) {
                retired = Some(old.image);
            }
            self.cache
                .insert(key, create_fx_entry(device, queue, request, register));
        }
        let entry = self
            .cache
            .entry(key)
            .or_insert_with(|| create_fx_entry(device, queue, request, register));
        // Uso de la entrada: anota el tick para el orden LRU del presupuesto
        // GPU (ver `BlurEngine::last_used`). Cada `sync_layer` es un uso — lo
        // llama la ruta de render por cada capa visible de cada frame.
        entry.last_used = self.tick;
        self.tick = self.tick.wrapping_add(1);
        let source_changed = entry.src_blob_id != request.source.data.id();
        if source_changed {
            let working = capped_or_original(request.source);
            let size = wgpu::Extent3d {
                width: working.width,
                height: working.height,
                depth_or_array_layers: 1,
            };
            queue.write_texture(
                entry.src.as_image_copy(),
                working.data.data(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * working.width),
                    rows_per_image: None,
                },
                size,
            );
            entry.src_blob_id = request.source.data.id();
        }
        if source_changed || entry.last != Some((request.color, request.radius)) {
            let radius =
                request.radius * entry.src.width() as f32 / request.source.width.max(1) as f32;
            if color_active {
                let target = if blur_active {
                    &entry.mid_a
                } else {
                    &entry.out
                };
                run_pass(
                    device,
                    queue,
                    &PassInput {
                        pipeline: &self.color_pipeline,
                        bind_layout: &self.bind_layout,
                        sampler: &self.sampler,
                        from: &entry.src,
                        to: target,
                        params: &request.color.to_bytes(),
                    },
                );
            }
            if blur_active {
                let sigma = (radius / 3.0).max(0.1);
                let taps = (radius.ceil() as i32).clamp(1, MAX_RADIUS);
                let input = if color_active {
                    &entry.mid_a
                } else {
                    &entry.src
                };
                run_pass(
                    device,
                    queue,
                    &PassInput {
                        pipeline: &self.blur_pipeline,
                        bind_layout: &self.bind_layout,
                        sampler: &self.sampler,
                        from: input,
                        to: &entry.mid_b,
                        params: &blur_params_bytes(&BlurParams {
                            dir: [1.0, 0.0],
                            sigma,
                            radius: taps,
                        }),
                    },
                );
                run_pass(
                    device,
                    queue,
                    &PassInput {
                        pipeline: &self.blur_pipeline,
                        bind_layout: &self.bind_layout,
                        sampler: &self.sampler,
                        from: &entry.mid_b,
                        to: &entry.out,
                        params: &blur_params_bytes(&BlurParams {
                            dir: [0.0, 1.0],
                            sigma,
                            radius: taps,
                        }),
                    },
                );
            }
            entry.last = Some((request.color, request.radius));
            return retired.map_or_else(
                || FxSync::Rebaked(entry.image.clone()),
                |retired| FxSync::Replaced {
                    retired,
                    image: entry.image.clone(),
                },
            );
        }
        FxSync::Unchanged
    }
}

fn capped_or_original(source: &ImageData) -> ImageData {
    capped_image(source, MAX_FX_DIM).unwrap_or_else(|| source.clone())
}

fn create_fx_entry(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    request: &SyncLayerRequest<'_>,
    register: &mut dyn FnMut(wgpu::Texture) -> ImageData,
) -> LayerFx {
    let working = capped_or_original(request.source);
    let size = wgpu::Extent3d {
        width: working.width,
        height: working.height,
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
        working.data.data(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * working.width),
            rows_per_image: None,
        },
        size,
    );
    let inter = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
    let mid_a = tex("fx mid a", inter);
    let mid_b = tex("fx mid b", inter);
    let out = tex(
        "fx out",
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let image = register(out.clone());
    LayerFx {
        src,
        src_blob_id: request.source.data.id(),
        mid_a,
        mid_b,
        out,
        image,
        // El tick real lo anota `sync_layer` al usarla; aquí un 0 provisional
        // (si algo la consultara antes del primer sync, sería el uso más
        // antiguo posible, y evictaría antes — inocuo).
        last_used: 0,
        last: None,
    }
}

pub(super) fn run_pass(device: &wgpu::Device, queue: &wgpu::Queue, input: &PassInput<'_>) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fx params"),
        size: input.params.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, input.params);
    let from_view = input.from.create_view(&Default::default());
    let to_view = input.to.create_view(&Default::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fx bind"),
        layout: input.bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&from_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(input.sampler),
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
        pass.set_pipeline(input.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}
