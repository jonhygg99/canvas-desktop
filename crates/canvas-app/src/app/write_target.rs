//! Destino de una escritura de FONDO: un lienzo que no está en pantalla y que
//! la cola de escritura (`app/write_queue.rs`) quiere guardar solo.
//!
//! La diferencia con el lienzo activo no es el formato ni el horneado, sino
//! QUIÉN decide cuando hace falta una confirmación destructiva: el activo puede
//! enseñar el aviso de sobrescritura (o el de «se borró la última foto») porque
//! el usuario lo está mirando; una ranura de fondo no puede, y un modal
//! disparado por un temporizador sobre un lienzo que no se ve es justo lo que
//! el autoguardado no debe hacer. Así que aquí solo cuenta lo que la app puede
//! escribir SIN preguntar:
//!
//! - un archivo que escribió ella misma (bulk, «Save as», lienzo nuevo),
//! - un archivo cuya sobrescritura el usuario ya autorizó,
//! - o cualquier archivo si el usuario pidió «Don't ask again».
//!
//! Todo lo demás se queda pendiente para «Save all» o para el `Ctrl+S` de su
//! propio lienzo, que sí pueden preguntar. Pura (tabla de tests).

use std::path::PathBuf;

use canvas_core::Document;

/// Lo que la política necesita saber de la ranura, del documento y del flujo
/// de guardado. Se agrupa en una struct en vez de acumular parámetros.
pub(super) struct BackgroundRequest<'a> {
    pub doc: &'a Document,
    /// La ranura todavía no tiene archivo (lienzo en blanco recién añadido).
    pub is_placeholder: bool,
    /// El documento ES un `.canvas` autónomo.
    pub is_design: bool,
    /// Nació en blanco y nunca se guardó: no hay foto original que destruir.
    pub born_blank: bool,
    /// El archivo cambió en disco por fuera: escribirlo pisaría esa versión.
    pub external_change: bool,
    pub app_written: bool,
    pub overwrite_ok: bool,
    pub skip_overwrite_warning: bool,
}

/// Qué hacer con una ranura de fondo. `Skip` lleva el motivo ya en inglés
/// para la traza/la barra (nunca un `String` armado sobre la marcha).
pub(super) enum BackgroundWrite {
    /// Escribir este archivo, sin preguntar nada.
    Auto(PathBuf),
    Skip(&'static str),
}

/// ¿Se puede escribir esta ranura de fondo sin intervención del usuario?
pub(super) fn background_target(req: BackgroundRequest<'_>) -> BackgroundWrite {
    if req.is_placeholder {
        return BackgroundWrite::Skip("a canvas that has no file yet");
    }
    if req.external_change {
        return BackgroundWrite::Skip("the ⚠ file-changed-on-disk banner");
    }
    let Some(path) = req.doc.source_path.clone() else {
        return BackgroundWrite::Skip("a canvas that was never saved");
    };
    if req.is_design {
        if !canvas_io::is_canvas_file(&path) {
            return BackgroundWrite::Skip("a design without a .canvas destination");
        }
        return BackgroundWrite::Auto(path);
    }
    // SVG/GIF son de solo lectura en el sitio y un mp4 no se sobrescribe en
    // silencio: mismo criterio que el `Ctrl+S` del lienzo activo.
    if !canvas_io::can_overwrite(&path) {
        return BackgroundWrite::Skip("a format that is read-only in place");
    }
    // Aplanar un documento sin capas de imagen descarta la copia editable de la
    // foto original: es una decisión del usuario y solo su lienzo puede pedirla.
    if !req.born_blank && !super::persistence::has_raster_layers(req.doc) {
        return BackgroundWrite::Skip("the “no image layers left” decision");
    }
    if !req.app_written && !req.overwrite_ok && !req.skip_overwrite_warning {
        return BackgroundWrite::Skip("the overwrite confirmation that only its canvas can ask");
    }
    BackgroundWrite::Auto(path)
}

#[cfg(test)]
#[path = "write_target_tests.rs"]
mod tests;
