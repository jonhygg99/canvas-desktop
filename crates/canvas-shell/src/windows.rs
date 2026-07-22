//! Integración con el Explorador de Windows: asociaciones «Abrir con» bajo
//! `HKCU\Software\Classes` (sin permisos de administrador) y menú contextual
//! de carpetas. Tras cada cambio se notifica al shell con `SHChangeNotify`,
//! o el Explorador no lo reflejaría hasta reiniciarlo.
//!
//! Estas claves son la LISTA CANÓNICA de lo que toca el registro: el
//! instalador debe escribir y limpiar exactamente las mismas.

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

use crate::integration::{ShellError, ShellIntegration};

/// ProgID con el que se registran todas las asociaciones.
pub const PROGID: &str = "CanvasDesktop.Image";

/// Extensiones asociadas (las que la app sabe abrir + el proyecto propio).
pub const ASSOC_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "webp", "svg", "gif", "bmp", "canvas"];

const DIR_SHELL_KEY: &str = r"Software\Classes\Directory\shell\CanvasDesktop";
const DIR_BG_SHELL_KEY: &str = r"Software\Classes\Directory\Background\shell\CanvasDesktop";
const MENU_LABEL: &str = "Open with Canvas Desktop";

pub struct WindowsShell;

impl ShellIntegration for WindowsShell {
    fn register_file_associations(&self, exe: &Path) -> Result<(), ShellError> {
        let exe = exe.display().to_string();
        let progid_root = format!(r"Software\Classes\{PROGID}");

        // ProgID: nombre, icono y comando de apertura.
        set_value(&progid_root, None, "Canvas Desktop Image")?;
        set_value(
            &format!(r"{progid_root}\DefaultIcon"),
            None,
            &format!("\"{exe}\",0"),
        )?;
        set_value(
            &format!(r"{progid_root}\shell\open\command"),
            None,
            &format!("\"{exe}\" \"%1\""),
        )?;

        // Cada extensión ofrece el ProgID en su lista de «Abrir con».
        for ext in ASSOC_EXTENSIONS {
            set_value(
                &format!(r"Software\Classes\.{ext}\OpenWithProgids"),
                Some(PROGID),
                "",
            )?;
        }

        // Menú contextual de carpetas («%1» = la carpeta pulsada) y del fondo
        // de una carpeta abierta («%V» = la carpeta actual).
        set_value(DIR_SHELL_KEY, None, MENU_LABEL)?;
        set_value(DIR_SHELL_KEY, Some("Icon"), &format!("\"{exe}\",0"))?;
        set_value(
            &format!(r"{DIR_SHELL_KEY}\command"),
            None,
            &format!("\"{exe}\" \"%1\""),
        )?;
        set_value(DIR_BG_SHELL_KEY, None, MENU_LABEL)?;
        set_value(DIR_BG_SHELL_KEY, Some("Icon"), &format!("\"{exe}\",0"))?;
        set_value(
            &format!(r"{DIR_BG_SHELL_KEY}\command"),
            None,
            &format!("\"{exe}\" \"%V\""),
        )?;

        notify_shell();
        Ok(())
    }

    fn unregister_file_associations(&self) -> Result<(), ShellError> {
        // Borra exactamente lo que `register_file_associations` creó; lo que
        // ya no exista se ignora.
        delete_tree(&format!(r"Software\Classes\{PROGID}"))?;
        for ext in ASSOC_EXTENSIONS {
            delete_value(&format!(r"Software\Classes\.{ext}\OpenWithProgids"), PROGID)?;
        }
        delete_tree(DIR_SHELL_KEY)?;
        delete_tree(DIR_BG_SHELL_KEY)?;

        notify_shell();
        Ok(())
    }

    fn update_jump_list(&self, recents: &[PathBuf]) -> Result<(), ShellError> {
        update_jump_list_impl(recents).map_err(|e| ShellError::Registry(format!("jump list: {e}")))
    }
}

/// AppUserModelID propio: hace que la Jump List y la barra de tareas
/// correspondan a esta app. Debe llamarse ANTES de crear la ventana.
pub fn set_app_user_model_id() {
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    unsafe {
        if let Err(e) = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from("CanvasDesktop.App"))
        {
            tracing::warn!("SetCurrentProcessExplicitAppUserModelID falló: {e}");
        }
    }
}

