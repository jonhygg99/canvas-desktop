//! Horneado CPU de la receta «pegar en lienzo vacío»: foto en *contain* +
//! fondo *cover* desenfocado. Réplica aproximada del resultado GPU (el PNG
//! plano es el respaldo; al abrir, el sidecar restaura las capas y el blur
//! sigue vivo por GPU): el worker no tiene GPU, así que el fondo se
//! desenfoca aquí en pequeño y se reescala.
//!
//! Todo puro y sin red: entra la foto decodificada y la geometría leída del
//! documento ya construido con `add_image_layer`, sale el RGBA de la página.

use image::{imageops::FilterType, RgbaImage};

/// Lado mayor con el que se desenfoca el fondo: barato y suficiente para
/// un fondo (el reescalado posterior suaviza el resto).
const BG_WORKING_LONG: u32 = 320;
/// Sigma del blur sobre la copia de trabajo (equivalente aproximado al
/// radio 50 a resolución completa).
const BG_SIGMA: f32 = 12.0;

/// Geometría leída del documento construido con `add_image_layer`.
pub struct BakeGeom {
    /// Tamaño de página en píxeles.
    pub pw: u32,
    /// Tamaño de página en píxeles.
    pub ph: u32,
    /// Fondo de página (blanco en `new_blank_image`).
    pub bg: [u8; 4],
    /// Rect de la foto ya calculado por `add_image_layer`.
    pub fx: f64,
    /// Rect de la foto ya calculado por `add_image_layer`.
    pub fy: f64,
    /// Rect de la foto ya calculado por `add_image_layer`.
    pub fw: f64,
    /// Rect de la foto ya calculado por `add_image_layer`.
    pub fh: f64,
    /// Hay capa «Blurred background» (la foto no cubre la página).
    pub has_bg: bool,
}

/// Compone la página: fondo blanco, encima el fondo desenfocado (si lo hay)
/// y encima la foto en su rect. Devuelve RGBA de `pw × ph`.
pub fn bake_contain_blur(photo: &RgbaImage, geom: &BakeGeom) -> Vec<u8> {
    let (pw, ph) = (geom.pw.max(1), geom.ph.max(1));
    let mut page = RgbaImage::from_pixel(pw, ph, image::Rgba(geom.bg));
    if geom.has_bg {
        let bg = blurred_cover(photo, pw, ph, geom.bg);
        image::imageops::overlay(&mut page, &bg, 0, 0);
    }
    let (fw, fh) = (
        geom.fw.round().max(1.0) as u32,
        geom.fh.round().max(1.0) as u32,
    );
    let (nw, nh) = photo.dimensions();
    if fw > 0 && fh > 0 && nw > 0 && nh > 0 {
        let fg =
            image::imageops::resize(photo, fw.min(pw * 4), fh.min(ph * 4), FilterType::Lanczos3);
        let (fx, fy) = (geom.fx.round() as i64, geom.fy.round() as i64);
        image::imageops::overlay(&mut page, &fg, fx, fy);
    }
    page.into_raw()
}

/// Fondo *cover* de `pw × ph` a partir de la foto: se reduce, se desenfoca
/// en pequeño y se reescala recortando el centro. Sin foto, el fondo liso.
fn blurred_cover(photo: &RgbaImage, pw: u32, ph: u32, bg: [u8; 4]) -> RgbaImage {
    let (nw, nh) = photo.dimensions();
    if nw == 0 || nh == 0 {
        return RgbaImage::from_pixel(pw, ph, image::Rgba(bg));
    }
    let scale = BG_WORKING_LONG as f32 / nw.max(nh) as f32;
    let (sw, sh) = (
        ((nw as f32 * scale).round() as u32).max(1),
        ((nh as f32 * scale).round() as u32).max(1),
    );
    let small = image::imageops::resize(photo, sw, sh, FilterType::Triangle);
    let blurred = image::imageops::blur(&small, BG_SIGMA);
    // *Cover* a página completa recortando el centro.
    let scale = (pw as f32 / sw as f32).max(ph as f32 / sh as f32);
    let (cw, ch) = (
        ((sw as f32 * scale).round() as u32).max(1),
        ((sh as f32 * scale).round() as u32).max(1),
    );
    let cover = image::imageops::resize(&blurred, cw, ch, FilterType::Triangle);
    let (ox, oy) = (
        (cw.saturating_sub(pw) / 2).min(cw.saturating_sub(1)),
        (ch.saturating_sub(ph) / 2).min(ch.saturating_sub(1)),
    );
    let cropped = image::imageops::crop_imm(&cover, ox, oy, pw.min(cw), ph.min(ch)).to_image();
    // Tamaño exacto de página aunque el redondeo deje 1px de menos.
    if cropped.dimensions() == (pw, ph) {
        cropped
    } else {
        image::imageops::resize(&cropped, pw, ph, FilterType::Triangle)
    }
}
