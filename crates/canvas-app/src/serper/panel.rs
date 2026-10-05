//! UI de la pestaña «Web» del sidebar del editor: barra de búsqueda con
//! selector de tokens, sección Advanced (dominios bloqueados), lista
//! vertical de tarjetas y pie con coste y transparencia del filtro. El
//! estado vive en `super::state::Panel`; la lista de bloqueados, el
//! presupuesto y el total histórico de créditos, en `AppSettings`. La
//! ventana de selección masiva vive en `super::bulk`.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;
use crate::settings::AppSettings;

use super::api::access_key;
use super::bulk::bulk_window_ui;
use super::card::photo_card_ui;
use super::state::{stalled, Panel, SEARCH_STALL_SECS, SHOW_STEP};
use super::types::{SearchMode, TokenBudget};
use super::API_KEY_ENV;

/// Margen lateral a cada lado de las tarjetas (igual que Unsplash).
const CARD_INSET: f32 = 12.0;

/// Contenido de la pestaña «Web» del panel lateral izquierdo. `dest` es la
/// carpeta destino del bulk ya resuelta (baraja → archivo → galería →
/// última usada; `None` = la ventana pedirá elegirla una vez).
/// `insert_dest` es la ranura que recibirá un clic de inserción (A07).
pub fn panel_ui(
    panel: &mut Panel,
    settings: &mut AppSettings,
    dest: Option<std::path::PathBuf>,
    insert_dest: loader::ImageInsertDest,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    sync_settings_once(panel, settings);
    if access_key().is_none() {
        ui.add_space(8.0);
        ui.label(format!("{API_KEY_ENV} is not set"));
        ui.add_space(4.0);
        ui.weak(crate::i18n::tr("Get a key at serper.dev and add it\nto the project .env as SERPER_API_KEY,\nthen restart the app."));
        return;
    }

    ui.add_space(6.0);
    search_bar_ui(panel, settings, ui, tx);
    advanced_ui(panel, settings, ui);
    results_ui(panel, settings, insert_dest, ui, tx);
    let ctx = ui.ctx().clone();
    bulk_window_ui(panel, dest, settings, &ctx, tx);
}

/// Primera pintura: el panel hereda bloqueados y presupuesto de los
/// ajustes (la UI trabaja sobre el espejo y escribe de vuelta al aplicar).
fn sync_settings_once(panel: &mut Panel, settings: &AppSettings) {
    if panel.synced_settings {
        return;
    }
    panel.synced_settings = true;
    panel.budget = settings.serper_budget;
    panel.blocked_text = settings.serper_blocked.join("\n");
}

/// Barra de búsqueda + selector de modo y de tokens 1–3.
fn search_bar_ui(
    panel: &mut Panel,
    settings: &mut AppSettings,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let mut do_search = false;
    ui.horizontal_wrapped(|ui| {
        for m in SearchMode::ALL {
            if ui
                .selectable_label(panel.mode == m, m.label())
                .on_hover_text(match m {
                    SearchMode::Web => "General web image search",
                    SearchMode::Social => {
                        "Photos of a person on Instagram/Facebook — paste a profile link or a name"
                    }
                })
                .clicked()
            {
                panel.switch_mode(m);
            }
        }
    });
    if panel.mode == SearchMode::Social {
        ui.weak(crate::i18n::tr(
            "Instagram/Facebook · social filter off · links expire fast",
        ));
    }
    let hint = match panel.mode {
        SearchMode::Web => "Search the web…",
        SearchMode::Social => "Paste an Instagram/Facebook link or name…",
    };
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            // Reservar primero el botón evita recortarlo en el sidebar estrecho.
            let clicked = ui.button(crate::i18n::tr("Search")).clicked();
            let width = (ui.available_width() - 8.0).max(1.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut panel.query)
                    .hint_text(hint)
                    .desired_width(width),
            );
            let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            do_search = (submit || clicked) && !panel.query.trim().is_empty();
        },
    );
    budget_ui(panel, settings, ui);
    if do_search {
        start_search(panel, &settings.serper_blocked, tx, ui.ctx());
    }
}

