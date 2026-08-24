//! Atajos de teclado del editor. Se leen una sola vez por frame, antes de
//! cualquier edicion: el orden entre los mas especificos y los mas generales
//! importa (Ctrl+Shift+Z antes que Ctrl+Z, y asi).

use eframe::egui;

use super::{DeckNav, EditorState};

impl EditorState {
    /// Atajos de edición globales del editor (deshacer/rehacer).
    ///
    /// `paste_requested` es la señal de Ctrl+V/Shift+Insert capturada por
    /// `paste_hook` a nivel de mensajes de Windows: en Win32, egui-winit se
    /// traga esa combinación sin emitir `Event::Paste` cuando el
    /// portapapeles solo tiene un bitmap (ver `paste_hook.rs`), así que en
    /// esa plataforma es la única señal fiable.
    pub fn handle_shortcuts(
        &mut self,
        ctx: &egui::Context,
        paste_requested: bool,
        deck_renaming: bool,
    ) {
        use egui::{Event, Key, KeyboardShortcut, Modifiers};
        // Deshacer/rehacer se evalúan primero y con su propia guarda: un
        // `TextEdit` con foco propio (renombrar una capa, editar su texto, o
        // renombrar una ranura de la baraja) debe quedarse con Ctrl+Z para su
        // propio undo, no el del documento. `ctx.text_edit_focused()` es
        // DEMASIADO ancho para eso — en egui 0.35 también es `true` mientras
        // se edita un `DragValue` del panel de propiedades por teclado (usa
        // un `TextEdit` interno con el mismo id), lo que dejaba Ctrl+Z muerto
        // tras tocar X/Y/W/H/Scale hasta hacer clic en otro sitio. Por eso
        // aquí se miran las banderas propias del editor en vez de esa guarda
        // global.
        let editing_own_text = self.rename_edit.is_some()
            || self.file_rename_edit.is_some()
            || self.content_edit.is_some()
            || deck_renaming;
        if !editing_own_text {
            // El orden importa: Ctrl+Shift+Z debe consumirse antes que Ctrl+Z.
            let redo = ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers::COMMAND | Modifiers::SHIFT,
                    Key::Z,
                )) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
            });
            let undo = ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))
            });
            if redo {
                self.redo();
            } else if undo {
                self.undo();
            }
        }

        // El resto de atajos (portapapeles, Supr, navegación de baraja…) sí
        // le siguen cediendo el paso a cualquier `TextEdit` con foco — ese es
        // el caso general que `text_edit_focused()` describe bien.
        if ctx.text_edit_focused() {
            return;
        }

        // Ctrl+Shift+G (desagrupar) antes que Ctrl+G (agrupar): mismo patrón
        // que redo/undo arriba, lo más específico primero.
        let ungroup = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::G,
            ))
        });
        let group = ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::G)));
        if ungroup {
            crate::layers_panel::ungroup_selection(self);
        } else if group {
            crate::layers_panel::group_selection(self);
        }

        // Ctrl+X/C no llegan como pulsaciones de tecla normales: winit los
        // intercepta para la integración con el portapapeles del SO y egui
        // los entrega como `Event::Cut`/`Copy`, así que `consume_shortcut`
        // nunca los ve — hay que mirar los eventos crudos.
        let (want_cut, want_copy, event_paste) = ctx.input(|i| {
            let mut cut = false;
            let mut copy = false;
            let mut paste = false;
            for ev in &i.events {
                match ev {
                    Event::Cut => cut = true,
                    Event::Copy => copy = true,
                    Event::Paste(_) => paste = true,
                    _ => {}
                }
            }
            (cut, copy, paste)
        });
        if want_cut {
            crate::clipboard::cut(self);
        }
        if want_copy {
            crate::clipboard::copy(self);
        }
        // `paste_requested` llega del hook del SO (MSG en Windows, NSEvent
        // monitor en macOS) y cubre el caso en que el portapapeles solo
        // trae un bitmap: egui-winit se traga Cmd/Ctrl+V sin emitir
        // `Event::Paste`. Cuando `Event::Paste` sí llega (p. ej. pegado de
        // texto), también se acepta como señal válida.
        let want_paste = paste_requested || event_paste;
        if want_paste && !crate::clipboard::paste(self) {
            self.save_error = Some(crate::clipboard::PASTE_EMPTY_MSG.to_owned());
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::D)))
        {
            crate::clipboard::duplicate(self);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::A)))
        {
            crate::clipboard::select_all(self);
        }
        let delete = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Delete))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Backspace))
        });
        if delete {
            crate::editor::delete_selected(self);
        }

        // Navegación entre lienzos de la baraja. `Ctrl+PageUp/Down` es un
        // alias (memoria muscular de pestañas de navegador); las flechas se
        // dejan libres a propósito para el futuro «mover capa con teclado».
        if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::PageDown))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::PageDown))
        }) {
            self.deck_nav = Some(DeckNav::Next);
        } else if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::PageUp))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::PageUp))
        }) {
            self.deck_nav = Some(DeckNav::Prev);
        } else if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Home)))
        {
            self.deck_nav = Some(DeckNav::First);
        } else if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::End)))
        {
            self.deck_nav = Some(DeckNav::Last);
        }
    }
}
