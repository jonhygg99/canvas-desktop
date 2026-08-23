//! Colocacion de las ranuras en el espacio de baraja y calculo de cuales
//! caen dentro de la vista (con margen de precarga).

use super::geometry::{DeckAxis, DeckRect};
use super::model::Slot;
use super::Deck;

impl Deck {
    /// Recoloca todas las ranuras en una fila o columna (según `self.axis`),
    /// centradas en el eje transversal, con un hueco proporcional al ancho de
    /// la pila (en espacio de baraja, no en pantalla: constante entre fotos
    /// grandes, no desproporcionado entre miniaturas pequeñas). Con una sola
    /// ranura da `rect = (0,0,w,h)` en cualquiera de los dos ejes. También
    /// calcula `add_zone`: el rect donde iría el PRÓXIMO lienzo, justo
    /// después del último con el mismo hueco (o en el origen, con un tamaño
    /// por defecto, si la baraja está vacía).
    pub fn relayout(&mut self) {
        let sizes: Vec<(f64, f64)> = self.slots.iter().map(Slot::size).collect();
        // Tamaño por defecto de una baraja vacía: el de la última ranura, o
        // (si no hay ninguna) el mismo por defecto que usa una página nueva.
        let add_size = sizes.last().copied().unwrap_or((1920.0, 1080.0));
        match self.axis {
            DeckAxis::Vertical => {
                let deck_w = sizes
                    .iter()
                    .fold(0.0_f64, |m, &(w, _)| m.max(w))
                    .max(add_size.0);
                let gap = (deck_w * 0.03).clamp(24.0, 400.0);
                let mut y = 0.0;
                for (slot, (w, h)) in self.slots.iter_mut().zip(sizes) {
                    slot.rect = DeckRect {
                        x: (deck_w - w) / 2.0,
                        y,
                        w,
                        h,
                    };
                    y += h + gap;
                }
                self.add_zone = DeckRect {
                    x: (deck_w - add_size.0) / 2.0,
                    y,
                    w: add_size.0,
                    h: add_size.1,
                };
            }
            DeckAxis::Horizontal => {
                let deck_h = sizes
                    .iter()
                    .fold(0.0_f64, |m, &(_, h)| m.max(h))
                    .max(add_size.1);
                let gap = (deck_h * 0.03).clamp(24.0, 400.0);
                let mut x = 0.0;
                for (slot, (w, h)) in self.slots.iter_mut().zip(sizes) {
                    slot.rect = DeckRect {
                        x,
                        y: (deck_h - h) / 2.0,
                        w,
                        h,
                    };
                    x += w + gap;
                }
                self.add_zone = DeckRect {
                    x,
                    y: (deck_h - add_size.1) / 2.0,
                    w: add_size.0,
                    h: add_size.1,
                };
            }
        }
        self.layout_dirty = false;
    }

    /// Rect envolvente de toda la pila, para «Fit all» (`Ctrl+Alt+0`).
    /// Min/max genéricos sobre ambos ejes en vez de asumir apilado vertical
    /// desde `(0,0)`: funciona igual con `DeckAxis::Horizontal`, donde es el
    /// eje X el que recorre toda la pila y el Y el que queda acotado.
    pub fn bounds(&self) -> DeckRect {
        if self.slots.is_empty() {
            return DeckRect::ZERO;
        }
        // Incluye `add_zone` (si hay carpeta: sin ella no se pinta ni se
        // puede pulsar) — si no, "ver toda la baraja" (`Ctrl+Alt+0`) deja la
        // zona "+" fuera o al borde del encuadre.
        let rects = self
            .slots
            .iter()
            .map(|s| s.rect)
            .chain(self.folder.is_some().then_some(self.add_zone));
        let mut x0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y0 = f64::INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for r in rects {
            x0 = x0.min(r.x);
            x1 = x1.max(r.x + r.w);
            y0 = y0.min(r.y);
            y1 = y1.max(r.y + r.h);
        }
        DeckRect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1.0),
            h: (y1 - y0).max(1.0),
        }
    }

    /// Índices cuyo rect corta `view` (ya dilatado por el llamante — usar
    /// `dilate`).
    pub fn visible_indices(&self, view: DeckRect) -> Vec<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.rect.intersects(view))
            .map(|(i, _)| i)
            .collect()
    }

    /// Dilata un rect de viewport `VISIBLE_MARGIN` a cada lado — el margen
    /// de precarga: una ranura entra en juego un poco antes de asomar de
    /// verdad, para que no haya un parpadeo al llegar a ella.
    pub fn dilate(view: DeckRect) -> DeckRect {
        let mx = view.w * VISIBLE_MARGIN;
        let my = view.h * VISIBLE_MARGIN;
        DeckRect {
            x: view.x - mx,
            y: view.y - my,
            w: view.w + 2.0 * mx,
            h: view.h + 2.0 * my,
        }
    }

    /// Sella `last_seen` de las ranuras visibles este frame (política LRU
    /// de descarte).
    pub fn mark_visible(&mut self, visible: &[usize]) {
        let frame = self
            .slots
            .iter()
            .map(|s| s.last_seen)
            .max()
            .unwrap_or(0)
            .wrapping_add(1);
        for &i in visible {
            if let Some(s) = self.slots.get_mut(i) {
                s.last_seen = frame;
            }
        }
    }
}

/// El rect visible se dilata esta fracción a cada lado antes de decidir qué
/// cargar/mostrar: evita que una ranura entre y salga de "visible" con un
/// scroll de un solo píxel.
pub(super) const VISIBLE_MARGIN: f64 = 0.5;