/// Selector del presupuesto por keyword (1 token ≈ 10 imgs/1cr, 2 tokens ≈
/// 100 imgs/2cr, 3 tokens ≈ 200 imgs/4cr). Escribe directo en ajustes
/// (persiste con el gasto de la sesión aparte).
fn budget_ui(panel: &mut Panel, settings: &mut AppSettings, ui: &mut egui::Ui) {
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.weak(crate::i18n::tr("Cost per keyword:"));
        for b in TokenBudget::ALL {
            if ui
                .selectable_label(panel.budget == b, format!("{} ({})", b.label(), b.detail()))
                .on_hover_text(format!(
                    "Up to ~{} images for ~{} Serper credits",
                    b.max_images(),
                    b.credits()
                ))
                .clicked()
            {
                panel.budget = b;
                settings.serper_budget = b;
                settings.save_in_background();
            }
        }
    });
}

/// Lanza una búsqueda nueva (página 1, gasta el primer token con el `num`
/// del presupuesto). Si la memoria ya tiene esa keyword, se sirve sin red
/// ni gasto. No hace nada si ya hay una en vuelo o la consulta está vacía.
/// Sella la spec activa (A09): «Bring more» paginará sobre ella, no sobre
/// lo que el usuario escriba después.
pub(super) fn start_search(
    panel: &mut Panel,
    blocked: &[String],
    tx: &Sender<loader::AppMsg>,
    ctx: &egui::Context,
) {
    if panel.searching || panel.query.trim().is_empty() {
        return;
    }
    let spec = super::state::ActiveSearch {
        keyword: panel.query.trim().to_owned(),
        mode: panel.mode,
        blocked: blocked.to_owned(),
        num: panel.budget.num(),
    };
    let key = spec.cache_key(1);
    if let Some(page) = panel.cache_lookup(&key) {
        panel.search_seq += 1;
        panel.searching = false;
        panel.search_started = None;
        panel.page = 1;
        panel.tokens_spent = 0;
        panel.pending_drop = None;
        panel.visible_count = SHOW_STEP;
        panel.active_search = Some(spec);
        panel.apply_page(&page, 1);
        panel.credits_last = 0;
        ctx.request_repaint();
        return;
    }
    panel.search_seq += 1;
    panel.searching = true;
    panel.search_started = Some(std::time::Instant::now());
    panel.page = 1;
    panel.tokens_spent = 1;
    panel.photos.clear();
    panel.error = None;
    panel.reached_end = false;
    panel.budget_exhausted = false;
    panel.pending_drop = None;
    panel.visible_count = SHOW_STEP;
    panel.active_search = Some(spec.clone());
    loader::spawn_serper_search(
        spec.page_request(1, panel.search_seq),
        tx.clone(),
        ctx.clone(),
    );
}

/// Gasta un token más en la MISMA búsqueda («Bring more»): la página
/// siguiente de la spec sellada (o la caché, sin gastar). El techo lo pone
/// `Panel::can_spend_more`. Nunca usa el borrador del cuadro de texto: si
/// el usuario escribió otra cosa sin pulsar Search, se pagina la búsqueda
/// en pantalla, no se mezcla con la nueva (A09).
pub(super) fn spend_token(panel: &mut Panel, tx: &Sender<loader::AppMsg>, ctx: &egui::Context) {
    if !panel.can_spend_more() {
        return;
    }
    let Some(spec) = panel.active_search.clone() else {
        return;
    };
    let next = panel.page + 1;
    let key = spec.cache_key(next);
    if let Some(page) = panel.cache_lookup(&key) {
        panel.search_seq += 1;
        panel.searching = false;
        panel.search_started = None;
        panel.page = next;
        panel.apply_page(&page, next);
        panel.credits_last = 0;
        ctx.request_repaint();
        return;
    }
    panel.search_seq += 1;
    panel.searching = true;
    panel.search_started = Some(std::time::Instant::now());
    panel.page = next;
    panel.tokens_spent += 1;
    loader::spawn_serper_search(
        spec.page_request(panel.page, panel.search_seq),
        tx.clone(),
        ctx.clone(),
    );
}

