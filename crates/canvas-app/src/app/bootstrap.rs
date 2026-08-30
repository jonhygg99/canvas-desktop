//! Construcción de la `App`: fuentes egui, renderer vello compartido,
//! listener de segunda instancia, menús nativos y el workspace raíz. Separado
//! de `mod.rs` para que quede solo la maquinaria de frames y los tipos.

use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use canvas_render::CanvasRenderer;
use eframe::egui;

use crate::loader::AppMsg;
use crate::lock::LockExt;
use crate::{menus, settings};

use super::{App, AppInner, MenuMirror, Workspace};

impl App {
    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        initial_path: Option<PathBuf>,
        instance: Option<canvas_shell::InstanceListener>,
    ) -> Result<Self> {
        // egui 0.35 solo trae Ubuntu-Light: `RichText::strong()` solo cambia
        // el COLOR, no el grosor. Para títulos de verdad en negrita (secciones
        // del panel de propiedades) registramos la variante Bold de la misma
        // familia Ubuntu y la usamos vía `FontFamily::Name("Ubuntu-Bold")`.
        {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "Ubuntu-Bold".to_owned(),
                // La ruta es relativa a ESTE archivo: app/ → src → canvas-app.
                egui::FontData::from_static(include_bytes!("../../assets/fonts/Ubuntu-Bold.ttf"))
                    .into(),
            );
            fonts.families.insert(
                egui::FontFamily::Name("Ubuntu-Bold".into()),
                vec!["Ubuntu-Bold".to_owned()],
            );
            cc.egui_ctx.set_fonts(fonts);
        }

        let rs = cc
            .wgpu_render_state
            .as_ref()
            .context("eframe no ha inicializado wgpu (¿backend glow activo?)")?
            .clone();
        let renderer = CanvasRenderer::new(&rs.device)?;
        let (shell_tx, shell_rx) = channel();

        // Rutas de segundas instancias: un hilo acepta conexiones del socket
        // local y las convierte en mensajes para la UI (canal global, no de
        // ningún workspace: se resuelven contra la ventana ENFOCADA).
        if let Some(listener) = instance {
            let tx = shell_tx.clone();
            let ctx = cc.egui_ctx.clone();
            listener.spawn_accept_loop(move |line| {
                let line = line.trim().to_owned();
                if line.is_empty() {
                    let _ = tx.send(AppMsg::FocusWindow);
                } else {
                    let _ = tx.send(AppMsg::OpenPathExternal(PathBuf::from(line)));
                }
                ctx.request_repaint();
            });
        }

        // Menú nativo (Windows): necesita el HWND de la ventana recién creada.
        // Vive en `App` (fuera del `Arc` compartido), no en `AppInner`.
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut native_menus: Option<menus::AppMenus> = None;
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    native_menus = menus::AppMenus::install(h.hwnd.get());
                }
            }
        }

        let settings = settings::AppSettings::load();
        if let Some(m) = native_menus.as_mut() {
            m.set_recents(&settings.recent_files);
        }

        let ws0 = Arc::new(Mutex::new(Workspace::new(egui::ViewportId::ROOT)));
        let mut inner = AppInner {
            workspaces: vec![Arc::clone(&ws0)],
            me: None,
            renderer,
            rs,
            settings,
            thumb_cache: super::window::thumbnail_cache_dir(),
            shell_status: String::new(),
            menu_mirror: MenuMirror::default(),
            applied_theme: None,
            switcher: super::switcher::SwitcherState::default(),
            focused: 0,
            shell_rx,
            shell_tx,
            next_ws_id: 1,
            last_workspaces_save: None,
            workspaces_dirty: false,
            pending_focus: None,
            atlas_regs: 0,
            atlas_reups: 0,
            atlas_log_frames: 0,
            #[cfg(target_os = "macos")]
            tabbed_windows: Default::default(),
            #[cfg(target_os = "macos")]
            tab_anchor: None,
        };

        // La app arranca SIEMPRE en la home (bienvenida) de la única ventana
        // raíz, con su tamaño por defecto: ni las ventanas hijas ni las
        // carpetas/documentos de la sesión anterior se restauran. La lista
        // sigue persistiéndose en `settings.json` como historial, pero aquí
        // se descarta. Una ruta explícita (argv o «Abrir con» del SO) sí abre.
        let _ = std::mem::take(&mut inner.settings.workspaces);

        let path_to_open = initial_path;
        if let Some(path) = path_to_open.clone() {
            if path.exists() {
                let ws0 = Arc::clone(&inner.workspaces[0]);
                let mut ws0 = ws0.lock_ok();
                inner.open_path(&mut ws0, path, &cc.egui_ctx);
            } else {
                tracing::info!("ruta inicial inexistente, se ignora: {}", path.display());
            }
        }
        // Gancho de desarrollo: `CANVAS_DEBUG_WINDOWS=2` (o más) abre la misma
        // ruta inicial en N ventanas del MISMO proceso — un único renderer
        // compartido, el escenario real de varias ventanas sobre la misma
        // carpeta, sin clics. La app empaquetada nunca lo lleva activo; sirve
        // para reproducir y verificar bugs de scopes/atlas en vivo.
        if let Some(n) = std::env::var("CANVAS_DEBUG_WINDOWS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n >= 2)
        {
            for _ in 1..n.min(8) {
                let ws = inner.new_workspace();
                if let Some(path) = path_to_open.as_ref() {
                    if path.exists() {
                        let mut ws = ws.lock_ok();
                        inner.open_path(&mut ws, path.clone(), &cc.egui_ctx);
                    }
                }
            }
            inner.pending_focus = Some(inner.workspaces.len() - 1);
        }
        inner.persist_workspaces_now();

        // El Arc que comparten las ventanas; se patcha `me` con el Arc
        // definitivo una vez creado (`None` mientras se construye; nadie lo
        // lee hasta el primer frame, y las ventanas hijas nacen en él).
        let inner_arc = Arc::new(Mutex::new(inner));
        inner_arc.lock_ok().me = Some(Arc::clone(&inner_arc));
        Ok(Self {
            inner: inner_arc,
            menus: native_menus,
        })
    }
}
