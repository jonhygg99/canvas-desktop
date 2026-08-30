//! Detección de RAM por plataforma, sin dependencias nuevas: `windows` (ya
//! en el árbol) en Windows, `libc::sysctlbyname` en macOS (libc ya está en
//! el árbol vía winit/wgpu/objc2), y `/proc/meminfo` en Linux. Cualquier
//! fallo devuelve `None` y el llamador cae al presupuesto histórico.
//!
//! Hay dos consultas con distinta cadencia:
//! - La RAM física TOTAL no cambia: se detecta una sola vez por proceso y
//!   se cachea en un `OnceLock`.
//! - La RAM LIBRE sí cambia: es la señal de presión de memoria que reduce
//!   el presupuesto de la baraja, y la consulta es barata (una syscall o un
//!   `read` de `/proc/meminfo`), así que se hace en cada llamada.

use std::sync::OnceLock;

/// Umbral de «poca RAM»: por debajo de 2 GiB libres, la caché de la baraja
/// reduce su presupuesto dinámicamente (ver `budget_under_free_ram` en
/// cache.rs) y `App` avisa antes de un «Save all» masivo. Con 1 GiB libres
/// el presupuesto cae a la mitad; con 512 MiB o menos, al mínimo. Un solo
/// umbral para ambos: es la misma señal de presión.
pub(crate) const FREE_RAM_REDUCTION_THRESHOLD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Umbral de «RAM crítica»: por debajo de 512 MiB libres se detiene el
/// trabajo NO esencial — los guardados/exportaciones nuevos fallan rápido
/// sin tocar el archivo y la baraja deja de precargar ranuras de fondo
/// (`is_critical_free_ram`). Es el suelo del presupuesto de la caché
/// (`MIN_EVICT_BUDGET_BYTES`): coherente que sea ese el punto de parada, no
/// el primer síntoma de presión (ese es `FREE_RAM_REDUCTION_THRESHOLD_BYTES`,
/// que solo reduce el ritmo).
pub(crate) const CRITICAL_FREE_RAM_BYTES: u64 = 512 * 1024 * 1024;

/// RAM física total de la máquina, en bytes, o `None` si no se pudo
/// determinar (plataforma desconocida, syscall fallida, `/proc` ausente).
/// `pub(crate)` para que el render en vivo (`editor/canvas/paint.rs`) pueda
/// pedir el presupuesto GPU del documento activo vía la re-exportación de
/// `deck`.
pub(crate) fn total_physical_ram_bytes() -> Option<u64> {
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(detect_ram_bytes)
}

/// RAM libre (disponible) en bytes, o `None` si no se pudo determinar.
/// Cada plataforma aproxima «lo que una app podría usar sin apretar al
/// sistema»: `ullAvailPhys` (incluye la lista standby) en Windows,
/// `MemAvailable` (incluye la caché de página reclamable) en Linux, y
/// páginas free + speculative + purgeable en macOS — libre a secas es
/// crónicamente bajo en macOS porque el sistema vive de caché de archivos,
/// y el propio OS cuenta esas tres clases como reclamables para decidir
/// presión. `pub(crate)` porque `App` la usa para el aviso de poca RAM
/// antes de «Save all».
pub(crate) fn free_ram_bytes() -> Option<u64> {
    detect_free_ram_bytes()
}

/// ¿Está el sistema en RAM crítica (por debajo de `CRITICAL_FREE_RAM_BYTES`
/// libres)? `None` (no medible) no cuenta como crítico: sin señal no se
/// detiene nada. Pura, y por eso se prueba con tabla (no depende del
/// hardware). Es el guard común de `persistence.rs` (abortar un Save/Export
/// antes del bake) y de `request_loads` (pausar la precarga de fondo).
pub(crate) fn is_critical_free_ram(free_bytes: Option<u64>) -> bool {
    matches!(free_bytes, Some(bytes) if bytes < CRITICAL_FREE_RAM_BYTES)
}

