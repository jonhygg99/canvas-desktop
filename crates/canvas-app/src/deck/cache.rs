//! Politica de cache: cuanta memoria ocupan las ranuras cargadas y cuales se
//! descartan cuando se pasa del presupuesto. Nunca se descarta una ranura
//! sucia, ni una cercana a la vista, ni una provisional.

use canvas_render::FxScope;

use super::loading::PRELOAD_RADIUS;
use super::model::SlotContent;
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
            freed.push(FxScope(self.slots[idx].id));
        }
        freed
    }
}

/// Techo duro de ranuras cargadas a la vez, además del presupuesto de bytes.
pub(super) const MAX_LOADED_SLOTS: usize = 12;

/// Presupuesto de píxeles decodificados en RAM, sin contar la activa. Una
/// foto de 20 MP son ~80 MB en RGBA: esto son unas 6 fotos así.
pub(super) const EVICT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

pub(super) const MIN_EVICT_BUDGET_BYTES: usize = 256 * 1024 * 1024;

pub(super) const MAX_EVICT_BUDGET_BYTES: usize = 1024 * 1024 * 1024;

/// Presupuesto adaptativo conservador. Se puede reducir en máquinas con
/// menos memoria mediante `CANVAS_PRELOAD_BUDGET_MB`; el clamp evita valores
/// que inutilicen la caché o comprometan la aplicación.
pub(super) fn adaptive_evict_budget() -> usize {
    let configured = std::env::var("CANVAS_PRELOAD_BUDGET_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|mb| mb.saturating_mul(1024 * 1024));
    configured
        .unwrap_or(EVICT_BUDGET_BYTES)
        .clamp(MIN_EVICT_BUDGET_BYTES, MAX_EVICT_BUDGET_BYTES)
}