/// Sección Advanced: dominios bloqueados (uno por línea). Alimentan el
/// filtro local Y las exclusiones `-site:` (capadas al tope de palabras).
/// El `TextEdit` es multilínea con altura fija y scroll propio para las
/// ~40 entradas por defecto; `clip_text` y ancho infinito evitan que tokens
/// largos como `lookaside.instagram.com` sangren fuera del panel.
fn advanced_ui(panel: &mut Panel, settings: &mut AppSettings, ui: &mut egui::Ui) {
    ui.add_space(2.0);
    egui::CollapsingHeader::new(crate::i18n::tr("Advanced"))
        .default_open(false)
        .show(ui, |ui| {
            ui.weak(crate::i18n::tr("Blocked domains (one per line):\nhidden from results and excluded\nfrom the query when it fits."));
            // Scroll vertical dedicado: sin él la rueda iba al
            // `ScrollArea` de los resultados de abajo y el `TextEdit`
            // parecía no hacer scroll aunque tuviera 40 líneas. Con altura
            // fija y `auto_shrink(false)` siempre queda barra y la rueda
            // hace scroll aquí cuando el cursor está encima.
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(2, 2))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(84.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut panel.blocked_text)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                });
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Apply")).clicked() {
                    let blocked = parse_blocked(&panel.blocked_text);
                    panel.blocked_text = blocked.join("\n");
                    settings.serper_blocked = blocked;
                    settings.save_in_background();
                }
                if ui.button(crate::i18n::tr("Reset defaults")).clicked() {
                    let defaults = super::filter::default_blocked_domains();
                    panel.blocked_text = defaults.join("\n");
                    settings.serper_blocked = defaults;
                    settings.save_in_background();
                }
            });
        });
}

/// Normaliza el borrador del Advanced: minúsculas, sin vacías ni
/// duplicadas, en orden de aparición.
fn parse_blocked(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let domain = line.trim().to_lowercase();
        if domain.is_empty() || out.contains(&domain) {
            continue;
        }
        out.push(domain);
    }
    out
}

