//! Detección de la RAM física total por plataforma, sin dependencias
//! nuevas: `windows` (ya en el árbol) en Windows, `libc::sysctlbyname` en
//! macOS (libc ya está en el árbol vía winit/wgpu/objc2), y `/proc/meminfo`
//! en Linux. Cualquier fallo devuelve `None` y el llamador cae al
//! presupuesto histórico. La detección se hace una sola vez por proceso y
//! se cachea: medir en cada frame sería desperdiciar una syscall (o un
//! spawn de proceso) para un valor que no cambia.

use std::sync::OnceLock;

/// RAM física total de la máquina, en bytes, o `None` si no se pudo
/// determinar (plataforma desconocida, syscall fallida, `/proc` ausente).
pub(super) fn total_physical_ram_bytes() -> Option<u64> {
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(detect_ram_bytes)
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
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb.saturating_mul(1024))
}
