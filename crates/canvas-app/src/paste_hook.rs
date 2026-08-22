//! Detecta Ctrl+V / Cmd+V a nivel de eventos crudos del sistema operativo.
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
//! **Windows**: hook de mensajes Win32 (`with_msg_hook` de winit) — el
//! único sitio donde Ctrl+V sigue siendo observable cuando el portapapeles
//! solo tiene un bitmap.
//!
//! **macOS**: `NSEvent` local monitor — equivalente funcional del MSG hook,
//! instalado antes de que winit procese el evento.
//!
//! En ambos casos se deja una señal en un flag atómico que `App::ui`
//! consume una vez por frame, sin importar qué vista esté activa.

use std::sync::atomic::{AtomicBool, Ordering};

static PASTE_REQUESTED: AtomicBool = AtomicBool::new(false);

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

#[cfg(target_os = "macos")]
pub fn install(_builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
    use std::ptr::NonNull;

    use block2::StackBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};

    let mask = NSEventMask::KeyDown;
    let block = StackBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: `event` es un puntero válido que el runtime de Objective-C
        // nos entrega; vivirá mientras dure la llamada al bloque.
        let event_ref = unsafe { event.as_ref() };
        // keyCode 9 = V; ignorar eventos de repetición de tecla.
        if !event_ref.isARepeat()
            && event_ref.keyCode() == 9
            && event_ref
                .modifierFlags()
                .contains(NSEventModifierFlags::Command)
        {
            PASTE_REQUESTED.store(true, Ordering::Relaxed);
        }
        // Devolvemos el evento sin modificarlo para que winit/egui sigan
        // viéndolo normalmente.
        event.as_ptr()
    });

    // SAFETY: El bloque es válido y no captura punteros colgantes.
    // `install` se llama una sola vez desde `main.rs`, así que podemos
    // filtrar el `Retained` devuelto para que el monitor viva lo que dure
    // el proceso.
    let monitor =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };

    if let Some(mon) = monitor {
        std::mem::forget(mon);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn install(_builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {}

/// Lee y limpia la señal de pegado. Se llama una vez por frame en
/// `App::ui`, en cualquier vista, para que no quede pegado de un frame a
/// otro si no había ningún editor abierto para consumirla.
pub fn take_request() -> bool {
    PASTE_REQUESTED.swap(false, Ordering::Relaxed)
}