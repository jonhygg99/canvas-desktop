//! Tests de `geometry`. Juntos a proposito: comparten el ayudante `t()` y
//! varios cruzan alineacion, redimensionado y recorte a la vez.

use super::*;
use crate::layer::{CropRect, Transform};

fn t(x: f64, y: f64, w: f64, h: f64) -> Transform {
    Transform::new(x, y, w, h)
}

#[test]
fn uncrop_restores_full_content_in_place() {
    // Recorte del cuarto superior izquierdo de una imagen 100×100 en (25,10).
    let start = t(0.0, 0.0, 100.0, 100.0);
    let (cropped_t, crop) =
        trim_crop_from_corner(&start, CropRect::full(), Corner::BottomRight, -50.0, -50.0);
    let restored = uncrop_transform(&cropped_t, crop);
    assert!((restored.x - start.x).abs() < 1e-9);
    assert!((restored.y - start.y).abs() < 1e-9);
    assert!((restored.width - 100.0).abs() < 1e-9);
    assert!((restored.height - 100.0).abs() < 1e-9);
}

#[test]
fn horizontal_alignment_against_page() {
    let layer = t(37.0, 50.0, 200.0, 100.0);
    assert_eq!(align_horizontal(&layer, 800.0, HAlign::Left).x, 0.0);
    assert_eq!(align_horizontal(&layer, 800.0, HAlign::Center).x, 300.0);
    assert_eq!(align_horizontal(&layer, 800.0, HAlign::Right).x, 600.0);
    // La Y no cambia al alinear en horizontal.
    assert_eq!(align_horizontal(&layer, 800.0, HAlign::Center).y, 50.0);
}

#[test]
fn vertical_alignment_against_page() {
    let layer = t(37.0, 50.0, 200.0, 100.0);
    assert_eq!(align_vertical(&layer, 600.0, VAlign::Top).y, 0.0);
    assert_eq!(align_vertical(&layer, 600.0, VAlign::Middle).y, 250.0);
    assert_eq!(align_vertical(&layer, 600.0, VAlign::Bottom).y, 500.0);
    assert_eq!(align_vertical(&layer, 600.0, VAlign::Middle).x, 37.0);
}

#[test]
fn cover_scales_up_and_centers() {
    // Imagen 4:3 sobre página 16:9: manda el ancho, sobra alto.
    let c = cover_transform(800.0, 600.0, 1920.0, 1080.0);
    assert_eq!((c.width, c.height), (1920.0, 1440.0));
    assert_eq!(c.x, 0.0);
    assert_eq!(c.y, (1080.0 - 1440.0) / 2.0);

    // Imagen apaisada sobre página vertical: manda el alto.
    let c = cover_transform(1920.0, 1080.0, 1080.0, 1920.0);
    assert!((c.height - 1920.0).abs() < 1e-9);
    assert!(c.width > 1080.0);
    assert!((c.x - (1080.0 - c.width) / 2.0).abs() < 1e-9);
}

#[test]
fn contain_upscales_a_smaller_image_and_centers() {
    // Imagen vertical 9:16 pequeña sobre página cuadrada: manda el alto,
    // se amplía (a diferencia de un simple "encajar sin ampliar"), y
    // sobra ancho repartido a partes iguales.
    let c = contain_transform(540.0, 960.0, 1080.0, 1080.0);
    assert!((c.height - 1080.0).abs() < 1e-9);
    assert!(c.width < 1080.0);
    assert!(c.width > 540.0);
    assert!((c.x - (1080.0 - c.width) / 2.0).abs() < 1e-9);
    assert_eq!(c.y, 0.0);
}

#[test]
fn contain_shrinks_a_larger_image_to_fit() {
    // Imagen 4:3 grande sobre página 16:9: manda el alto, sobra ancho.
    let c = contain_transform(3000.0, 2000.0, 1920.0, 1080.0);
    assert!((c.height - 1080.0).abs() < 1e-9);
    assert!(c.width < 1920.0);
    assert!((c.x - (1920.0 - c.width) / 2.0).abs() < 1e-9);
}

