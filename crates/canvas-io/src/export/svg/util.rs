//! Ayudantes de serializacion a SVG: la matriz de colocacion, el escapado de
//! texto y el formateo compacto de numeros y colores.

use canvas_core::Transform;

/// `place_transform` (canvas-render/scene.rs) equivalente en SVG. Deriva de
/// `Affine::translate((t.x,t.y)) * Affine::rotate_about(rot, center) *
/// Affine::translate(center) * Affine::scale_non_uniform(sx,sy) *
/// Affine::translate(-center)`; como `rotate_about(th, c) = translate(c) *
/// rotate(th) * translate(-c)` en kurbo, el `translate(-center)` interno se
/// cancela con el `translate(center)` del volteo, quedando exactamente la
/// cadena de abajo (verificado contra la fuente de kurbo 0.11).
pub(super) fn place_transform_svg(t: &Transform) -> String {
    let (cx, cy) = (t.width / 2.0, t.height / 2.0);
    let sx = if t.flip_h { -1.0 } else { 1.0 };
    let sy = if t.flip_v { -1.0 } else { 1.0 };
    format!(
        "translate({tx} {ty}) translate({cx} {cy}) rotate({rot}) scale({sx} {sy}) translate({ncx} {ncy})",
        tx = n(t.x),
        ty = n(t.y),
        cx = n(cx),
        cy = n(cy),
        rot = n(t.rotation),
        sx = n(sx),
        sy = n(sy),
        ncx = n(-cx),
        ncy = n(-cy),
    )
}

pub(super) fn hex([r, g, b, _]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub(super) fn alpha([_, _, _, a]: [u8; 4]) -> f64 {
    f64::from(a) / 255.0
}

/// Escapa `&`, `<`, `>` y `"` para texto/atributos XML.
pub(super) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Formatea un número recortado a 4 decimales, sin ceros sobrantes. NaN/inf
/// se convierten en `0` (un SVG con un `NaN` en un atributo numérico no es
/// válido y algunos visores lo rechazan entero).
pub(super) fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let s = format!("{rounded:.4}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}
