//! Genera un diseño `.canvas` de prueba con la capa de «fondo desenfocado»
//! (blur 50) + una capa de imagen delante, para verificar EN VIVO el fix de
//! scopes abriendo dos ventanas sobre el mismo archivo (`CANVAS_DEBUG_WINDOWS=2`):
//! cada ventana renderiza esas capas con blur, y el log del atlas
//! (`CANVAS_DEBUG_ATLAS=1`) debe mostrar 0 subidas por frame en reposo.
//!
//! Uso: cargo run -p canvas-io --example make_blur_design -- /ruta/salida.canvas

use std::path::Path;

use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_io::{write_design, CanvasPayload, LayerPixels};

fn solid_rgba(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    (0..w * h)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .ok_or("falta la ruta de salida: make_blur_design /ruta/salida.canvas")?;
    let out = Path::new(&out);

    let (w, h) = (1080u32, 1080u32);
    let mut document = Document::new(f64::from(w), f64::from(h));
    if let Ok(page) = document.page_mut() {
        page.background = Some([255, 255, 255, 255]);
    }

    // Capa 1: fondo desenfocado — cubre la página entera con blur 50, la
    // misma receta de la app. Se marca en `background_layer` del payload.
    let bg_id = document
        .add_layer(
            "Blurred background",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    document.layer_mut(bg_id).unwrap().effects.blur_radius = 50.0;

    // Capa 2: imagen delante, centrada.
    let (fw, fh) = (400u32, 400u32);
    let fg_id = document
        .add_layer(
            "Foreground",
            Transform::new(
                f64::from((w - fw) / 2),
                f64::from((h - fh) / 2),
                f64::from(fw),
                f64::from(fh),
            ),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: fw,
                natural_height: fh,
                crop: None,
            }),
        )
        .unwrap();

    let images: Vec<LayerPixels> = vec![
        (bg_id.raw(), solid_rgba(w, h, [60, 70, 90]), w, h),
        (fg_id.raw(), solid_rgba(fw, fh, [220, 120, 40]), fw, fh),
    ];

    let payload = CanvasPayload {
        document,
        images,
        background_layer: Some(bg_id.raw()),
        preview: None,
    };
    write_design(out, &payload)?;
    println!("diseño con blur escrito en {}", out.display());
    Ok(())
}
