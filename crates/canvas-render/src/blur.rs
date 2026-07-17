//! Desenfoque gaussiano en GPU: dos pasadas (horizontal y vertical) con un
//! shader wgpu propio. No destructivo: la imagen original no se toca; la
//! textura desenfocada se registra en vello y la escena la usa en su lugar.

use std::collections::HashMap;

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

/// Radio máximo del kernel (taps por lado). El slider de la UI llega a 100.
const MAX_RADIUS: i32 = 100;

#[repr(C)]
#[derive(Clone, Copy)]
struct Params {
    dir: [f32; 2],
    sigma: f32,
    radius: i32,
}

// SAFETY del transmute manual: Params es #[repr(C)], 16 bytes, sin padding.
fn params_bytes(p: &Params) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&p.dir[0].to_le_bytes());
    out[4..8].copy_from_slice(&p.dir[1].to_le_bytes());
    out[8..12].copy_from_slice(&p.sigma.to_le_bytes());
    out[12..16].copy_from_slice(&p.radius.to_le_bytes());
    out
}

/// Texturas de una capa con desenfoque activo.
struct LayerBlur {
    /// Imagen original subida a GPU (una vez).
    src: wgpu::Texture,
    /// Salida de la pasada horizontal.
    mid: wgpu::Texture,
    /// Salida final (vertical); es la que consume vello.
    out: wgpu::Texture,
    /// Handle de vello que redirige a `out`.
    image: ImageData,
    last_radius: f32,
}

pub struct BlurEngine {
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cache: HashMap<LayerId, LayerBlur>,
}

impl BlurEngine {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur gaussiano"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur pipeline layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bind_layout,
            sampler,
            cache: HashMap::new(),
        }
    }

    /// Imágenes sustitutas (desenfocadas) por capa, para la escena.
    pub fn overrides(&self) -> HashMap<LayerId, ImageData> {
        self.cache
            .iter()
            .map(|(id, b)| (*id, b.image.clone()))
            .collect()
    }

    /// Sincroniza el desenfoque de una capa. Devuelve las capas cuyo override
    /// hay que retirar de vello (radio a cero).
    ///
    /// `register` registra la textura de salida en vello y devuelve su handle
    /// (se inyecta para no acoplar este módulo al `Renderer`).
    pub fn sync_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: LayerId,
        source: &ImageData,
        radius: f32,
        register: &mut dyn FnMut(wgpu::Texture) -> ImageData,
    ) -> Option<ImageData> {
        if radius <= 0.0 {
            // Devuelve el handle a des-registrar, si lo había.
            return self.cache.remove(&layer).map(|b| b.image);
        }

        let entry = self.cache.entry(layer).or_insert_with(|| {
            let (w, h) = (source.width, source.height);
            let src = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("blur src"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                src.as_image_copy(),
                source.data.data(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            let mid = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("blur mid"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            // La salida además debe poder copiarse al atlas de vello (COPY_SRC).
            let out = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("blur out"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let image = register(out.clone());
            LayerBlur {
                src,
                mid,
                out,
                image,
                last_radius: -1.0,
            }
        });

        if (entry.last_radius - radius).abs() > f32::EPSILON {
            let sigma = (radius / 3.0).max(0.1);
            let taps = (radius.ceil() as i32).clamp(1, MAX_RADIUS);
            run_pass(
                device,
                queue,
                &self.pipeline,
                &self.bind_layout,
                &self.sampler,
                &entry.src,
                &entry.mid,
                [1.0, 0.0],
                sigma,
                taps,
            );
            run_pass(
                device,
                queue,
                &self.pipeline,
                &self.bind_layout,
                &self.sampler,
                &entry.mid,
                &entry.out,
                [0.0, 1.0],
                sigma,
                taps,
            );
            entry.last_radius = radius;
        }
        None
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
    dir: [f32; 2],
    sigma: f32,
    radius: i32,
) {
    let params = Params { dir, sigma, radius };
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("blur params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &params_bytes(&params));

    let from_view = from.create_view(&Default::default());
    let to_view = to.create_view(&Default::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur bind"),
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
        label: Some("blur encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blur pass"),
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
