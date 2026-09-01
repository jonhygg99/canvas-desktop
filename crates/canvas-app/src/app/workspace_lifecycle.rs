//! Ciclo de vida de las ventanas (workspaces): crear, cerrar, enfocar,
//! mantener las ventanas hijas registradas en egui y persistir la lista con
//! throttle. El frame raíz (`mod.rs`) orquesta; aquí vive la mecánica.

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::lock::LockExt;
use crate::settings;

use super::{AppInner, Workspace};

impl AppInner {
    /// Crea un workspace nuevo (ventana) con la bienvenida y lo añade a la
    /// lista. `from_root` indica si el usuario lo pidió desde el menú de una
    /// ventana (se persiste ya) o es uno restaurado del arranque.
    pub(crate) fn new_workspace(&mut self) -> Arc<Mutex<Workspace>> {
        let id = self.next_ws_id;
        self.next_ws_id += 1;
        let viewport = egui::ViewportId::from_hash_of(("canvas-ws", id));
        let ws = Arc::new(Mutex::new(Workspace::new(viewport)));
        self.workspaces.push(Arc::clone(&ws));
        // La persistencia se difiere al final del frame raíz: llamar aquí
        // `persist_workspaces_now` bloquearía TODOS los workspaces, y si
        // este método se invoca con el del frame actual ya lockeado (menú
        // «New Window», Ctrl+N/Ctrl+T) el hilo se bloquearía a sí mismo.
        self.workspaces_dirty = true;
        ws
    }

    /// Persiste la lista de workspaces (documento activo + geometría) en los
    /// ajustes. Se llama al crear/cerrar una ventana, con throttle para la
    /// geometría en vivo, y en `on_exit`.
    pub(crate) fn persist_workspaces_now(&mut self) {
        self.settings.workspaces = self
            .workspaces
            .iter()
            .map(|ws| {
                let ws = ws.lock_ok();
                settings::StoredWorkspace {
                    path: ws.persisted_path(),
                    pos: ws.geometry.map(|(p, _)| [p.x, p.y]),
                    size: ws.geometry.map(|(_, s)| [s.x, s.y]),
                }
            })
            .collect();
        self.settings.save_in_background();
        self.last_workspaces_save = Some(std::time::Instant::now());
    }

