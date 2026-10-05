//! Geometria y conversion de parametros compartidos con el canvas.
use canvas_core::Transform;

/// Fija el playhead al fotograma anterior o igual (rejilla `fps`).
pub(crate) fn quantize_playhead(playhead: f64, trim_start: f64, fps: f64) -> f64 {
    if fps <= 0.0 {
        return playhead;
    }
    let k = ((playhead - trim_start) * fps).floor().max(0.0);
    trim_start + k / fps
}

/// Rect contain (x, y, w, h) del vídeo dentro de la caja: crece solo hasta
/// que un lado toca el borde, proporción intacta (como al pegar imágenes).
pub(crate) fn contain_rect(box_w: f32, box_h: f32, vw: f64, vh: f64) -> (f32, f32, f32, f32) {
    if vw <= 0.0 || vh <= 0.0 {
        return (0.0, 0.0, box_w, box_h);
    }
    let scale = (f64::from(box_w) / vw).min(f64::from(box_h) / vh) as f32;
    let (w, h) = (vw as f32 * scale, vh as f32 * scale);
    ((box_w - w) / 2.0, (box_h - h) / 2.0, w, h)
}

/// Rect contain crecido por el zoom alrededor de su centro (como el
/// Transform en el lienzo): la caja de preview lo recorta.
pub(crate) fn grown_rect(x: f32, y: f32, w: f32, h: f32, zoom: f32) -> (f32, f32, f32, f32) {
    let z = zoom.max(1.0);
    let (nw, nh) = (w * z, h * z);
    (x + (w - nw) / 2.0, y + (h - nh) / 2.0, nw, nh)
}

/// Slider 0..=100 → radio de blur de la capa (el fondo de referencia usa 50).
pub(crate) fn blur_radius_for_slider(v: f32) -> f32 {
    v.clamp(0.0, 100.0) / 2.0
}

/// Radio de la capa → slider (restaura al abrir).
pub(crate) fn slider_for_radius(r: f32) -> f32 {
    (r * 2.0).clamp(0.0, 100.0)
}

/// Transform contain escalado `zoom` desde el centro: base encajada (crece
/// solo hasta tocar el borde), la página recorta lo que sobresale.
pub(crate) fn zoom_transform(vw: f64, vh: f64, pw: f64, ph: f64, zoom: f32) -> Transform {
    let base = (pw / vw.max(1.0)).min(ph / vh.max(1.0));
    let z = zoom.max(1.0) as f64 * base;
    Transform::new((pw - vw * z) / 2.0, (ph - vh * z) / 2.0, vw * z, vh * z)
}

/// Ancho actual → zoom contra la base contain (restaura al abrir). Libre
/// hasta 10× (el slider solo llega a 3, el campo manual más).
pub(crate) fn zoom_for_transform(layer_w: f64, vw: f64, vh: f64, pw: f64, ph: f64) -> f32 {
    let base = (pw / vw.max(1.0)).min(ph / vh.max(1.0));
    if base > 0.0 {
        (layer_w / (vw * base)).clamp(1.0, 10.0) as f32
    } else {
        1.0
    }
}
