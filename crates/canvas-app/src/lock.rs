//! Bloqueo de `Mutex` con recuperación de envenenamiento.
//!
//! `std::sync::Mutex` envenena el lock si un hilo entra en pánico
//! sosteniéndolo. `lock().unwrap()` convertiría ese fallo puntual de UNA
//! operación en un pánico en cascada que tumba TODAS las ventanas: el
//! siguiente frame intenta lockear el mismo `Mutex`, entra en pánico, y así
//! en cada acceso. El estado protegido sigue siendo utilizable para el resto
//! del proceso (el pánico ocurrió a mitad de UNA operación y `AppInner`/
//! `Workspace` se reconstituyen frame a frame), así que se recupera el guard
//! del envenenamiento y se sigue — el mismo patrón que ya usa `crash_log.rs`
//! para su buffer circular de logs.

use std::sync::{Mutex, MutexGuard};

/// `lock_ok()` en vez de `lock().unwrap()`: devuelve el guard aunque el
/// `Mutex` esté envenenado por un pánico previo en otro hilo.
pub(crate) trait LockExt {
    type Target;

    fn lock_ok(&self) -> MutexGuard<'_, Self::Target>;
}

impl<T> LockExt for Mutex<T> {
    type Target = T;

    #[inline]
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn recovers_from_poisoned_mutex() {
        let m = Arc::new(Mutex::new(7u32));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _guard = m2.lock().unwrap();
            panic!("envenena el mutex a propósito");
        })
        .join();
        // Envenenado: lock().unwrap() entraría en pánico; lock_ok() recupera
        // el dato (coherente: el pánico fue de OTRA operación).
        assert_eq!(*m.lock_ok(), 7);
    }
}
