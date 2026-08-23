//! Planificador de cargas: que ranuras pedir al hilo de disco, en que orden y
//! cuantas a la vez. La generacion sirve para descartar respuestas de una
//! baraja anterior.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::cache::adaptive_evict_budget;
use super::model::SlotContent;
use super::Deck;

impl Deck {
    /// Pide la carga de las ranuras `Idle` entre las visibles, las del radio
    /// de precarga alrededor de la activa, y el destino de un salto pendiente
    /// (`jump_to`) si lo hay — `apply_jump` exige que ese destino esté
    /// `Ready`, así que sin esto un salto a una ranura fuera del radio de
    /// precarga se quedaría pedido para siempre, porque nada dispararía
    /// jamás su carga. El destino de `jump_to` va primero (es lo que el
    /// usuario está esperando activamente), el resto por cercanía a la
    /// activa como antes. Respeta `MAX_INFLIGHT_LOADS`. Devuelve las rutas
    /// para las que `App` debe lanzar `loader::spawn_load_slot`; las marca
    /// `Loading`.
    pub fn request_loads(&mut self, visible: &[usize]) -> Vec<PathBuf> {
        if self.slots.is_empty() {
            return Vec::new();
        }
        let active = self.active;
        let jump = self.jump_to.filter(|&i| i < self.slots.len());
        let lo = active.saturating_sub(PRELOAD_RADIUS);
        let hi = (active + PRELOAD_RADIUS).min(self.slots.len() - 1);
        let mut candidates: Vec<usize> = visible.iter().copied().chain(lo..=hi).collect();
        if self.preload_all {
            candidates.extend(0..self.slots.len());
        }
        if let Some(j) = jump {
            candidates.push(j);
        }
        candidates.sort_unstable();
        candidates.dedup();
        // `!s.is_placeholder` es defensa en profundidad: con `evict`
        // guardada (ver más abajo) una provisional nunca debería llegar
        // aquí en `Idle`, pero la condición es barata y evita cargar de
        // disco un archivo que aún no existe si algo falla.
        candidates.retain(|&i| {
            matches!(self.slots.get(i), Some(s) if matches!(s.content, SlotContent::Idle) && !s.is_placeholder)
        });
        candidates.sort_by_key(|&i| {
            let jump_rank = usize::from(Some(i) != jump);
            let distance = i.abs_diff(active);
            let visibility_rank = usize::from(!visible.contains(&i));
            (jump_rank, distance, visibility_rank, i)
        });

        let mut spawned = Vec::new();
        let memory_pressure = self.loaded_bytes() > adaptive_evict_budget() / 2;
        let inflight_limit = configured_inflight_limit().unwrap_or(if memory_pressure {
            1
        } else {
            MAX_INFLIGHT_LOADS
        });
        for i in candidates {
            if self.inflight >= inflight_limit {
                break;
            }
            if let Some(slot) = self.slots.get_mut(i) {
                slot.content = SlotContent::Loading;
                self.inflight += 1;
                spawned.push(slot.path.clone());
            }
        }
        spawned
    }

    /// Una carga (con éxito o sin él) terminó: libera un hueco de
    /// `MAX_INFLIGHT_LOADS`.
    pub fn loading_finished(&mut self) {
        self.inflight = self.inflight.saturating_sub(1);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Ranuras vecinas a la activa que se cargan siempre, se vean o no: al
/// saltar con `PageUp`/`PageDown` el destino inmediato ya está listo.
pub(super) const PRELOAD_RADIUS: usize = 2;

/// Cargas simultáneas en vuelo. Más de dos solo hace competir a los hilos
/// por el disco sin acelerar nada.
pub(super) const MAX_INFLIGHT_LOADS: usize = 2;

pub(super) fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub(super) fn configured_inflight_limit() -> Option<usize> {
    std::env::var("CANVAS_PRELOAD_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.clamp(1, 4))
}
