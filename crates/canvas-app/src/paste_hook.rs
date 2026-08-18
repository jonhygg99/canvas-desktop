//! Enlaza Ctrl+V (y Shift+Insert) a nivel de mensajes crudos de Windows.
//!
//! egui-winit intercepta Ctrl+V para leer el portapapeles como TEXTO
//! (`egui-winit-0.35.0/src/lib.rs:1007-1015`): si el portapapeles solo trae
//! un bitmap (el caso típico al copiar una imagen en el navegador),
//! `clipboard.get()` falla, nunca se emite `egui::Event::Paste` y la tecla
//! se traga (hay un `return` antes de empujar el `Event::Key`). Sin ese
//! evento, `EditorState::handle_shortcuts` nunca se entera del pegado y la
//! ruta que ya existe para imágenes del sistema (`clipboard::system_image`)
//! no llega a dispararse.
//!
//! El único sitio donde Ctrl+V sigue siendo observable en ese caso es el
//! bucle de mensajes de Win32, así que aquí se engancha con `with_msg_hook`
//! y se deja una señal en un flag atómico que `App::ui` consume una vez por
//! frame, sin importar qué vista esté activa.

use std::sync::atomic::{AtomicBool, Ordering};

static PASTE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Registra el hook en Windows; no-op en el resto de sistemas, donde
/// `egui::Event::Paste` no tiene este problema (arboard no compite ahí con
/// la lectura de texto del portapapeles como en Win32).
#[cfg(windows)]
pub fn install(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_INSERT, VK_SHIFT, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{MSG, WM_KEYDOWN};
    use winit::platform::windows::EventLoopBuilderExtWindows;

    builder.with_msg_hook(|msg| {
        // SAFETY: winit garantiza que, en Windows, el puntero que entrega a
        // este hook apunta a un `MSG` válido mientras dura la llamada.
        let msg = unsafe { &*msg.cast::<MSG>() };
        if msg.message == WM_KEYDOWN {
            let key = u16::try_from(msg.wParam.0 & 0xffff).unwrap_or_default();
            let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
            let is_paste_key =
                (key == VK_V.0 && down(VK_CONTROL.0)) || (key == VK_INSERT.0 && down(VK_SHIFT.0));
            if is_paste_key {
                PASTE_REQUESTED.store(true, Ordering::Relaxed);
            }
        }
        // No consumir el mensaje: winit debe seguir viéndolo para que sus
        // propios eventos de teclado/modificadores no se desincronicen.
        false
    });
}

#[cfg(not(windows))]
pub fn install(_builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {}

/// Lee y limpia la señal de pegado. Se llama una vez por frame en
/// `App::ui`, en cualquier vista, para que no quede pegado de un frame a
/// otro si no había ningún editor abierto para consumirla.
pub fn take_request() -> bool {
    PASTE_REQUESTED.swap(false, Ordering::Relaxed)
}
