//! Ciclo de vida del motor de efectos: crear los pipelines y el muestreador
//! una sola vez, publicar las imagenes sustitutas de un scope, y soltar las
//! texturas cuando el scope desaparece.

use std::collections::HashMap;

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

use super::{fx_bytes, BlurEngine, FxScope};

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
            display: HashMap::new(),
            tick: 0,
        }
    }

    /// Bytes GPU totales en la caché de efectos: todas las texturas de todos
    /// los scopes (cada capa con efectos = 4 texturas, ver `fx_bytes`). Es la
    /// señal del presupuesto del documento activo — el monto que la evicción
    /// LRU (Task 6 del plan de memoria) intenta acotar. O(n) sobre la caché,
    /// pensado para la comprobación de presupuesto, no para el hot path.
    pub fn total_bytes(&self) -> u64 {
        self.cache
            .values()
            .map(|entry| fx_bytes(entry.src.width(), entry.src.height()))
            .sum()
    }

    /// Bytes GPU de un scope concreto (un documento). Las copias `display`
    /// (CPU) no entran: el presupuesto acota memoria GPU.
    pub fn bytes_in_scope(&self, scope: FxScope) -> u64 {
        self.cache
            .iter()
            .filter(|((s, _), _)| *s == scope)
            .map(|((_, _), entry)| fx_bytes(entry.src.width(), entry.src.height()))
            .sum()
    }

    /// Tick del último uso de `scope` — el máximo de `last_used` de sus
    /// capas con efectos, o `None` si el scope no tiene ninguna. Es el orden
    /// de evicción LRU del presupuesto GPU: el scope más antiguo es el
    /// candidato a liberar primero (salvo el que se está renderizando).
    pub fn last_used(&self, scope: FxScope) -> Option<u64> {
        self.cache
            .iter()
            .filter(|((s, _), _)| *s == scope)
            .map(|((_, _), entry)| entry.last_used)
            .max()
    }

    /// Imágenes sustitutas por capa de `scope`, para la escena: las texturas
    /// procesadas (efectos GPU) y las copias reducidas de las capas sin
    /// efectos demasiado grandes (`display`). La clave devuelta es el
    /// `LayerId` pelado (sin el scope): así `append_document`/`ImageMap` no
    /// necesitan saber que la caché de efectos distingue documentos. Una capa
    /// con efectos nunca tiene entrada en `display` (se retira al activar
    /// efectos), así que no puede haber colisión; de todas formas `display`
    /// gana si la hubiera.
    pub fn overrides(&self, scope: FxScope) -> HashMap<LayerId, ImageData> {
        let mut out: HashMap<LayerId, ImageData> = self
            .cache
            .iter()
            .filter(|((s, _), _)| *s == scope)
            .map(|((_, id), b)| (*id, b.image.clone()))
            .collect();
        for ((s, id), entry) in &self.display {
            if *s == scope {
                out.insert(*id, entry.image.clone());
            }
        }
        out
    }

    /// Retira de la caché todas las capas de `scope` (un lienzo descargado
    /// de la baraja) y devuelve sus handles para des-registrarlos de vello:
    /// sin esto, sus texturas de efectos quedarían vivas en GPU
    /// indefinidamente aunque el documento ya no esté cargado. Las copias
    /// reducidas (`display`) son CPU puro y solo hay que soltarlas de la
    /// caché; el atlas de vello las descarta solo al no verlas más.
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
        self.display.retain(|(s, _), _| *s != scope);
        removed
    }
}
