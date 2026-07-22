//! Efectos GPU por capa, no destructivos: filtro de color (una pasada) y
//! desenfoque gaussiano (dos pasadas, horizontal y vertical), encadenados
//! color → blur. La imagen original no se toca; la textura procesada se
//! registra en vello y la escena la usa en su lugar.

use std::collections::HashMap;

use canvas_core::{Effects, LayerId};
use vello::peniko::ImageData;
use vello::wgpu;

/// Radio máximo del kernel (taps por lado). El slider de la UI llega a 100.
const MAX_RADIUS: i32 = 100;

/// Parámetros del filtro de color (0 = neutro en todos).
#[derive(Clone, Copy, PartialEq, Default)]
pub struct ColorParams {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub grayscale: f32,
    pub sepia: f32,
}

impl ColorParams {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    fn to_bytes(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, v) in [
            self.brightness,
            self.contrast,
            self.saturation,
            self.temperature,
            self.grayscale,
            self.sepia,
            0.0,
            0.0,
        ]
        .into_iter()
        .enumerate()
        {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

impl From<&Effects> for ColorParams {
    fn from(e: &Effects) -> Self {
        Self {
            brightness: e.brightness,
            contrast: e.contrast,
            saturation: e.saturation,
            temperature: e.temperature,
            grayscale: e.grayscale,
            sepia: e.sepia,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlurParams {
    dir: [f32; 2],
    sigma: f32,
    radius: i32,
}

// SAFETY del transmute manual: BlurParams es #[repr(C)], 16 bytes, sin padding.
fn blur_params_bytes(p: &BlurParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&p.dir[0].to_le_bytes());
    out[4..8].copy_from_slice(&p.dir[1].to_le_bytes());
    out[8..12].copy_from_slice(&p.sigma.to_le_bytes());
    out[12..16].copy_from_slice(&p.radius.to_le_bytes());
    out
}

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
    cache: HashMap<LayerId, LayerFx>,
}

impl BlurEngine {
    pub fn new(device: &wgpu::Device) -> Self {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fx bind layout"),
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
            label: Some("fx pipeline layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let make_pipeline = |label: &str, wgsl: &str| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
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
            })
        };
        let blur_pipeline = make_pipeline("blur gaussiano", include_str!("blur.wgsl"));
        let color_pipeline = make_pipeline("filtro de color", include_str!("color_filter.wgsl"));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fx sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            blur_pipeline,
            color_pipeline,
            bind_layout,
            sampler,
            cache: HashMap::new(),
        }
    }

    /// Imágenes sustitutas (procesadas) por capa, para la escena.
    pub fn overrides(&self) -> HashMap<LayerId, ImageData> {
        self.cache
            .iter()
            .map(|(id, b)| (*id, b.image.clone()))
            .collect()
    }

    /// Sincroniza los efectos GPU de una capa. Devuelve el handle a
    /// des-registrar de vello cuando ya no queda ningún efecto activo.
    ///
    /// `register` registra la textura de salida en vello y devuelve su handle
    /// (se inyecta para no acoplar este módulo al `Renderer`).
    #[allow(clippy::too_many_arguments)]
    pub fn sync_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: LayerId,
        source: &ImageData,
        color: ColorParams,
        radius: f32,
        register: &mut dyn FnMut(wgpu::Texture) -> ImageData,
    ) -> Option<ImageData> {
        let blur_active = radius > 0.0;
        if !blur_active && color.is_identity() {
            // Devuelve el handle a des-registrar, si lo había.
            return self.cache.remove(&layer).map(|b| b.image);
        }

        let entry = self.cache.entry(layer).or_insert_with(|| {
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