#[test]
fn contain_matches_cover_when_the_aspect_ratio_is_the_same() {
    // Misma proporción que la página: "contain" y "cover" coinciden
    // exactamente, sin hueco ni recorte.
    let contain = contain_transform(1080.0, 1080.0, 500.0, 500.0);
    let cover = cover_transform(1080.0, 1080.0, 500.0, 500.0);
    assert_eq!(contain, cover);
    assert_eq!((contain.width, contain.height), (500.0, 500.0));
}

#[test]
fn resize_around_center_keeps_center() {
    let start = t(10.0, 20.0, 100.0, 50.0);
    let (cx, cy) = start.center();

    let grown = resize_around_center(&start, 200.0, 150.0);
    assert_eq!((grown.width, grown.height), (200.0, 150.0));
    let (gcx, gcy) = grown.center();
    assert!((gcx - cx).abs() < 1e-9);
    assert!((gcy - cy).abs() < 1e-9);

    let shrunk = resize_around_center(&start, 40.0, 10.0);
    assert_eq!((shrunk.width, shrunk.height), (40.0, 10.0));
    let (scx, scy) = shrunk.center();
    assert!((scx - cx).abs() < 1e-9);
    assert!((scy - cy).abs() < 1e-9);
}

#[test]
fn resize_around_center_preserves_rotation_and_flips() {
    let start = Transform {
        rotation: 30.0,
        flip_h: true,
        flip_v: true,
        ..t(10.0, 20.0, 100.0, 50.0)
    };
    let r = resize_around_center(&start, 200.0, 150.0);
    assert_eq!(r.rotation, 30.0);
    assert!(r.flip_h);
    assert!(r.flip_v);
}

#[test]
fn resize_around_center_clamps_min_size() {
    let start = t(10.0, 20.0, 100.0, 50.0);
    let (cx, cy) = start.center();
    let r = resize_around_center(&start, 0.0, -5.0);
    assert_eq!((r.width, r.height), (1.0, 1.0));
    let (rcx, rcy) = r.center();
    assert!((rcx - cx).abs() < 1e-9);
    assert!((rcy - cy).abs() < 1e-9);
}

#[test]
fn resize_bottom_right_keeps_top_left_anchored() {
    let start = t(10.0, 20.0, 100.0, 50.0);
    let r = resize_from_corner(&start, Corner::BottomRight, 50.0, 25.0, false, 1.0);
    assert_eq!((r.x, r.y), (10.0, 20.0));
    assert_eq!((r.width, r.height), (150.0, 75.0));
}

#[test]
fn resize_top_left_keeps_bottom_right_anchored() {
    let start = t(10.0, 20.0, 100.0, 50.0);
    let r = resize_from_corner(&start, Corner::TopLeft, -20.0, -10.0, false, 1.0);
    assert_eq!((r.width, r.height), (120.0, 60.0));
    // La esquina inferior derecha (110, 70) no se mueve.
    assert_eq!((r.x + r.width, r.y + r.height), (110.0, 70.0));
    assert_eq!((r.x, r.y), (-10.0, 10.0));
}

#[test]
fn aspect_lock_preserves_ratio_using_dominant_axis() {
    let start = t(0.0, 0.0, 200.0, 100.0);
    // dx domina (50% de cambio frente a 10%).
    let r = resize_from_corner(&start, Corner::BottomRight, 100.0, 10.0, true, 1.0);
    assert_eq!((r.width, r.height), (300.0, 150.0));
    assert!((r.aspect_ratio() - start.aspect_ratio()).abs() < 1e-9);
}

#[test]
fn aspect_lock_shrinks_too() {
    let start = t(0.0, 0.0, 200.0, 100.0);
    let r = resize_from_corner(&start, Corner::BottomRight, -100.0, -10.0, true, 1.0);
    assert_eq!((r.width, r.height), (100.0, 50.0));
}

#[test]
fn unlocked_resize_changes_ratio() {
    let start = t(0.0, 0.0, 200.0, 100.0);
    let r = resize_from_corner(&start, Corner::BottomRight, 0.0, 100.0, false, 1.0);
    assert_eq!((r.width, r.height), (200.0, 200.0));
}

