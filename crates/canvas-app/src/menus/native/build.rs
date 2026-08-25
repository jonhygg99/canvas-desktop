//! Construccion del arbol de menus nativo con `muda` y la tabla de
//! aceleradores que solo se muestran como texto (los atajos de verdad los
//! sigue gestionando egui: los aceleradores nativos de muda necesitarian
//! `TranslateAcceleratorW`, que eframe no expone).

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

use super::AppMenus;

fn accel(mods: Modifiers, code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Some(mods), code))
}

impl AppMenus {
    pub(super) fn build(hwnd: isize) -> Result<Self, muda::Error> {
        let ctrl = Modifiers::CONTROL;
        let ctrl_shift = Modifiers::CONTROL | Modifiers::SHIFT;
        let ctrl_alt = Modifiers::CONTROL | Modifiers::ALT;

        let new_item = MenuItem::with_id("new", "New Design", true, accel(ctrl_shift, Code::KeyN));
        let new_window_item =
            MenuItem::with_id("new_window", "New Window", true, accel(ctrl, Code::KeyN));
        let open_item = MenuItem::with_id("open", "Open…", true, accel(ctrl, Code::KeyO));
        let open_folder_item = MenuItem::with_id(
            "open_folder",
            "Open Folder…",
            true,
            accel(ctrl_shift, Code::KeyO),
        );
        let close_project_item = MenuItem::with_id("close_project", "Close Project", true, None);
        let save_item = MenuItem::with_id("save", "Save", false, accel(ctrl, Code::KeyS));
        let save_as_item =
            MenuItem::with_id("save_as", "Save As…", false, accel(ctrl_shift, Code::KeyS));
        let save_all_item =
            MenuItem::with_id("save_all", "Save All", false, accel(ctrl_alt, Code::KeyS));
        let export_item =
            MenuItem::with_id("export", "Export…", false, accel(ctrl_shift, Code::KeyE));
        let recent_menu = Submenu::with_id("recent", "Open Recent", true);
        let quit_item = MenuItem::with_id("quit", "Quit", true, accel(ctrl, Code::KeyQ));

        let file = Submenu::with_items(
            "&File",
            true,
            &[
                &new_item,
                &new_window_item,
                &open_item,
                &open_folder_item,
                &recent_menu,
                &close_project_item,
                &PredefinedMenuItem::separator(),
                &save_item,
                &save_as_item,
                &save_all_item,
                &export_item,
                &PredefinedMenuItem::separator(),
                &quit_item,
            ],
        )?;

        let undo_item = MenuItem::with_id("undo", "Undo", false, accel(ctrl, Code::KeyZ));
        let redo_item = MenuItem::with_id("redo", "Redo", false, accel(ctrl, Code::KeyY));
        let cut_item = MenuItem::with_id("cut", "Cut", false, accel(ctrl, Code::KeyX));
        let copy_item = MenuItem::with_id("copy", "Copy", false, accel(ctrl, Code::KeyC));
        let paste_item = MenuItem::with_id("paste", "Paste", false, accel(ctrl, Code::KeyV));
        let duplicate_item =
            MenuItem::with_id("duplicate", "Duplicate", false, accel(ctrl, Code::KeyD));
        let delete_item = MenuItem::with_id("delete", "Delete", false, None);
        let select_all_item =
            MenuItem::with_id("select_all", "Select All", false, accel(ctrl, Code::KeyA));
        let group_item = MenuItem::with_id("group", "Group", false, accel(ctrl, Code::KeyG));
        let ungroup_item =
            MenuItem::with_id("ungroup", "Ungroup", false, accel(ctrl_shift, Code::KeyG));

        let edit = Submenu::with_items(
            "&Edit",
            true,
            &[
                &undo_item,
                &redo_item,
                &PredefinedMenuItem::separator(),
                &cut_item,
                &copy_item,
                &paste_item,
                &duplicate_item,
                &delete_item,
                &PredefinedMenuItem::separator(),
                &select_all_item,
                &PredefinedMenuItem::separator(),
                &group_item,
                &ungroup_item,
            ],
        )?;

        let zoom_in_item = MenuItem::with_id("zoom_in", "Zoom In", false, accel(ctrl, Code::Equal));
        let zoom_out_item =
            MenuItem::with_id("zoom_out", "Zoom Out", false, accel(ctrl, Code::Minus));
        let fit_item = MenuItem::with_id("fit", "Fit to Window", false, accel(ctrl, Code::Digit0));
        let grid_item = MenuItem::with_id("grid", "Grid", false, None);
        let rulers_item = MenuItem::with_id("rulers", "Rulers", false, None);
        let next_canvas_item = MenuItem::with_id("next_canvas", "Next Canvas", false, None);
        let prev_canvas_item = MenuItem::with_id("prev_canvas", "Previous Canvas", false, None);
        let canvases_panel_item =
            MenuItem::with_id("canvases_panel", "Canvases Panel", false, None);
        let canvases_axis_item = MenuItem::with_id("canvases_axis", "Canvases Axis", false, None);
        let canvases_side_item =
            MenuItem::with_id("canvases_side", "Canvases Panel Side", false, None);
        let layers_panel_item = MenuItem::with_id("layers_panel", "Layers Panel", false, None);
        let add_canvas_item = MenuItem::with_id("add_canvas", "Add Canvas", false, None);
        let full_screen_item = MenuItem::with_id("full_screen", "Full Screen", true, None);

        let view = Submenu::with_items(
            "&View",
            true,
            &[
                &zoom_in_item,
                &zoom_out_item,
                &fit_item,
                &PredefinedMenuItem::separator(),
                &grid_item,
                &rulers_item,
                &PredefinedMenuItem::separator(),
                &prev_canvas_item,
                &next_canvas_item,
                &canvases_panel_item,
                &canvases_axis_item,
                &canvases_side_item,
                &layers_panel_item,
                &add_canvas_item,
                &PredefinedMenuItem::separator(),
                &full_screen_item,
            ],
        )?;

        let settings_item = MenuItem::with_id("settings", "Settings…", true, None);
        let about_item = MenuItem::with_id("about", "About Canvas Desktop", true, None);
        let help = Submenu::with_items("&Help", true, &[&settings_item, &about_item])?;

        let menu = Menu::with_items(&[&file, &edit, &view, &help])?;
        // SAFETY: el HWND viene de la ventana viva de eframe; muda
        // subclasea su WndProc para pintar y despachar el menú.
        unsafe { menu.init_for_hwnd(hwnd)? };

        let editor_items = vec![
            save_item,
            save_as_item,
            save_all_item,
            export_item,
            undo_item.clone(),
            redo_item.clone(),
            zoom_in_item,
            zoom_out_item,
            fit_item,
            grid_item,
            rulers_item,
            next_canvas_item,
            prev_canvas_item,
            canvases_panel_item,
            canvases_axis_item,
            canvases_side_item,
            layers_panel_item,
            add_canvas_item,
            cut_item,
            copy_item,
            paste_item,
            duplicate_item,
            delete_item,
            select_all_item,
            group_item,
            ungroup_item,
        ];

        Ok(Self {
            _menu: menu,
            recent_menu,
            recent_items: Vec::new(),
            editor_items,
            editor_enabled: false,
            undo_item,
            redo_item,
        })
    }
}
