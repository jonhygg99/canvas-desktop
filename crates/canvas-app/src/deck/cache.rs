//! Politica de cache: cuanta memoria ocupan las ranuras cargadas y cuales se
//! descartan cuando se pasa del presupuesto. Nunca se descarta una ranura
//! sucia, ni una cercana a la vista, ni una provisional.

use canvas_render::FxScope;

use super::loading::PRELOAD_RADIUS;
use super::model::SlotContent;
use super::system::total_physical_ram_bytes;
use super::Deck;

impl Deck {
    pub(super) fn loaded_bytes(&self) -> usize {
        self.slots
            .iter()
            .filter_map(|s| match &s.content {
                SlotContent::Ready(d) => Some(d.bytes),
                _ => None,
            })
            .sum()
    }

    /// Descarta ranuras `Ready` lejanas, limpias, sin historial de deshacer
    /// pendiente y sin guardado en curso, hasta volver al presupuesto (la
    /// más lejos vista primero). Nunca descarta la activa, una sucia, ni una
    /// que aún pueda deshacer algo (recargarla de disco perdería ese
    /// historial para siempre). Devuelve los `FxScope` a liberar en el
    /// renderer — el llamador, que tiene el `CanvasRenderer`, hace el
    /// `forget_scope` (aquí no se acopla `Deck` a `canvas-render` más que
    /// por el tipo del scope).
    pub fn evict(&mut self) -> Vec<FxScope> {
        self.evict_with_budget(adaptive_evict_budget())
    }

    pub fn evict_with_budget(&mut self, budget: usize) -> Vec<FxScope> {
        let active = self.active;
        let mut freed = Vec::new();
        loop {
            let loaded_count = self
                .slots
                .iter()
                .filter(|s| matches!(s.content, SlotContent::Ready(_)))
                .count();
            if self.loaded_bytes() <= budget && loaded_count <= MAX_LOADED_SLOTS {
                break;
            }
            let candidate = self
                .slots
                .iter()
                .enumerate()
                .filter(|(i, s)| {
                    *i != active
                        && Some(*i) != self.jump_to
                        && i.abs_diff(active) > PRELOAD_RADIUS
                        // Una provisional está LIMPIA por construcción
                        // (`mark_saved` al nacer en `push_placeholder`), así
                        // que pasaría el resto de este filtro y volvería a
                        // `Idle` — y desde `Idle`, `request_loads`
                        // intentaría cargar de disco un archivo que aún no
                        // existe, dejándola en `Failed`. No se puede confiar
                        // en que otro estado la proteja por accidente.
                        && !s.is_placeholder
                        // Limpia (`is_dirty() == false`) no basta: una
                        // ranura guardada puede seguir teniendo pasos de
                        // deshacer en la pila, y expulsarla a `Idle` los
                        // perdería para siempre (al volver, se recarga de
                        // disco con un `History` en blanco). `can_undo()`
                        // los protege también.
                        && matches!(&s.content, SlotContent::Ready(d) if !d.history.is_dirty() && !d.history.can_undo() && !d.saving)
                })
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(i, _)| i);
            let Some(idx) = candidate else {
                tracing::warn!(
                    "baraja: presupuesto de memoria excedido ({} ranuras cargadas) y ninguna \
                     se puede descartar (todas sucias, guardando, o cerca de la activa)",
                    loaded_count
                );
                break;
            };
            self.slots[idx].content = SlotContent::Idle;
            freed.push(FxScope(self.slots[idx].scope));
        }
        freed
    }
}

/// Techo duro de ranuras cargadas a la vez, además del presupuesto de bytes.
pub(super) const MAX_LOADED_SLOTS: usize = 12;

/// Presupuesto de píxeles decodificados en RAM, sin contar la activa, cuando
/// no se puede conocer la RAM de la máquina. Una foto de 20 MP son ~80 MB en
/// RGBA: esto son unas 6 fotos así.
pub(super) const EVICT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

/// El presupuesto nunca baja de 256 MB (una foto 20 MP + vecinas) ni sube de
/// 1 GB: con más RAM no compensa retener más píxeles precargados de los que
/// el usuario va a ver.
pub(super) const MIN_EVICT_BUDGET_BYTES: usize = 256 * 1024 * 1024;

pub(super) const MAX_EVICT_BUDGET_BYTES: usize = 1024 * 1024 * 1024;

/// Fracción de la RAM física total que la caché de la baraja puede ocupar:
/// 1/16. Con 8 GB da los 512 MB históricos, con 16 GB el techo de 1 GB, y
/// con 4 GB baja a 256 MB para no competir con el resto del sistema. La
/// elección es deliberadamente conservadora: la baraja es una caché y la RAM
/// sobrante sirve para lo que el usuario esté haciendo además del editor.
const RAM_BUDGET_FRACTION: f64 = 1.0 / 16.0;

/// Presupuesto que corresponde a una RAM total dada (bytes), ya clampeado al
/// intervalo [MIN, MAX]. Pura: se prueba con valores de tabla sin depender
/// del hardware de la máquina de test.
pub(super) fn evict_budget_from_ram(total_bytes: u64) -> usize {
    let scaled = (total_bytes as f64 * RAM_BUDGET_FRACTION) as usize;
    scaled.clamp(MIN_EVICT_BUDGET_BYTES, MAX_EVICT_BUDGET_BYTES)
}

/// Decisión pura del presupuesto: la env var gana, si no la RAM medida, si
/// no el histórico — siempre clampeado. Separada de `adaptive_evict_budget`
/// para poder probar las tres ramas sin tocar variables de entorno ni
/// hardware.
pub(super) fn resolve_evict_budget(configured: Option<usize>, ram_bytes: Option<u64>) -> usize {
    configured
        .or_else(|| ram_bytes.map(evict_budget_from_ram))
        .unwrap_or(EVICT_BUDGET_BYTES)
        .clamp(MIN_EVICT_BUDGET_BYTES, MAX_EVICT_BUDGET_BYTES)
}

/// Presupuesto adaptativo. `CANVAS_PRELOAD_BUDGET_MB` sigue ganando (afinar
/// por máquina o por prueba); sin ella, el presupuesto escala con la RAM
/// física de la máquina (1/16, ver `RAM_BUDGET_FRACTION`), y si no se puede
/// medir cae al valor histórico. El clamp evita valores que inutilicen la
/// caché o comprometan la aplicación.
pub(super) fn adaptive_evict_budget() -> usize {
    let configured = std::env::var("CANVAS_PRELOAD_BUDGET_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|mb| mb.saturating_mul(1024 * 1024));
    resolve_evict_budget(configured, total_physical_ram_bytes())
}
