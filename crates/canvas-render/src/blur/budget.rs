//! Presupuesto de memoria GPU del documento activo: la misma política que la
//! caché de la baraja (1/16 de la RAM física, techo [256 MiB, 1 GiB], reducido
//! por RAM libre por debajo de 2 GiB). `canvas-render` no puede depender de
//! `canvas-app`, así que el umbral de reducción y el histórico se declaran
//! aquí también — deben moverse juntos con `deck/system.rs`.
//!
//! El presupuesto acota el mayor componente controlable del atlas de vello:
//! las texturas de efectos de `BlurEngine` (4 por capa con blur). Con el
//! total bajo presupuesto, lo que la app registra cabe en el atlas y el
//! «bake parcial por descarte de imágenes» se cierra en origen — no es una
//! garantía matemática (el propio vello registra también las fuentes
//! originales), el guard anti-incompleto queda como red de seguridad.

/// Umbral de «poca RAM»: igual que `FREE_RAM_REDUCTION_THRESHOLD_BYTES` de
/// la baraja (2 GiB). Duplicado a propósito: ambos presupuestos deben
/// reducirse en el mismo punto, y el renderer no puede importarlo.
pub const FX_FREE_RAM_REDUCTION_THRESHOLD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// El presupuesto nunca baja de 256 MB (unas 4 capas grandes con blur) ni
/// sube de 1 GB (con más RAM no compensa retener más texturas de efectos de
/// las que la vista va a usar) — el mismo intervalo que la baraja.
pub const MIN_FX_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_FX_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// Fracción de la RAM física total que la caché de efectos GPU puede ocupar:
/// 1/16, como la baraja. Con 4 GB da el mínimo de 256 MB, con 8 GB los
/// 512 MB históricos y con 16 GB el techo de 1 GB.
const FX_RAM_BUDGET_FRACTION: f64 = 1.0 / 16.0;

/// Presupuesto cuando no se puede medir la RAM de la máquina: el histórico
/// de 512 MB, el mismo suelo que usaba la baraja antes de escalar.
const FALLBACK_FX_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// Presupuesto que corresponde a una RAM total dada (bytes), ya clampeado al
/// intervalo [MIN, MAX]. Pura: se prueba con tabla sin depender del hardware.
pub fn fx_budget_from_ram(total_bytes: u64) -> u64 {
    let scaled = (total_bytes as f64 * FX_RAM_BUDGET_FRACTION) as u64;
    scaled.clamp(MIN_FX_BUDGET_BYTES, MAX_FX_BUDGET_BYTES)
}

/// Presupuesto bajo presión de memoria: si la RAM libre cae por debajo de
/// `FX_FREE_RAM_REDUCTION_THRESHOLD_BYTES`, se escala linealmente por
/// `libre / umbral`, nunca por debajo del mínimo — con 0 bytes libres aún
/// queda el mínimo para no inutilizar la caché. Pura: se prueba con tabla.
pub fn fx_budget_under_free_ram(base: u64, free_bytes: u64) -> u64 {
    let threshold = FX_FREE_RAM_REDUCTION_THRESHOLD_BYTES;
    if free_bytes >= threshold {
        return base;
    }
    let scaled = (base as f64 * (free_bytes as f64 / threshold as f64)) as u64;
    scaled.clamp(MIN_FX_BUDGET_BYTES, base)
}

/// Decisión final del presupuesto GPU: la RAM total fija el techo (1/16,
/// clampeado; el histórico 512 MiB si no se puede medir) y la RAM libre lo
/// reduce dinámicamente bajo presión. `None` (no medible) no reduce nada.
/// Pura: se prueba sin tocar hardware.
pub fn resolve_fx_budget(total_bytes: Option<u64>, free_bytes: Option<u64>) -> u64 {
    let base = total_bytes
        .map(fx_budget_from_ram)
        .unwrap_or(FALLBACK_FX_BUDGET_BYTES);
    match free_bytes {
        Some(free) => fx_budget_under_free_ram(base, free),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_budget_scales_with_ram_and_clamps() {
        // (RAM total, presupuesto esperado)
        let cases = [
            (4u64 * 1024 * 1024 * 1024, MIN_FX_BUDGET_BYTES), // 4 GB → mínimo
            (8u64 * 1024 * 1024 * 1024, 512 * 1024 * 1024),   // 8 GB → histórico
            (16u64 * 1024 * 1024 * 1024, MAX_FX_BUDGET_BYTES), // 16 GB → techo
            (32u64 * 1024 * 1024 * 1024, MAX_FX_BUDGET_BYTES), // 32 GB → clampeado al techo
            (1024u64 * 1024 * 1024, MIN_FX_BUDGET_BYTES),     // 1 GB → clampeado al mínimo
        ];
        for (ram, expected) in cases {
            assert_eq!(fx_budget_from_ram(ram), expected, "RAM {ram}");
        }
    }

    #[test]
    fn fx_budget_reduces_linearly_under_free_ram_pressure() {
        let base = MAX_FX_BUDGET_BYTES;
        let threshold = FX_FREE_RAM_REDUCTION_THRESHOLD_BYTES;
        // Libre por encima del umbral → sin reducción.
        assert_eq!(fx_budget_under_free_ram(base, threshold + 1), base);
        assert_eq!(fx_budget_under_free_ram(base, threshold), base);
        // A la mitad del umbral → mitad del presupuesto.
        assert_eq!(fx_budget_under_free_ram(base, threshold / 2), base / 2);
        // Sin RAM libre → nunca por debajo del mínimo.
        assert_eq!(fx_budget_under_free_ram(base, 0), MIN_FX_BUDGET_BYTES);
    }

    #[test]
    fn resolve_fx_budget_falls_back_and_combines() {
        // Sin medición posible → histórico.
        assert_eq!(resolve_fx_budget(None, None), FALLBACK_FX_BUDGET_BYTES);
        // RAM medida, sin presión → techo por RAM.
        let sixteen = 16u64 * 1024 * 1024 * 1024;
        assert_eq!(resolve_fx_budget(Some(sixteen), None), MAX_FX_BUDGET_BYTES);
        // RAM medida + RAM libre alta → sin reducción.
        assert_eq!(
            resolve_fx_budget(Some(sixteen), Some(3 * 1024 * 1024 * 1024)),
            MAX_FX_BUDGET_BYTES
        );
        // RAM medida + RAM libre a la mitad del umbral → mitad.
        assert_eq!(
            resolve_fx_budget(Some(sixteen), Some(1024 * 1024 * 1024)),
            MAX_FX_BUDGET_BYTES / 2
        );
    }
}
