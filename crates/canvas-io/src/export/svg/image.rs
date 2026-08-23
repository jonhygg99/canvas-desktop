//! Emision de una capa de imagen (o SVG embebido) como `<image>`, con los
//! pixeles ya procesados en base64.

use canvas_core::{CropRect, Transform};

use super::util::n;

/// `<image>` con el mismo recorte no destructivo que `scene.rs` (espeja
/// `image_local = scale_non_uniform(sx,sy) * translate(-crop.x*iw,
/// -crop.y*ih)`); si no hay píxeles para la capa (no debería pasar: el
/// llamador siempre los entrega), no escribe nada.
pub(super) fn image_element(
    svg: &mut String,
    png_base64: Option<&String>,
    t: &Transform,
    crop: Option<CropRect>,
    natural_width: u32,
    natural_height: u32,
) {
    let Some(b64) = png_base64 else { return };
    let (iw, ih) = (f64::from(natural_width), f64::from(natural_height));
    if iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let resolved = crop.map(CropRect::clamped).unwrap_or_else(CropRect::full);
    let sx = t.width / (resolved.width * iw);
    let sy = t.height / (resolved.height * ih);
    let (x, y) = (-resolved.x * iw * sx, -resolved.y * ih * sy);
    let (w, h) = (iw * sx, ih * sy);

    if crop.is_some() {
        svg.push_str(&format!(
            "<clipPath id=\"cropN\"><rect width=\"{}\" height=\"{}\"/></clipPath>\n",
            n(t.width),
            n(t.height),
        ));
        svg.push_str("<g clip-path=\"url(#cropN)\">\n");
    }
    svg.push_str(&format!(
        "<image x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/png;base64,{b64}\"/>\n",
        x = n(x),
        y = n(y),
        w = n(w),
        h = n(h),
    ));
    if crop.is_some() {
        svg.push_str("</g>\n");
    }
}
