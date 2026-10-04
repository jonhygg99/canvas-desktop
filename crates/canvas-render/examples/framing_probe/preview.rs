//! Referencia raster del mismo fondo reducido y geometr?a usados en el preview.
use anyhow::Result;
use canvas_core::framing::Framing;
use image::imageops::{self, FilterType};
use std::{io::Write, path::Path};

pub(super) fn write(folder: &Path, png: &Path, framing: Framing) -> Result<()> {
    let source = image::open(png)?;
    let reduced = source
        .resize_to_fill(360, 640, FilterType::Triangle)
        .to_rgba8();
    let background = imageops::blur(&reduced, 20.0 / 3.0);
    let mut portrait = imageops::resize(&background, 1080, 1920, FilterType::Triangle);
    let p = framing
        .placement(f64::from(source.width()), f64::from(source.height()))
        .unwrap();
    let foreground = source
        .resize_exact(p.width as u32, p.height as u32, FilterType::Triangle)
        .to_rgba8();
    imageops::overlay(
        &mut portrait,
        &foreground,
        p.x.round() as i64,
        p.y.round() as i64,
    );
    portrait.save(folder.join("canvas-preview-reference.png"))?;
    let mut csv = std::fs::File::create(folder.join("geometry.csv"))?;
    writeln!(csv, "x,y,scale,left,top,width,height")?;
    for scale_pct in [50, 100, 200] {
        for x_pct in [-100.0, 0.0, 100.0] {
            for y_pct in [-100.0, 0.0, 100.0] {
                let p = Framing {
                    x_pct,
                    y_pct,
                    scale_pct,
                }
                .placement(1920.0, 1080.0)
                .unwrap();
                writeln!(
                    csv,
                    "{x_pct},{y_pct},{scale_pct},{},{},{},{}",
                    p.x, p.y, p.width, p.height
                )?;
            }
        }
    }
    Ok(())
}
