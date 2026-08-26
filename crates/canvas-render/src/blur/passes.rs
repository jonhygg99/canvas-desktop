//! La cadena de pasadas GPU por capa: color -> blur horizontal -> blur
//! vertical, saltandose las que no hagan falta. Es donde de verdad se decide
//! si hay que rehornear una capa o si su textura sigue valiendo.

use canvas_core::LayerId;
use vello::peniko::ImageData;
use vello::wgpu;

use super::params::{blur_params_bytes, BlurParams, ColorParams, MAX_RADIUS};
use super::{
    capped_dims, capped_image, BlurEngine, DisplayEntry, FxScope, FxSync, LayerFx, MAX_FX_DIM,
};

/// Petición de sincronización de efectos GPU para una capa, agrupada para
/// reducir la firma de `sync_layer` de 9 a 5 parámetros.
pub struct SyncLayerRequest<'a> {
    pub scope: FxScope,
    pub layer: LayerId,
    pub source: &'a ImageData,
    pub color: ColorParams,
    pub radius: f32,
}

/// Recursos GPU que `run_pass` necesita, agrupados para reducir su firma de
/// 8 a 3 parámetros.
struct PassInput<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    bind_layout: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    from: &'a wgpu::Texture,
    to: &'a wgpu::Texture,
    params: &'a [u8],
}

impl BlurEngine {
    /// Mantiene la copia reducida de pantalla de una capa SIN efectos: se
    /// crea solo si la imagen original supera `MAX_FX_DIM` y su `Blob::id()`
    /// cambió (si no, la copia ya vale); se retira si no aplica (imagen
    /// pequeña: la escena usa la original).
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

    /// Sincroniza los efectos GPU de una capa de `scope`. Ver `FxSync` para
    /// qué debe hacer el llamador con el resultado.
    ///
    /// `register` registra la textura de salida en vello y devuelve su handle
    /// (se inyecta para no acoplar este módulo al `Renderer`).
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

        // Sin efectos: la escena debe volver a la imagen original... salvo si
        // es tan grande que vello no podría alojarla en su atlas (la
        // descartaría en silencio y el lienzo o el guardado saldría plano o
        // con la capa ausente): entonces se deja en `display` una copia
        // reducida para la escena. Se retira la textura de efectos si la
        // hubiera (el llamador la des-registra con `Removed`).
        if !blur_active && !color_active {
            self.sync_display(request);
            return match self.cache.remove(&key) {
                Some(b) => FxSync::Removed(b.image),
                None => FxSync::Unchanged,
            };
        }

        // Con efectos, la escena usa la textura procesada: fuera la copia de
        // pantalla (ambas comparten la clave de `LayerId` en el mapa de
        // overrides y no pueden convivir).
        self.display.remove(&key);

        // Relevo de dimensiones: si los píxeles de origen cambiaron Y la
        // imagen de trabajo vigente tiene otro tamaño que el del juego de
        // texturas cacheado (dimensionado con la PRIMERA imagen de la
        // entrada), hay que recrear el juego completo: reescribir en las
        // texturas viejas desbordaría su tamaño y wgpu lo rechaza con un
        // error de validación que tumba la app (pegar una foto de otra
        // resolución sobre una capa con efectos, informe crash-1787704986:
        // «Copy of X 0..2033 … overrunning … size 2000»). La textura
        // sustituida sale en `retired` para que el llamador la des-registre
        // de vello.
        //
        // `capped_dims` no materializa el thumbnail, así que mirarlo cada
        // frame es gratis.
        let mut retired: Option<ImageData> = None;
        let stale_size = match self.cache.get(&key) {
            Some(entry) => {
                entry.src_blob_id != request.source.data.id()
                    && capped_dims(request.source) != (entry.src.width(), entry.src.height())
            }
            None => false,
        };
        if stale_size {
            if let Some(old) = self.cache.remove(&key) {
                retired = Some(old.image);
            }
            self.cache
                .insert(key, create_fx_entry(device, queue, request, register));
        }

        // Imagen de trabajo: la original o una reducida por debajo del tope
        // del atlas (`MAX_FX_DIM`). Todo el pipeline de efectos trabaja sobre
        // ella; la escena la dibuja estirada al rect de la capa.
        let entry = self
            .cache
            .entry(key)
            .or_insert_with(|| create_fx_entry(device, queue, request, register));

        // Si los píxeles de origen cambiaron (el `Blob::id()` es distinto),
        // re-subir `src` y forzar un re-horneado. Sin esto, editar una imagen
        // (pegado, reemplazo) sin tocar el slider de blur dejaba la textura
        // procesada mostrando los píxeles antiguos — la caché solo invalidaba
        // cuando cambiaban los parámetros de efecto, no el contenido.
        // Si el TAMAÑO de trabajo también cambió, el relevo de arriba ya
        // recreó las texturas y subió los píxeles: aquí las dimensiones de
        // la nueva fuente SIEMPRE coinciden con las de `src`.
        let source_changed = entry.src_blob_id != request.source.data.id();
        if source_changed {
            let working = capped_or_original(request.source);
            let (w, h) = (working.width, working.height);
            let size = wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            };
            queue.write_texture(
                entry.src.as_image_copy(),
                working.data.data(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: None,
                },
                size,
            );
            entry.src_blob_id = request.source.data.id();
        }

        if source_changed || entry.last != Some((request.color, request.radius)) {
            // El radio del blur se escala a la resolución de trabajo real (la
            // textura `src`): un radio de N píxeles de ORIGEN debe seguir
            // siendo N píxeles de origen al desenfocar, aunque la pasada se
            // haga sobre la imagen reducida.
            let radius =
                request.radius * entry.src.width() as f32 / request.source.width.max(1) as f32;
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
                let blur_input = if color_active {
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
                        from: blur_input,
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
            return match retired {
                Some(retired) => FxSync::Replaced {
                    retired,
                    image: entry.image.clone(),
                },
                None => FxSync::Rebaked(entry.image.clone()),
            };
        }
        FxSync::Unchanged
    }
}

/// Imagen de trabajo de `sync_layer`: la original o su copia reducida al
/// tope del atlas si supera `MAX_FX_DIM`. Clonar un `ImageData` es barato
/// (el `Blob` va en un `Arc`).
fn capped_or_original(source: &ImageData) -> ImageData {
    match capped_image(source, MAX_FX_DIM) {
        Some(capped) => capped,
        None => source.clone(),
    }
}

/// Crea el juego COMPLETO de texturas de una capa con efectos (fuente,
/// intermedias y salida), SIEMPRE dimensionado a la imagen de trabajo
/// vigente, sube los píxeles a `src` y registra la salida en vello. Único
/// sitio donde se dimensionan texturas de efectos: así el tamaño del juego
/// coincide por construcción con la fuente que llega (ver «relevo de
/// dimensiones» en `sync_layer`). La entrada nace con `last` vacío para que
/// el horneado repase la cadena entera.
fn create_fx_entry(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    request: &SyncLayerRequest<'_>,
    register: &mut dyn FnMut(wgpu::Texture) -> ImageData,
) -> LayerFx {
    let working = capped_or_original(request.source);
    let (w, h) = (working.width, working.height);
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
        working.data.data(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: None,
        },
        size,
    );
    let inter = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
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
        src_blob_id: request.source.data.id(),
        mid_a,
        mid_b,
        out,
        image,
        last: None,
    }
}

fn run_pass(device: &wgpu::Device, queue: &wgpu::Queue, input: &PassInput<'_>) {
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