/// Lista de resultados + pie con «Bring more», «Show more» local y la
/// transparencia del coste y del filtro. `dest` es la ranura que recibirá un
/// clic de inserción (A07): se sella en la petición que viaja al worker.
fn results_ui(
    panel: &mut Panel,
    settings: &mut AppSettings,
    dest: loader::ImageInsertDest,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    if panel.searching && panel.photos.is_empty() {
        ui.add_space(10.0);
        let stuck = stalled(
            panel.search_started,
            std::time::Instant::now(),
            SEARCH_STALL_SECS,
        );
        if panel.stalled_ui(stuck, ui) {
            return;
        }
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(crate::i18n::tr("Searching… (1 token)"));
        });
        return;
    }
    if panel.photos.is_empty() {
        if let Some(err) = &panel.error {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().error_fg_color, err);
        } else {
            ui.add_space(8.0);
            ui.weak(crate::i18n::tr(
                "Search the web and click a photo\nto add it to the canvas.",
            ));
        }
        return;
    }

    let row_w = (ui.available_width() - CARD_INSET * 2.0).max(120.0);
    let img_h = (row_w * 0.66).clamp(150.0, 320.0);
    let shown = panel.visible_count.min(panel.photos.len());
    select_row_ui(panel, ui);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                // El clic solo PIDE insertar (la tarjeta no conoce la
                // baraja): el sellado + spawn ocurren aquí, con el destino
                // ya resuelto, y fuera de los préstamos de la tarjeta (A07).
                let mut clicked: Option<(String, String, String, String)> = None;
                {
                    let inserting = &mut panel.inserting;
                    for item in panel.photos.iter_mut().take(shown) {
                        if photo_card_ui(item, inserting, row_w, img_h, ui, tx) {
                            let p = &item.photo;
                            clicked = Some((
                                p.id.clone(),
                                format!("Web · {}", p.source_host()),
                                p.image_url.clone(),
                                p.source_url.clone(),
                            ));
                        }
                        ui.add_space(12.0);
                    }
                }
                if let Some((id, label, url, post_url)) = clicked {
                    if let Some(target) = panel.begin_insert(dest, &id) {
                        // La miniatura ya mostrada alimenta el último nivel
                        // del rescate (A08): si la directa falla, se prefiere
                        // lo que el usuario vio antes que un `og:image` ajeno.
                        let thumb_url = panel
                            .photos
                            .iter()
                            .find(|p| p.photo.id == id)
                            .and_then(|p| p.photo.thumb_url.clone());
                        loader::spawn_serper_image(
                            loader::SerperImageRequest {
                                id,
                                label,
                                url,
                                post_url,
                                thumb_url,
                                target,
                            },
                            tx.clone(),
                            ui.ctx().clone(),
                        );
                    }
                }
                ui.add_space(4.0);
                list_footer_ui(panel, ui, shown, tx);
                ui.add_space(12.0);
            });
        });
    ui.add_space(4.0);
    cost_footer_ui(panel, settings, ui);
}

/// Fila «Select…»: abre la ventana masiva (todo marcado por defecto).
fn select_row_ui(panel: &mut Panel, ui: &mut egui::Ui) {
    ui.add_enabled_ui(!panel.searching && !panel.photos.is_empty(), |ui| {
        ui.horizontal(|ui| {
            if ui
                .button(format!("Select… ({})", panel.photos.len()))
                .on_hover_text(crate::i18n::tr("Choose images and save one per canvas"))
                .clicked()
            {
                panel.open_bulk();
            }
            if panel.bulk_busy {
                ui.spinner();
                ui.weak(crate::i18n::tr("Bulk saving…"));
            } else if let Some(msg) = panel.bulk_done_msg.clone() {
                ui.weak(msg);
            }
        });
    });
}

/// Pie de la lista: spinner, error con reintento, «Show more» local (sin
/// coste), «Bring more» (1 token) o avisos de fin/presupuesto.
fn list_footer_ui(panel: &mut Panel, ui: &mut egui::Ui, shown: usize, tx: &Sender<loader::AppMsg>) {
    if panel.searching {
        let stuck = stalled(
            panel.search_started,
            std::time::Instant::now(),
            SEARCH_STALL_SECS,
        );
        if panel.stalled_ui(stuck, ui) {
            return;
        }
        ui.add(egui::Spinner::new().size(26.0));
        return;
    }
    if let Some(err) = panel.error.clone() {
        ui.colored_label(ui.visuals().error_fg_color, err);
        if ui.button(crate::i18n::tr("Try again")).clicked() && panel.photos.is_empty() {
            // Reintento de la página actual con la spec sellada (A09) y
            // sin tocar el gasto (esa llamada ya se pagó).
            if let Some(spec) = panel.active_search.clone() {
                panel.search_seq += 1;
                panel.searching = true;
                panel.search_started = Some(std::time::Instant::now());
                loader::spawn_serper_search(
                    spec.page_request(panel.page.max(1), panel.search_seq),
                    tx.clone(),
                    ui.ctx().clone(),
                );
            }
        }
        return;
    }
    if shown < panel.photos.len() {
        let left = panel.photos.len() - shown;
        if ui
            .button(format!("Show more ({left} already loaded)"))
            .clicked()
        {
            panel.visible_count += SHOW_STEP;
        }
    }
    if panel.budget_exhausted {
        ui.weak(format!(
            "Tokens used: {}/{} — change the query for a new search.",
            panel.tokens_spent,
            panel.budget.calls()
        ));
    } else if panel.reached_end {
        ui.weak(crate::i18n::tr("No more results for this search."));
    } else if panel.can_spend_more() {
        let left = panel.budget.calls() - panel.tokens_spent;
        // La etiqueta nombra la búsqueda que se pagina (A09): si el
        // borrador dice otra cosa, queda claro que «Bring more» continúa
        // los resultados en pantalla en vez de mezclar.
        let more_label = match panel.active_search.as_ref() {
            Some(spec) => format!("Bring more “{}” · 1 token ({left} left)", spec.keyword),
            None => format!("Bring more · 1 token ({left} left)"),
        };
        if ui
            .button(more_label)
            .on_hover_text(crate::i18n::tr(
                "Loads the next page (~100 images, ~2 credits)",
            ))
            .clicked()
        {
            spend_token(panel, tx, ui.ctx());
        }
    }
}

