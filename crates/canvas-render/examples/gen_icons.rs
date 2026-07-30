//! Regenera los iconos empaquetados a partir de `assets/icon.svg`: el `.ico`
//! multitamaño de Windows, el `.icns` de macOS y los PNG de la jerarquía
//! hicolor de Linux. No depende de herramientas externas (Inkscape,
//! ImageMagick): solo usa `resvg`/`tiny-skia`, ya en el árbol de dependencias
//! para exportar SVG (ver `canvas-io::export`).
//!
//! Uso: `cargo run -p canvas-render --example gen_icons`
//! Vuelve a ejecutarlo tras cambiar `assets/icon.svg` (arte definitivo).

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Tamaños incluidos en `assets/windows/icon.ico`.
const ICO_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

/// Tamaños de la jerarquía `hicolor` de Linux
/// (`assets/linux/hicolor/{n}x{n}/apps/canvas-desktop.png`).
const HICOLOR_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];

/// Entradas del `.icns` de macOS: (OSType, lado en píxeles).
const ICNS_ENTRIES: &[(&[u8; 4], u32)] = &[
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic11", 32), // 16pt@2x
    (b"ic12", 64), // 32pt@2x
    (b"ic07", 128),
    (b"ic13", 256), // 128pt@2x
    (b"ic08", 256),
    (b"ic14", 512), // 256pt@2x
    (b"ic09", 512),
    (b"ic10", 1024), // 512pt@2x
];

fn main() -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let svg_path = repo_root.join("assets/icon.svg");
    let svg_data = std::fs::read(&svg_path)
        .with_context(|| format!("no se pudo leer {}", svg_path.display()))?;

    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&svg_data, &options)
        .map_err(|e| anyhow::anyhow!("SVG inválido en {}: {e}", svg_path.display()))?;
    let natural = tree.size();

    let render_png = |side: u32| -> Result<Vec<u8>> {
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(side, side).context("tamaño de pixmap inválido")?;
        let scale = side as f32 / natural.width().max(natural.height());
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap.encode_png().context("codificando PNG")
    };

    // --- Windows: assets/windows/icon.ico ---
    let ico_dir = repo_root.join("assets/windows");
    std::fs::create_dir_all(&ico_dir)?;
    let ico_path = ico_dir.join("icon.ico");
    let pngs: Vec<(u32, Vec<u8>)> = ICO_SIZES
        .iter()
        .map(|&s| render_png(s).map(|p| (s, p)))
        .collect::<Result<_>>()?;
    write_ico(&ico_path, &pngs)?;
    println!("escrito {}", ico_path.display());

    // --- macOS: assets/macos/icon.icns ---
    let icns_dir = repo_root.join("assets/macos");
    std::fs::create_dir_all(&icns_dir)?;
    let icns_path = icns_dir.join("icon.icns");
    let mut icns_pngs = std::collections::HashMap::new();
    for &(_, side) in ICNS_ENTRIES {
        icns_pngs
            .entry(side)
            .or_insert_with(|| render_png(side).unwrap());
    }
    write_icns(&icns_path, &icns_pngs)?;
    println!("escrito {}", icns_path.display());

    // --- Linux: assets/linux/hicolor/{n}x{n}/apps/canvas-desktop.png ---
    let hicolor_root = repo_root.join("assets/linux/hicolor");
    for &side in HICOLOR_SIZES {
        let dir = hicolor_root.join(format!("{side}x{side}/apps"));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("canvas-desktop.png");
        std::fs::write(&path, render_png(side)?)?;
        println!("escrito {}", path.display());
    }

    Ok(())
}

/// Escribe un `.ico` (formato Windows Vista+: entradas PNG embebidas, sin
/// necesidad de convertir a BMP/DIB).
fn write_ico(path: &Path, pngs: &[(u32, Vec<u8>)]) -> Result<()> {
    let mut buf = Vec::new();
    // ICONDIR: reservado(0), tipo(1=icono), nº imágenes.
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&(pngs.len() as u16).to_le_bytes());

    let header_len = 6 + 16 * pngs.len();
    let mut offset = header_len as u32;
    for (side, png) in pngs {
        // ICONDIRENTRY: ancho/alto (0 = 256), paletas, reservado,
        // planos de color, bits por píxel, tamaño en bytes, offset.
        let side_byte = if *side >= 256 { 0u8 } else { *side as u8 };
        buf.push(side_byte);
        buf.push(side_byte);
        buf.push(0); // sin paleta
        buf.push(0); // reservado
        buf.extend_from_slice(&1u16.to_le_bytes()); // planos
        buf.extend_from_slice(&32u16.to_le_bytes()); // bpp
        buf.extend_from_slice(&(png.len() as u32).to_le_bytes());
        buf.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in pngs {
        buf.extend_from_slice(png);
    }

    let mut file = std::fs::File::create(path)?;
    file.write_all(&buf)?;
    Ok(())
}

/// Escribe un `.icns` (contenedor Apple: cabecera `icns` + longitud total,
/// seguido de bloques OSType + longitud + datos PNG).
fn write_icns(path: &Path, pngs_by_side: &std::collections::HashMap<u32, Vec<u8>>) -> Result<()> {
    let mut body = Vec::new();
    for &(ostype, side) in ICNS_ENTRIES {
        let png = pngs_by_side
            .get(&side)
            .with_context(|| format!("falta el PNG de {side}px para el icns"))?;
        body.extend_from_slice(ostype);
        let chunk_len = 8 + png.len() as u32;
        body.extend_from_slice(&chunk_len.to_be_bytes());
        body.extend_from_slice(png);
    }

    let mut file = std::fs::File::create(path)?;
    file.write_all(b"icns")?;
    let total_len = 8 + body.len() as u32;
    file.write_all(&total_len.to_be_bytes())?;
    file.write_all(&body)?;
    Ok(())
}
