//! Fullscreen propio de macOS, sin Space nuevo.
//!
//! La transición NATIVA de fullscreen (botón verde / `toggleFullScreen:`) choca
//! con las pestañas nativas de la barra de título (`apply_native_tabbing`): al
//! entrar, AppKit sincroniza la barra de pestañas sobre la ventana de la
//! transición (`NSToolbarFullScreenWindow`) y en macOS 26 lanza una excepción
//! asíncrona (`titlebarAccessoryViewControllers not supported for this window
//! style`, `NSWindowStackController _makeTabBarForWindow:visible:`) que ningún
//! `@catch` puede capturar — se lanza en un block de dispatch, escapa por el FFI
//! hacia Rust (sin handler de excepciones ObjC) y aborta el proceso en el
//! unwinder. Por eso el fullscreen de esta app en macOS es un «simple
//! fullscreen»: auto-oculta menú/dock y llena la pantalla SIN crear un Space ni
//! ejecutar el controlador de transición. Las pestañas nativas siguen
//! funcionando.
//!
//! Dos restricciones verificadas contra macOS 26 (el `NSWindowStackController`
//! lanza la misma excepción asíncrona e in capturable en TODA operación de la
//! barra de pestañas):
//!
//! - El simple fullscreen NO toca el `styleMask`: quitar la barra de título
//!   hace que el stack controller sincronice (remoción de) la barra de pestañas
//!   y se cuelga el proceso con 2+ ventanas. Se mantiene el estilo: la barra de
//!   título y la de pestañas quedan visibles en fullscreen (como Safari).
//! - El botón verde se neutraliza con `NSWindowCollectionBehavior`
//!   (`FullScreenNone` + sin `FullScreenPrimary`/`FullScreenAuxiliary`): hacer
//!   zoom no basta — una ventana estándar sigue siendo fullscreen-capaz POR
//!   DEFECTO aunque se quiten los bits de fullscreen; `FullScreenNone` es el
//!   opt-out explícito.
//!
//! Al tocar ventanas de egui hay una NSWindow fantasma de infraestructura que
//! puede lanzar NSExceptions; toda manipulación va envuelta en
//! `objc2::exception::catch` — una NSException sin capturar aborta el proceso.

use objc2::exception::catch;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationPresentationOptions, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::NSRect;

use super::Workspace;

/// Estado de la ventana antes de entrar en fullscreen, para restaurarlo al salir.
#[derive(Clone, Copy)]
pub(crate) struct SavedFullscreen {
    frame: NSRect,
    style_mask: NSWindowStyleMask,
    presentation: NSApplicationPresentationOptions,
    movable: bool,
}

/// Alterna el fullscreen simple de la ventana principal. `ws` guarda el estado
/// restaurable mientras dura el fullscreen (cada ventana tiene el suyo).
pub(crate) fn toggle(ws: &mut Workspace) {
    if let Some(saved) = ws.macos_simple_fs.take() {
        exit_simple_fullscreen(saved);
    } else if let Some(saved) = enter_simple_fullscreen() {
        ws.macos_simple_fs = Some(saved);
    }
}

/// Entra en fullscreen simple: guarda el estado, auto-oculta el menú/dock y
/// llena la pantalla. Devuelve el estado a restaurar (o `None` si no hay
/// ventana principal disponible).
///
/// IMPORTANTE: NO toca el `styleMask`. Con las pestañas nativas activas,
/// quitar la barra de título hace que el `NSWindowStackController` sincronice
/// la barra de pestañas (remoción) y lance en macOS 26 la excepción asíncrona
/// `titlebarAccessoryViewControllers not supported for this window style` — la
/// misma que crashea la transición nativa — en un block de dispatch que ningún
/// `catch` puede capturar. Mantener el estilo (barra de título y pestañas
/// visibles, como Safari en fullscreen) evita esa sincronización.
pub(crate) fn enter_simple_fullscreen() -> Option<SavedFullscreen> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let result = catch(std::panic::AssertUnwindSafe(|| {
        let window = app.mainWindow()?;
        let screen = window.screen()?;
        let saved = SavedFullscreen {
            frame: window.frame(),
            style_mask: window.styleMask(),
            presentation: app.presentationOptions(),
            movable: window.isMovable(),
        };
        app.setPresentationOptions(
            NSApplicationPresentationOptions::AutoHideDock
                | NSApplicationPresentationOptions::AutoHideMenuBar,
        );
        window.setFrame_display(screen.frame(), true);
        window.setMovable(false);
        Some(saved)
    }));
    match result {
        Ok(saved) => saved,
        Err(e) => {
            tracing::debug!("simple fullscreen: excepción al entrar: {e:?}");
            None
        }
    }
}

/// Sale del fullscreen simple y restaura la geometría, el estilo y las opciones
/// de presentación previas.
pub(crate) fn exit_simple_fullscreen(saved: SavedFullscreen) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let result = catch(std::panic::AssertUnwindSafe(|| {
        if let Some(window) = app.mainWindow() {
            app.setPresentationOptions(saved.presentation);
            window.setStyleMask(saved.style_mask);
            window.setFrame_display(saved.frame, true);
            window.setMovable(saved.movable);
        }
    }));
    if let Err(e) = result {
        tracing::debug!("simple fullscreen: excepción al salir: {e:?}");
    }
}

/// Quita la capacidad de fullscreen NATIVO de la ventana: el botón verde pasa a
/// hacer zoom (maximizar) en vez de lanzar la transición que crashea con las
/// pestañas activas. Se llama una vez por ventana, desde `apply_native_tabbing`.
///
/// Con las pestañas nativas activas no basta con quitar
/// `FullScreenPrimary`/`FullScreenAuxiliary`: una ventana estándar sigue siendo
/// fullscreen-capaz POR DEFECTO, y el botón verde lanzaría la transición nativa.
/// `FullScreenNone` es el opt-out explícito de macOS para esa ventana.
pub(crate) fn neutralize_fullscreen_button(window: &NSWindow) {
    let behavior = (window.collectionBehavior()
        & !NSWindowCollectionBehavior::FullScreenPrimary
        & !NSWindowCollectionBehavior::FullScreenAuxiliary)
        | NSWindowCollectionBehavior::FullScreenNone;
    window.setCollectionBehavior(behavior);
}