/// Pie de transparencia: gasto REAL (última llamada, sesión y total
/// histórico), filtradas y query efectiva. Serper no expone el saldo por
/// API: el botón abre el dashboard donde se ve.
fn cost_footer_ui(panel: &Panel, settings: &AppSettings, ui: &mut egui::Ui) {
    ui.weak(format!(
        "Searches {}/{} · last {}cr · session {}cr · total {}cr",
        panel.tokens_spent,
        panel.budget.calls(),
        panel.credits_last,
        panel.credits_session,
        settings.serper_credits_total,
    ));
    // Insignia de caché: esta página se re-sirvió sin gastar nada.
    if panel.cached_badge && !panel.photos.is_empty() {
        ui.weak(crate::i18n::tr("cached result · 0 credits spent"));
    }
    if ui
        .link("Serper balance →")
        .on_hover_text(crate::i18n::tr(
            "Serper has no balance API — see the remaining credits on the dashboard",
        ))
        .clicked()
    {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab("https://serper.dev/dashboard"));
    }
    let dropped = panel.filtered.total() + panel.filtered_post;
    if dropped > 0 {
        ui.weak(format!(
            "Filtered {dropped} (host {} · pattern {} · title {} · ratio {} · ext {} · network {})",
            panel.filtered.host,
            panel.filtered.pattern,
            panel.filtered.title,
            panel.filtered.ratio + panel.filtered_post,
            panel.filtered.extension,
            panel.filtered.network,
        ));
    }
    if !panel.last_query.is_empty() {
        ui.weak(format!(
            "Query: {} ({} -site, {} cut)",
            truncate_query(&panel.last_query),
            panel.exclusions_applied,
            panel.exclusions_dropped,
        ));
    }
    if panel.query_simplified && !panel.photos.is_empty() {
        ui.weak(crate::i18n::tr(
            "Free Serper account: query sent without -site:/filters — local filter still applies.",
        ));
    }
}

/// Recorta la query efectiva para el pie (la completa viajó al worker).
fn truncate_query(query: &str) -> String {
    const MAX: usize = 90;
    if query.chars().count() <= MAX {
        return query.to_owned();
    }
    format!("{}…", query.chars().take(MAX).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_search_bar_keeps_controls_inside_sidebar() {
        for width in [180.0, 220.0, 320.0] {
            let ctx = egui::Context::default();
            let mut panel = Panel::default();
            let mut settings = AppSettings::default();
            let (tx, _rx) = std::sync::mpsc::channel();
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0)),
                ));
                search_bar_ui(&mut panel, &mut settings, &mut child, &tx);
                assert!(child.min_rect().width() <= width + 0.5);
                assert!(child.min_rect().height() < 220.0);
            });
        }
    }
}