#[test]
fn rotated_resize_keeps_opposite_corner_anchored() {
    let mut start = t(100.0, 100.0, 200.0, 100.0);
    start.rotation = 30.0;
    let anchor_before = start.corners()[0]; // superior izquierda

    // Arrastra la esquina inferior derecha 40 px en página.
    let r = resize_rotated_from_corner(&start, Corner::BottomRight, 40.0, 10.0, false, 1.0);
    let anchor_after = r.corners()[0];
    assert!((anchor_after.0 - anchor_before.0).abs() < 1e-9);
    assert!((anchor_after.1 - anchor_before.1).abs() < 1e-9);
    assert_eq!(r.rotation, 30.0);
}

#[test]
fn rotated_resize_with_zero_rotation_matches_plain() {
    let start = t(10.0, 20.0, 100.0, 50.0);
    let plain = resize_from_corner(&start, Corner::BottomRight, 30.0, 15.0, true, 1.0);
    let rotated = resize_rotated_from_corner(&start, Corner::BottomRight, 30.0, 15.0, true, 1.0);
    assert_eq!(plain, rotated);
}

#[test]
fn trim_crop_shrinks_window_and_keeps_content_fixed() {
    let start = t(0.0, 0.0, 100.0, 100.0);
    let (nt, crop) =
        trim_crop_from_corner(&start, CropRect::full(), Corner::BottomRight, -20.0, -30.0);
    assert_eq!((nt.x, nt.y), (0.0, 0.0)); // la esquina opuesta no se mueve
    assert_eq!((nt.width, nt.height), (80.0, 70.0));
    assert!((crop.width - 0.8).abs() < 1e-9);
    assert!((crop.height - 0.7).abs() < 1e-9);
    assert_eq!((crop.x, crop.y), (0.0, 0.0));
}

#[test]
fn trim_crop_cannot_expand_beyond_content() {
    let start = t(0.0, 0.0, 100.0, 100.0);
    // Sin recorte previo no hay contenido extra: expandir no hace nada.
    let (nt, crop) =
        trim_crop_from_corner(&start, CropRect::full(), Corner::BottomRight, 50.0, 50.0);
    assert_eq!((nt.width, nt.height), (100.0, 100.0));
    assert_eq!(crop, CropRect::full().clamped());
}

#[test]
fn trim_crop_top_left_moves_origin_and_crop_offset() {
    let start = t(0.0, 0.0, 100.0, 100.0);
    let (nt, crop) = trim_crop_from_corner(&start, CropRect::full(), Corner::TopLeft, 25.0, 10.0);
    // El borde izquierdo entra 25 px y el superior 10.
    assert_eq!((nt.x, nt.y), (25.0, 10.0));
    assert_eq!((nt.width, nt.height), (75.0, 90.0));
    assert!((crop.x - 0.25).abs() < 1e-9);
    assert!((crop.y - 0.10).abs() < 1e-9);
    // Deshacer el recorte (expandir de nuevo) recupera contenido.
    let (nt2, crop2) = trim_crop_from_corner(&nt, crop, Corner::TopLeft, -25.0, -10.0);
    assert!((nt2.x).abs() < 1e-9);
    assert!((crop2.x).abs() < 1e-9);
    assert!((crop2.width - 1.0).abs() < 1e-9);
}

#[test]
fn contains_point_respects_rotation() {
    let mut layer = t(0.0, 0.0, 100.0, 20.0);
    // Punto justo fuera de la esquina AABB.
    assert!(!layer.contains_point(95.0, 25.0));
    layer.rotation = 90.0; // ahora es alto y estrecho alrededor de (50,10)
    assert!(layer.contains_point(50.0, 55.0));
    assert!(!layer.contains_point(95.0, 10.0));
}

#[test]
fn resize_clamps_to_min_size_without_flipping() {
    let start = t(0.0, 0.0, 100.0, 100.0);
    let r = resize_from_corner(&start, Corner::BottomRight, -500.0, -500.0, false, 8.0);
    assert_eq!((r.width, r.height), (8.0, 8.0));

    let locked = resize_from_corner(&start, Corner::BottomRight, -500.0, -500.0, true, 8.0);
    assert!(locked.width >= 8.0 && locked.height >= 8.0);
    assert!((locked.aspect_ratio() - 1.0).abs() < 1e-9);
}
