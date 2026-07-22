//! GATE de la fase de vectores: valida que parley (layout de texto) y vello
//! (draw_glyphs) conviven con el MISMO peniko antes de construir nada encima.
//! Renderiza un texto headless a PNG y comprueba que hay píxeles pintados.
//!
//! Uso: cargo run -p canvas-render --example text_probe [-- salida.png]

use anyhow::{anyhow, Context, Result};
use parley::{Alignment, AlignmentOptions, FontContext, LayoutContext, PositionedLayoutItem};
use vello::kurbo::Affine;
use vello::peniko::color::palette;
use vello::peniko::Fill;
use vello::util::RenderContext;
use vello::{AaConfig, Glyph, RenderParams, Renderer, RendererOptions, Scene};

fn main() -> Result<()> {
    let output = std::env::args().nth(1);
    let (width, height) = (480u32, 160u32);

    // Layout con parley: fuentes del sistema, 48 px.
    let text = "Canvas Desktop";
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::<[u8; 4]>::new();
    let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, true);
    builder.push_default(parley::StyleProperty::FontSize(48.0));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());

    // Escena vello: fondo blanco + glifos del layout.
    let mut scene = Scene::new();
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        palette::css::WHITE,
        None,
        &vello::kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
    );
    let transform = Affine::translate((24.0, 40.0));
    let mut glyphs_drawn = 0usize;
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font = run.run().font().clone();
            let font_size = run.run().font_size();
            let coords = run.run().normalized_coords().to_vec();
            let glyphs: Vec<Glyph> = run
                .positioned_glyphs()
                .map(|g| {
                    glyphs_drawn += 1;
                    Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    }
                })
                .collect();
            scene
                .draw_glyphs(&font)
                .font_size(font_size)
                .normalized_coords(&coords)
                .brush(palette::css::BLACK)
                .transform(transform)
                .draw(Fill::NonZero, glyphs.into_iter());
        }
    }
    anyhow::ensure!(
        glyphs_drawn > 5,
        "parley no produjo glifos ({glyphs_drawn})"
    );

    // Render headless.
    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);
    let mut renderer =
        Renderer::new(device, RendererOptions::default()).map_err(|e| anyhow!("renderer: {e}"))?;

    let target = device.create_texture(&vello::wgpu::TextureDescriptor {
        label: Some("probe target"),
        size: vello::wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: vello::wgpu::TextureDimension::D2,
        format: vello::wgpu::TextureFormat::Rgba8Unorm,
        usage: vello::wgpu::TextureUsages::STORAGE_BINDING | vello::wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    renderer
        .render_to_texture(
            device,
            queue,
            &scene,
            &view,
            &RenderParams {
                base_color: palette::css::WHITE,
                width,
                height,
                antialiasing_method: AaConfig::Area,
            },
        )
        .map_err(|e| anyhow!("render: {e}"))?;

    // Readback y verificación: tiene que haber tinta negra de sobra.
    let padded = (width * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&vello::wgpu::BufferDescriptor {
        label: Some("probe readback"),
        size: u64::from(padded) * u64::from(height),
        usage: vello::wgpu::BufferUsages::MAP_READ | vello::wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        vello::wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: vello::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        vello::wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(vello::wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(vello::wgpu::PollType::wait_indefinitely())
        .map_err(|e| anyhow!("esperando GPU: {e:?}"))?;
    rx.recv().context("map no respondió")??;
    let data = slice.get_mapped_range();

    let mut dark = 0usize;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        let line = &data[start..start + (width * 4) as usize];
        rgba.extend_from_slice(line);
        for px in line.chunks(4) {
            if px[0] < 128 && px[1] < 128 && px[2] < 128 {
                dark += 1;
            }
        }
    }
    drop(data);
    if let Some(path) = output {
        image::RgbaImage::from_raw(width, height, rgba)
            .context("buffer")?
            .save(&path)?;
        println!("guardado: {path}");
    }
    anyhow::ensure!(
        dark > 500,
        "el texto no dejó suficiente tinta ({dark} píxeles oscuros)"
    );
    println!("TEXT_PROBE=ok ({glyphs_drawn} glifos, {dark} píxeles de tinta)");
    Ok(())
}