#[cfg(target_os = "windows")]
fn detect_ram_bytes() -> Option<u64> {
    use windows::Win32::System::SystemInformation::GetPhysicallyInstalledSystemMemory;
    let mut kb = 0u64;
    // SAFETY: `kb` es un puntero válido a un `u64`; la función solo escribe
    // ahí cuando devuelve `true`.
    unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }
        .as_bool()
        .then_some(kb.saturating_mul(1024))
}

#[cfg(target_os = "macos")]
fn detect_ram_bytes() -> Option<u64> {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `size` es un buffer válido de `len` bytes, y los punteros
    // `newp`/`newlen` nulos indican una consulta de solo lectura.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut size as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then_some(size)
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn detect_ram_bytes() -> Option<u64> {
    // `/proc/meminfo` expone `MemTotal:` en kB (Linux y demás Unix con
    // procfs); leerlo es solo E/S de archivo, sin syscalls extrañas.
    meminfo_kb("MemTotal:")
}

#[cfg(target_os = "windows")]
fn detect_free_ram_bytes() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX::default();
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` es válido y con `dwLength` correcto; la función
    // rellena el resto de campos cuando devuelve éxito.
    unsafe { GlobalMemoryStatusEx(&mut status) }
        .is_ok()
        .then_some(status.ullAvailPhys)
}

#[cfg(target_os = "macos")]
fn detect_free_ram_bytes() -> Option<u64> {
    // Páginas que el OS considera reclamables para presión: libres +
    // especulativas + purgeables. `vm.page_inactive_count` NO existe como
    // oid en este macOS; no hay que usarlo.
    let mut pages = 0u64;
    for oid in [
        c"vm.page_free_count",
        c"vm.page_speculative_count",
        c"vm.page_purgeable_count",
    ] {
        let mut value = 0u64;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: consulta de solo lectura con buffer válido, igual que
        // `detect_ram_bytes`.
        let rc = unsafe {
            libc::sysctlbyname(
                oid.as_ptr(),
                (&mut value as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        pages += value;
    }
    // `hw.pagesize` es un entero de 4 bytes; buffer `u32` para que la
    // escritura de la syscall quepa exacta.
    let mut page_size: u32 = 0;
    let mut len = std::mem::size_of::<u32>();
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.pagesize".as_ptr(),
            (&mut page_size as *mut u32).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    Some(pages.saturating_mul(page_size as u64))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn detect_free_ram_bytes() -> Option<u64> {
    // `MemAvailable` (Linux ≥ 3.14) incluye la caché de página reclamable,
    // mejor señal de «cuánta RAM puede usar una app» que `MemFree`; en
    // Unix sin ella se cae a `MemFree`.
    meminfo_kb("MemAvailable:").or_else(|| meminfo_kb("MemFree:"))
}

/// Lee un campo `Clave:` de `/proc/meminfo` (en kB) y lo devuelve en bytes.
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn meminfo_kb(key: &str) -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|line| line.starts_with(key))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb.saturating_mul(1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabla del guard común de RAM crítica (Task 3 del plan de memoria): el
    /// predicado que aborta Save/Export antes del bake. El umbral es exclusivo
    /// inferior — exactamente `CRITICAL_FREE_RAM_BYTES` no es crítico; por
    /// debajo sí. `None` (no medible) nunca cuenta: sin señal no se para nada.
    #[test]
    fn is_critical_free_ram_boundary_table() {
        let threshold = CRITICAL_FREE_RAM_BYTES;
        let cases = [
            // (bytes libres, ¿crítico?)
            (None, false),                       // no medible → no se para
            (Some(threshold + 1), false),        // cómodo
            (Some(threshold), false),            // justo en el umbral
            (Some(threshold - 1), true),         // un byte por debajo
            (Some(512 * 1024 * 1024 / 2), true), // holgadamente por debajo
            (Some(0), true),                     // cero
        ];
        for (free, expected) in cases {
            assert_eq!(is_critical_free_ram(free), expected, "free={free:?}");
        }
    }
}
