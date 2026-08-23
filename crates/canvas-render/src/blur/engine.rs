//! Ciclo de vida del motor de efectos: crear los pipelines y el muestreador
//! una sola vez, publicar las imagenes sustitutas de un scope, y soltar las
//! texturas cuando el scope desaparece.

use std::collections::HashMap;

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

use super::{BlurEngine, FxScope};

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

    /// Imágenes sustitutas (procesadas) por capa de `scope`, para la escena.
    /// La clave devuelta es el `LayerId` pelado (sin el scope): así
    /// `append_document`/`ImageMap` no necesitan saber que la caché de
    /// efectos distingue documentos.
    pub fn overrides(&self, scope: FxScope) -> HashMap<LayerId, ImageData> {
        self.cache
            .iter()
            .filter(|((s, _), _)| *s == scope)
            .map(|((_, id), b)| (*id, b.image.clone()))
            .collect()
    }

    /// Retira de la caché todas las capas de `scope` (un lienzo descargado
    /// de la baraja) y devuelve sus handles para des-registrarlos de vello:
    /// sin esto, sus texturas de efectos quedarían vivas en GPU
    /// indefinidamente aunque el documento ya no esté cargado.
    pub fn forget_scope(&mut self, scope: FxScope) -> Vec<ImageData> {
        let mut removed = Vec::new();
        self.cache.retain(|(s, _), entry| {
            if *s == scope {
                removed.push(entry.image.clone());
                false
            } else {
                true
            }
        });
        removed
    }
}
