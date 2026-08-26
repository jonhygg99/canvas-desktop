//! Genera diseños `.canvas` de prueba con la capa de «fondo desenfocado»
//! (blur 50) + una capa de imagen delante, para verificar EN VIVO el fix de
//! scopes abriendo dos ventanas (`CANVAS_DEBUG_WINDOWS=2`).
//!
//! Dos modos según el argumento:
//!
//! - `make_blur_design /ruta/salida.canvas` → un único diseño.
//! - `make_blur_design /ruta/carpeta` → una CARPETA con 5 diseños PESADOS
//!   (3024×4032 ≈ 12 MP, la talla de foto de teléfono que llenaba el atlas)
//!   con colores distintos, para el modo estrés de dos ventanas editando en
//!   paralelo sobre varias imágenes.
//!
//! Uso: cargo run -p canvas-io --example make_blur_design -- /ruta/salida.canvas
//!      cargo run -p canvas-io --example make_blur_design -- /ruta/carpeta

use std::path::{Path, PathBuf};

use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_io::{write_design, CanvasPayload, LayerPixels};

/// Degradado vertical entre dos colores: RGBA a `w×h` con el mismo peso en
/// RAM que una foto real (los sólidos también pesan al decodificarse, pero el
/// degradado se ve como imagen de verdad).
fn gradient_rgba(w: u32, h: u32, top: [u8; 3], bottom: [u8; 3]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        let t = y as f32 / (h as f32 - 1.0).max(1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        let row = [
            mix(top[0], bottom[0]),
            mix(top[1], bottom[1]),
            mix(top[2], bottom[2]),
            255,
        ];
        for _ in 0..w {
            rgba.extend_from_slice(&row);
        }
    }
    rgba
}

/// Un diseño pesado: página 3024×4032, fondo desenfocado a página completa
/// (blur 50) + capa principal centrada. Devuelve el payload listo para
/// `write_design`.
fn heavy_design(name: &str, hue: [u8; 3]) -> CanvasPayload {
    let (w, h) = (3024u32, 4032u32);
    let mut document = Document::new(f64::from(w), f64::from(h));
    if let Ok(page) = document.page_mut() {
        page.background = Some([255, 255, 255, 255]);
    }

    // Fondo desenfocado: cubre la página entera con blur 50 (la receta de la
    // app). Se marca en `background_layer` del payload.
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

    // Capa principal delante, centrada.
    let (fw, fh) = (1500u32, 2000u32);
    let fg_id = document
        .add_layer(
            name,
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

    // Degradados distintos por diseño (~49 MB + ~12 MB de RGBA por diseño).
    let bg_rgba = gradient_rgba(w, h, [hue[0], hue[1], hue[2]], [40, 40, 60]);
    let fg_rgba = gradient_rgba(fw, fh, [250, 250, 250], hue);
    let images: Vec<LayerPixels> =
        vec![(bg_id.raw(), bg_rgba, w, h), (fg_id.raw(), fg_rgba, fw, fh)];

    CanvasPayload {
        document,
        images,
        background_layer: Some(bg_id.raw()),
        preview: None,
    }
}

fn single_design(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let payload = heavy_design("Foreground", [220, 120, 40]);
    write_design(out, &payload)?;
    println!("diseño con blur escrito en {}", out.display());
    Ok(())
}

fn stress_folder(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out)?;
    let hues: [[u8; 3]; 5] = [
        [200, 60, 60],
        [60, 160, 200],
        [90, 200, 90],
        [220, 180, 60],
        [170, 90, 220],
    ];
    for (i, hue) in hues.iter().enumerate() {
        let name = format!("Foto{}", i + 1);
        let path: PathBuf = out.join(format!("{name}.canvas"));
        let payload = heavy_design(&name, *hue);
        write_design(&path, &payload)?;
        println!("  {}", path.display());
    }
    println!(
        "carpeta de estrés con {} diseños de 3024×4032 (~61 MB RGBA cada uno) en {}",
        hues.len(),
        out.display()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .ok_or("falta la ruta: make_blur_design /ruta/salida.canvas | /ruta/carpeta")?;
    let target = Path::new(&arg);
    let is_single = target
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("canvas"));
    if is_single {
        single_design(target)
    } else {
        stress_folder(target)
    }
}