/// Publica los recientes como categoría de la Jump List de la barra de
/// tareas. COM con apartment STA propio del hilo llamador.
fn update_jump_list_impl(recents: &[PathBuf]) -> windows::core::Result<()> {
    use windows::core::Interface;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
    use windows::Win32::System::Com::StructuredStorage::{
        InitPropVariantFromStringAsVector, PropVariantClear,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };

    let exe = std::env::current_exe().map_err(|e| {
        windows::core::Error::new(windows::core::HRESULT(-1), format!("current_exe: {e}"))
    })?;

    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let list: ICustomDestinationList =
                CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
            let mut slots = 0u32;
            let _removed: IObjectArray = list.BeginList(&mut slots)?;

            let collection: IObjectCollection =
                CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
            for path in recents.iter().take(10) {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                link.SetPath(&HSTRING::from(exe.as_os_str()))?;
                link.SetArguments(&HSTRING::from(format!("\"{}\"", path.display())))?;
                link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;

                // El texto visible del elemento es la propiedad Title.
                let store: IPropertyStore = link.cast()?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let mut title = InitPropVariantFromStringAsVector(&HSTRING::from(name))?;
                store.SetValue(&PKEY_Title, &title)?;
                store.Commit()?;
                let _ = PropVariantClear(&mut title);

                collection.AddObject(&link)?;
            }

            let array: IObjectArray = collection.cast()?;
            list.AppendCategory(&HSTRING::from("Recent"), &array)?;
            list.CommitList()?;
            Ok(())
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

fn registry_err(context: &str, err: WIN32_ERROR) -> ShellError {
    ShellError::Registry(format!("{context}: error {}", err.0))
}

/// Crea (si hace falta) `HKCU\{subkey}` y escribe un valor REG_SZ. `name`
/// `None` escribe el valor por defecto de la clave.
fn set_value(subkey: &str, name: Option<&str>, data: &str) -> Result<(), ShellError> {
    let subkey_w = HSTRING::from(subkey);
    let mut key = HKEY::default();
    unsafe {
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &subkey_w,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if rc.is_err() {
            return Err(registry_err(subkey, rc));
        }
        // REG_SZ en UTF-16 con terminador nulo.
        let data_w: Vec<u16> = data.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(data_w.as_ptr().cast::<u8>(), data_w.len() * 2);
        // `name` None → valor por defecto de la clave (lpValueName nulo).
        let name_w = name.map(HSTRING::from);
        let name_pc = name_w
            .as_ref()
            .map(|h| PCWSTR(h.as_ptr()))
            .unwrap_or(PCWSTR::null());
        let rc = RegSetValueExW(key, name_pc, None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
        if rc.is_err() {
            return Err(registry_err(subkey, rc));
        }
    }
    Ok(())
}

/// Borra recursivamente `HKCU\{subkey}`; si no existe, no es un error.
fn delete_tree(subkey: &str) -> Result<(), ShellError> {
    let subkey_w = HSTRING::from(subkey);
    unsafe {
        let rc = RegDeleteTreeW(HKEY_CURRENT_USER, &subkey_w);
        if rc.is_err() && rc != ERROR_FILE_NOT_FOUND {
            return Err(registry_err(subkey, rc));
        }
    }
    Ok(())
}

/// Borra un valor de `HKCU\{subkey}`; clave o valor ausentes no son error.
fn delete_value(subkey: &str, name: &str) -> Result<(), ShellError> {
    let subkey_w = HSTRING::from(subkey);
    let mut key = HKEY::default();
    unsafe {
        let rc = RegOpenKeyExW(HKEY_CURRENT_USER, &subkey_w, None, KEY_SET_VALUE, &mut key);
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if rc.is_err() {
            return Err(registry_err(subkey, rc));
        }
        let rc = RegDeleteValueW(key, &HSTRING::from(name));
        let _ = RegCloseKey(key);
        if rc.is_err() && rc != ERROR_FILE_NOT_FOUND {
            return Err(registry_err(subkey, rc));
        }
    }
    Ok(())
}

/// Avisa al Explorador de que las asociaciones cambiaron (sin esto no se
/// reflejan hasta reiniciar el shell).
fn notify_shell() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}
