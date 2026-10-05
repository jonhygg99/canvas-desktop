//! Fixture multicapas para comprobar framing y export sin tocar medios del usuario.
//! cargo run -p canvas-render --example framing_probe -- target/framing-probe
use anyhow::{anyhow, Context, Result};
use canvas_render::CanvasRenderer;
#[path = "framing_probe/fixture.rs"]
mod fixture;
#[path = "framing_probe/preview.rs"]
mod preview;
use std::path::PathBuf;
use vello::util::RenderContext;

fn main() -> Result<()> {
    let folder = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("expected a test output folder")?,
    );
    std::fs::create_dir_all(&folder)?;
    let (doc, images) = fixture::build()?;
    let mut ctx = RenderContext::new();
    let id = pollster::block_on(ctx.device(None)).ok_or_else(|| anyhow!("GPU unavailable"))?;
    let handle = &ctx.devices[id];
    let mut renderer = CanvasRenderer::new(&handle.device)?;
    let (rgba, w, h, skipped) = renderer.bake_page_counting(
        &handle.device,
        &handle.queue,
        canvas_render::FxScope::default(),
        &doc,
        &images,
        1.0,
    )?;
    anyhow::ensure!(
        skipped == 0 && (w, h) == (1920, 1080),
        "incomplete composition"
    );
    fixture::check(&rgba, w)?;
    let payload = canvas_io::CanvasPayload {
        document: doc,
        images: images
            .iter()
            .map(|(id, p)| (id.raw(), p.data.data().to_vec(), p.width, p.height))
            .collect(),
        background_layer: None,
        preview: canvas_io::make_preview(&rgba, w, h),
    };
    let design = folder.join("composition.canvas");
    canvas_io::write_design(&design, &payload)?;
    let original = std::fs::read(&design)?;
    let png = folder.join("composition.png");
    canvas_io::save_rgba(&png, rgba, w, h, 92, None)?;
    let f = canvas_core::framing::Framing {
        scale_pct: 50,
        ..Default::default()
    };
    preview::write(&folder, &png, f)?;
    canvas_io::write_framing(&png, f)?;
    canvas_io::write_framing(&design, f)?;
    anyhow::ensure!(
        std::fs::read(&design)? == original,
        "framing modified canvas"
    );
    println!("FRAMING_PROBE=ok: image + grouped text/shape + crop + effects + opacity + video poster + background, 1920x1080 PNG, portable sidecar, original canvas preserved");
    Ok(())
}