    /// Persiste la geometría con un throttle corto (no spam de hilos de
    /// escritura por cada píxel de un arrastre de ventana).
    pub(crate) fn maybe_persist_workspaces(&mut self) {
        let due = self
            .last_workspaces_save
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(2));
        if due {
            self.persist_workspaces_now();
        }
    }

    /// Libera los recursos GPU del workspace que se cierra y lo retira de la
    /// lista. Solo para ventanas hijas (la raíz no se retira nunca).
    pub(crate) fn close_workspace(&mut self, index: usize) {
        debug_assert!(index > 0, "la raíz no se cierra por esta vía");
        // La textura nativa (register_native_texture) vive en el registry del
        // renderer COMPARTIDO: hay que liberarla a mano o la GPU perdería
        // slots con cada ventana cerrada.
        let ws = self.workspaces.remove(index);
        if let Ok(ws) = ws.lock() {
            if let Some(surface) = &ws.surface {
                self.rs.renderer.write().free_texture(&surface.egui_id());
            }
        }
        if self.focused >= self.workspaces.len() {
            self.focused = 0;
        }
        self.persist_workspaces_now();
    }

    /// Enfoca (trae al frente) la ventana de un workspace.
    pub(crate) fn focus_workspace(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(ws_arc) = self.workspaces.get(idx) else {
            return;
        };
        self.focused = idx;
        let viewport = ws_arc.lock_ok().viewport;
        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
        // La ventana objetivo se repinta sola al recibir el foco; la paleta
        // se cierra en el frame siguiente.
        ctx.request_repaint_of(viewport);
    }

    /// Refresca `self.focused` contra el estado real de las ventanas (el SO
    /// manda: Alt+Tab etc. no pasan por aquí).
    pub(super) fn update_focused(&mut self, ctx: &egui::Context) {
        // Solo se actualiza cuando ALGUNA ventana tiene el foco del SO: si
        // ninguna lo tiene (p. ej. el arranque de una segunda instancia roba
        // el foco un instante antes de reenviar su ruta por el socket), se
        // CONSERVA la última enfocada — así una apertura externa aterriza en
        // la ventana que el usuario estaba usando, no siempre en la raíz.
        let focused_now = ctx.input(|i| {
            i.raw
                .viewports
                .iter()
                .find(|(_, info)| info.focused == Some(true))
                .and_then(|(id, _)| {
                    self.workspaces
                        .iter()
                        .position(|ws| ws.lock_ok().viewport == *id)
                })
        });
        if let Some(idx) = focused_now {
            self.focused = idx.min(self.workspaces.len().saturating_sub(1));
        }
    }

    /// Tras los frames: retira las ventanas que pidieron cerrarse y
    /// persiste (throttleado) la geometría.
    pub(super) fn finish_root_frame(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        let mut i = 1;
        while i < self.workspaces.len() {
            if self.workspaces[i].lock_ok().close_requested {
                self.close_workspace(i);
                changed = true;
            } else {
                i += 1;
            }
        }
        if changed {
            // El conmutador pudo quedar apuntando a una ventana retirada.
            if self.focused >= self.workspaces.len() {
                self.focused = 0;
            }
            self.switcher.selected = self
                .switcher
                .selected
                .min(self.workspaces.len().saturating_sub(1));
            let _ = ctx;
        }
        if self.workspaces_dirty {
            self.workspaces_dirty = false;
            self.persist_workspaces_now();
        }
        self.maybe_persist_workspaces();
    }

    /// Crea (o mantiene) las ventanas hijas con `show_viewport_deferred`.
    /// Hay que llamarla cada frame con los MISMOS `ViewportId`, o la ventana
    /// se cierra.
    pub(super) fn spawn_child_viewports(&self, ctx: &egui::Context) {
        if self.workspaces.len() <= 1 {
            return;
        }
        let me = Arc::clone(
            self.me
                .as_ref()
                .expect("me se parchea en App::new antes del primer frame"),
        );
        for (i, ws_arc) in self.workspaces.iter().enumerate().skip(1) {
            let mut ws = ws_arc.lock_ok();
            if ws.close_requested {
                continue;
            }
            let viewport = ws.viewport;
            // La geometría SOLO viaja en el builder que CREA la ventana: la
            // siembra es un contrato puro, probado en
            // `workspace_lifecycle_tests.rs` (ver `seed_builder_geometry`).
            let (spawn_geometry, seeded) = seed_builder_geometry(ws.geometry_seeded, ws.geometry);
            ws.geometry_seeded = seeded;
            let mut builder = egui::ViewportBuilder::default().with_title(ws.label());
            if let Some((pos, size)) = spawn_geometry {
                builder = builder
                    .with_position([pos.x, pos.y])
                    .with_inner_size([size.x, size.y]);
            }
            let ws_arc = Arc::clone(ws_arc);
            let child_ctx = ctx.clone();
            let me = Arc::clone(&me);
            let idx = i;
            ctx.show_viewport_deferred(viewport, builder, move |ui, class| {
                if class == egui::ViewportClass::Deferred {
                    let mut inner = me.lock_ok();
                    if let Some(ws_cur) = inner.workspaces.get(idx) {
                        if Arc::ptr_eq(ws_cur, &ws_arc) {
                            let mut ws = ws_arc.lock_ok();
                            inner.child_frame(ui, &child_ctx, &mut ws, idx);
                        }
                    }
                }
            });
        }
    }
}

/// Siembra de la geometría en el builder de NACIMIENTO de una ventana
/// hija: dado el estado actual del flag y la geometría conocida, devuelve
/// la geometría que el builder debe ofrecer y el nuevo valor del flag.
///
/// Contrato: la PRIMERA vez que se registra el viewport (`seeded ==
/// false`) el builder puede llevar la geometría heredada (Ctrl+N/Ctrl+T);
/// desde entonces NUNCA vuelve a ofrecer tamaño/posición, aunque la
/// geometría capturada en vivo cambie — el flag se siembra también cuando
/// no había geometría (p. ej. `CANVAS_DEBUG_WINDOWS`), porque una
/// geometría posterior es captura, no intención.
///
/// Por qué: eframe parchea este builder cada frame
/// (`ViewportBuilder::patch` emite `InnerSize`/`OuterPosition` ante
/// cualquier cambio) y la geometría que se re-leía cada frame era el rect
/// EXTERIOR, reaplicado como tamaño INTERIOR — la ventana crecía la
/// decoración por frame hasta el límite del área de trabajo (el bug del
/// redimensionado en Windows). Tras el nacimiento `patch` no difiere nada
/// y el redimensionado manual del usuario es soberano.
fn seed_builder_geometry(
    seeded: bool,
    geometry: Option<(egui::Pos2, egui::Vec2)>,
) -> (Option<(egui::Pos2, egui::Vec2)>, bool) {
    if seeded {
        return (None, true);
    }
    (geometry, true)
}

#[cfg(test)]
#[path = "workspace_lifecycle_tests.rs"]
mod tests;
