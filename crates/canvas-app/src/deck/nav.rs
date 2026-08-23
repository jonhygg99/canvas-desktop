//! Navegacion entre lienzos de la baraja: la ruta del siguiente/anterior (con
//! vuelta al principio), y el intercambio sin perdida entre la ranura activa y
//! el `EditorState` cuando se salta.

use std::path::PathBuf;

use super::model::SlotContent;
use super::Deck;

impl Deck {
    fn path_at(&self, idx: usize) -> Option<PathBuf> {
        self.slots.get(idx).map(|s| s.path.clone())
    }

    /// Ruta del lienzo siguiente, envolviendo. `None` con ≤1 lienzo.
    pub fn next_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at((self.active + 1) % self.slots.len())
    }

    /// Ruta del lienzo anterior, envolviendo. `None` con ≤1 lienzo.
    pub fn prev_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at((self.active + self.slots.len() - 1) % self.slots.len())
    }

    pub fn first_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at(0)
    }

    pub fn last_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at(self.slots.len() - 1)
    }
}

/// Cambia el lienzo activo: devuelve el actual a su ranura y saca el nuevo.
/// Función libre (no un método de `Deck`) a propósito: ninguno de los dos es
/// dueño del otro, y así queda claro en la firma. Solo se aplica si el
/// destino ya está `Ready` y el editor está ocioso (`EditorState::is_idle`,
/// sin gestos ni ediciones de panel a medias) — si no, la petición se deja
/// pendiente y se reintenta el siguiente frame; un arrastre termina al
/// soltar, así que la espera real es de uno o dos frames, invisible.
/// Devuelve `true` si el salto se aplicó.
pub fn apply_jump(deck: &mut Deck, state: &mut crate::editor::EditorState) -> bool {
    let Some(target) = deck.jump_to else {
        return false;
    };
    if target >= deck.slots.len() || target == deck.active {
        deck.jump_to = None;
        deck.jump_center = false;
        return false;
    }
    if !state.is_idle() {
        return false;
    }
    if let SlotContent::Failed(_) = &deck.slots[target].content {
        // No se reintenta sola: si se dejara `jump_to` puesto, `request_loads`
        // la seguiría priorizando cada frame sin ningún efecto — mejor
        // avisar una vez y soltar la petición.
        tracing::warn!(
            "baraja: se pidió saltar a «{}», que falló al cargar; se descarta el salto",
            deck.slots[target].name
        );
        deck.jump_to = None;
        deck.jump_center = false;
        return false;
    }
    if !matches!(deck.slots[target].content, SlotContent::Ready(_)) {
        return false;
    }
    let SlotContent::Ready(incoming) =
        std::mem::replace(&mut deck.slots[target].content, SlotContent::Active)
    else {
        unreachable!("comprobado justo arriba");
    };
    let outgoing = state.take_slot();
    deck.slots[deck.active].content = SlotContent::Ready(Box::new(outgoing));
    state.put_slot(*incoming);
    deck.active = target;
    deck.jump_to = None;
    true
}
