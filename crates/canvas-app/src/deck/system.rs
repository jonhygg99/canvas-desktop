//! Detección de RAM por plataforma, sin dependencias nuevas: `windows` (ya
//! en el árbol) en Windows, `libc::sysctlbyname` en macOS (libc ya está en
//! el árbol vía winit/wgpu/objc2), y `/proc/meminfo` en Linux. Cualquier
//! fallo devuelve `None` y el llamador cae al presupuesto histórico.
//!
//! Hay dos consultas con distinta cadencia:
//! - La RAM física TOTAL no cambia: se detecta una sola vez por proceso y
//!   se cachea en un `OnceLock`.
//! - La RAM LIBRE sí cambia: es la señal de presión de memoria que reduce
//!   el presupuesto de la baraja, y la consulta es barata (una o dos
//!   syscalls, o un `read` de `/proc/meminfo`), así que se hace en cada
//!   llamada. En macOS mide como el OS — incluye la caché de archivos
//!   reclamable (`inactive`) y cruza con el nivel oficial de presión
//!   (`kern.memorystatus_vm_pressure_level`) como techo duro; sin eso, un
//!   Mac normal lleno de caché marca «crítico» en falso y bloquea guardados
//!   con el sistema al 78 % de RAM libre y sin swap.

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
    // SAFETY: `kb` es un puntero válido a un `u64`; la función escribe ahí
    // solo cuando devuelve `Ok(())`.
    unsafe { GetPhysicallyInstalledSystemMemory(&mut kb).is_ok() }
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
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` es válido y con `dwLength` correcto; la función
    // rellena el resto de campos cuando devuelve éxito.
    unsafe { GlobalMemoryStatusEx(&mut status) }
        .is_ok()
        .then_some(status.ullAvailPhys)
}

#[cfg(target_os = "macos")]
fn detect_free_ram_bytes() -> Option<u64> {
    let stats = vm_statistics()?;
    let bytes = reclaimable_free_bytes(
        u64::from(stats.free_count),
        u64::from(stats.speculative_count),
        u64::from(stats.purgeable_count),
        u64::from(stats.inactive_count),
        page_size_bytes()?,
    );
    // El nivel oficial de presión del OS manda: si dice warning/critical,
    // la medición de páginas no puede contradecirlo (la caché inactiva
    // puede dejar de ser reclamable en un pico). Ver `apply_pressure_ceiling`.
    Some(apply_pressure_ceiling(bytes, memory_pressure_level()))
}

/// RAM libre reclamable en macOS, en la misma moneda que el OS: páginas
/// libres + especulativas + purgeables + **inactivas** (la caché de archivos
/// que el sistema recicla bajo presión). Sumar la caché inactiva es lo que
/// evita el «crítico en falso»: un Mac normal vive de caché de archivos y
/// `vm.page_free_count` a secas es crónicamente bajo. Pura, testeable con
/// tabla.
#[cfg(target_os = "macos")]
fn reclaimable_free_bytes(
    free_pages: u64,
    speculative_pages: u64,
    purgeable_pages: u64,
    inactive_pages: u64,
    page_size: u64,
) -> u64 {
    free_pages
        .saturating_add(speculative_pages)
        .saturating_add(purgeable_pages)
        .saturating_add(inactive_pages)
        .saturating_mul(page_size)
}

/// Estadísticas de VM en una sola syscall (`host_statistics64`, la misma
/// fuente que `vm_stat`). Incluye `inactive_count`, que los oids sysctl
/// `vm.page_*` no exponen (verificado en macOS 15). `None` si falla.
///
/// `mach_host_self` está deprecado en `libc` (sugiere la crate `mach2`);
/// se permite el deprecado a propósito: es la única vía en `libc` sin
/// añadir una dependencia nueva, que el proyecto evita.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn vm_statistics() -> Option<libc::vm_statistics64> {
    let mut stats = std::mem::MaybeUninit::<libc::vm_statistics64>::uninit();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: buffer válido (MaybeUninit del tipo exacto) con el contador
    // correcto; la función rellena `stats` y devuelve `KERN_SUCCESS` (0).
    let rc = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast(),
            &mut count,
        )
    };
    (rc == 0).then(|| unsafe { stats.assume_init() })
}

/// Nivel oficial de presión de macOS: 1 normal, 2 warning, 4 critical.
/// `kern.memorystatus_vm_pressure_level` es la decisión del propio kernel
/// (la misma que pinta el indicador del sistema); sin él (syscall fallida o
/// valor inesperado) se asume normal — sin señal no se detiene nada.
#[cfg(target_os = "macos")]
fn memory_pressure_level() -> u32 {
    let mut level = 0u32;
    let mut len = std::mem::size_of::<u32>();
    // SAFETY: consulta de solo lectura con buffer válido, igual que
    // `detect_ram_bytes`.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            (&mut level as *mut u32).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || !matches!(level, 1 | 2 | 4) {
        return 1;
    }
    level
}

/// Tope duro de la medición según el nivel oficial del OS, para que contar
/// la caché inactiva como libre no enmascare presión real: `warning` (2)
/// reporta a lo sumo por debajo del umbral de reducción, `critical` (4) por
/// debajo del de RAM crítica. `normal` (1) y niveles desconocidos dejan la
/// medición intacta. Pura, testeable con tabla.
#[cfg(target_os = "macos")]
fn apply_pressure_ceiling(bytes: u64, pressure_level: u32) -> u64 {
    match pressure_level {
        2 => bytes.min(FREE_RAM_REDUCTION_THRESHOLD_BYTES - 1),
        4 => bytes.min(CRITICAL_FREE_RAM_BYTES - 1),
        _ => bytes,
    }
}

/// Tamaño de página del sistema (`hw.pagesize`, entero de 4 bytes; buffer
/// `u32` para que la escritura de la syscall quepa exacta).
#[cfg(target_os = "macos")]
fn page_size_bytes() -> Option<u64> {
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
    (rc == 0).then_some(u64::from(page_size))
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
    /// Tabla de la suma reclamable en macOS (Task 1 del plan): las páginas
    /// inactivas (caché de archivos) cuentan como libres, igual que en el
    /// cálculo de presión del kernel. Regresión del reporte del usuario:
    /// un sistema normal con ~5 GiB de caché inactiva y poca free real.
    #[cfg(target_os = "macos")]
    #[test]
    fn reclaimable_free_bytes_table() {
        let page = 4096u64;
        let mib = 1024 * 1024u64;
        let gib = 1024 * mib;
        let cases = [
            // (free, spec, purgeable, inactive, esperado en bytes)
            (0, 0, 0, 0, 0),
            (1, 0, 0, 0, page),
            (gib / page, gib / page, gib / page, gib / page, 4 * gib),
            // El caso del usuario: free baja de verdad, pero la caché
            // inactiva (~5 GiB) es reclamable → no debe marcar crítico.
            (300 * mib / page, 0, 0, 5 * gib / page, 300 * mib + 5 * gib),
        ];
        for (f, s, p, i, expected) in cases {
            assert_eq!(
                reclaimable_free_bytes(f, s, p, i, page),
                expected,
                "free={f} spec={s} purge={p} inactive={i}"
            );
        }
    }

    /// Tabla del tope duro por nivel oficial de presión: `warning` (2) y
    /// `critical` (4) recortan la medición aunque la caché inactiva sea
    /// grande; `normal` (1) y niveles desconocidos la dejan intacta.
    #[cfg(target_os = "macos")]
    #[test]
    fn pressure_ceiling_table() {
        let big = 8 * 1024 * 1024 * 1024u64;
        let cases = [
            // (bytes medidos, nivel oficial, esperado)
            (big, 1, big),
            (big, 0, big),
            (big, 2, FREE_RAM_REDUCTION_THRESHOLD_BYTES - 1),
            (big, 4, CRITICAL_FREE_RAM_BYTES - 1),
            // Ya por debajo del techo: la medición no se infla.
            (10 * 1024 * 1024, 4, 10 * 1024 * 1024),
        ];
        for (bytes, level, expected) in cases {
            assert_eq!(
                apply_pressure_ceiling(bytes, level),
                expected,
                "bytes={bytes} level={level}"
            );
        }
    }

    /// Regresión end-to-end del reporte del usuario: con el sistema en
    /// presión normal (nivel 1) y ~5 GiB de caché inactiva, el valor final
    /// que consume el resto de la app supera el umbral de reducción y no
    /// dispara el guard de RAM crítica.
    #[cfg(target_os = "macos")]
    #[test]
    fn user_case_is_not_critical() {
        let page = 4096u64;
        let bytes = reclaimable_free_bytes(
            300 * 1024 * 1024 / page,
            0,
            0,
            5 * 1024 * 1024 * 1024 / page,
            page,
        );
        let reported = apply_pressure_ceiling(bytes, 1);
        assert!(reported >= FREE_RAM_REDUCTION_THRESHOLD_BYTES);
        assert!(!is_critical_free_ram(Some(reported)));
    }

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
