//! Selector de fuente de la pestaña «Images» (Google / Instagram-Facebook /
//! Unsplash): tres etiquetas seleccionables encima de la barra de búsqueda,
//! con su etiqueta y su ayuda. Antes eran dos pestañas con dos barras de
//! búsqueda; ahora la barra y los resultados son los de la fuente elegida, y
//! el cuerpo que se pinta lo decide `images_tab`.
//!
//! Regla de cambio de fuente (`apply_source`): elegir Google o
//! Instagram/Facebook manda el modo a Serper — alternar entre esos dos SÍ
//! reinicia la búsqueda, como cuando eran pestañas distintas, porque el
//! corpus es otro. Elegir Unsplash no toca nada de Serper: sus resultados,
//! paginación, `active_search` y gasto sobreviven al viaje de ida y vuelta.

use eframe::egui;

use crate::editor::state::ImageSource;

/// Etiqueta de una fuente (la clave del catálogo es su versión en inglés).
pub(super) fn source_label(source: ImageSource) -> &'static str {
    crate::i18n::tr(match source {
        ImageSource::Web => "Web images",
        ImageSource::Social => "Instagram/Facebook",
        ImageSource::Unsplash => "Unsplash",
    })
}

/// Ayuda de una línea de una fuente (tooltip de su opción).
pub(super) fn source_tip(source: ImageSource) -> &'static str {
    crate::i18n::tr(match source {
        ImageSource::Web => "General web image search",
        ImageSource::Social => {
            "Photos of a person on Instagram/Facebook — paste a profile link or a name"
        }
        ImageSource::Unsplash => "Photos from Unsplash",
    })
}

/// Pinta el selector y escribe la fuente elegida. Las tres etiquetas envuelven
/// (`horizontal_wrapped`): en el sidebar estrecho no caben en una línea y
/// cortarlas dejaría opciones invisibles.
///
/// No devuelve «¿cambió?» a propósito: la regla de cambio de fuente
/// (`apply_source`) se aplica cada frame y es idempotente, así que una
/// bandera de «este frame hubo clic» solo invitaría a saltársela cuando la
/// fuente apareciera por otra vía (ajustes, otra ventana) y el buscador
/// quedaría con el modo de la fuente anterior.
pub(super) fn source_selector_ui(source: &mut ImageSource, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for option in ImageSource::ALL {
            if ui
                .selectable_label(*source == option, source_label(option))
                .on_hover_text(source_tip(option))
                .clicked()
            {
                *source = option;
            }
        }
    });
}

/// Aplica una fuente al buscador de Serper: los dos modos de Serper escriben
/// su modo (idempotente: `switch_mode` no hace nada si ya coincide) y
/// Unsplash no escribe nada.
pub(super) fn apply_source(panel: &mut crate::serper::Panel, source: ImageSource) {
    if let Some(mode) = source.serper_mode() {
        panel.switch_mode(mode);
    }
}
