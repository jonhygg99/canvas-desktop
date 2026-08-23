//! Sincronizacion con el disco y con lo que el usuario reordena: fusionar un
//! reescaneo de la carpeta sin perder ids ni miniaturas, ordenar, mover una
//! ranura de sitio, y las ranuras provisionales (lienzos nuevos aun sin
//! archivo).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui;

use crate::gallery::ItemKind;
use crate::settings::GallerySort;

use super::geometry::MoveDir;
use super::model::{file_name, idle_slot, Slot, SlotContent};
use super::Deck;

impl Deck {
    /// Añade AL FINAL una ranura provisional: un lienzo en blanco del tamaño
    /// de página heredado, sin archivo detrás todavía. `ext` es la extensión
    /// elegida en Ajustes (`settings.new_canvas_format`) — `"canvas"` sigue
    /// siendo un diseño autónomo, cualquier otra cosa es un raster real con
    /// su sidecar. Devuelve su índice para que el llamador salte a ella.
    /// `None` si la baraja no tiene carpeta (`Deck::single`: un archivo
    /// suelto no tiene dónde escribir un hermano).
    /// Cada llamada crea una ranura provisional independiente. Sus nombres
    /// visibles se derivan del id de la ranura para no colisionar antes de
    /// que ninguna de ellas exista en disco.
    pub fn push_placeholder(&mut self, page: (f64, f64), ext: &str) -> Option<usize> {
        let folder = self.folder.clone()?;
        let is_design = ext == canvas_io::CANVAS_EXTENSION;
        let path = canvas_io::peek_numbered_path(&folder, ext, self.next_id);
        let mut state = if is_design {
            crate::editor::EditorState::new_blank(page.0, page.1)
        } else {
            crate::editor::EditorState::new_blank_image(page.0, page.1)
        };
        let mut doc = state.take_slot();
        // Un `History::default()` arranca con `saved_depth: None`, o sea
        // `is_dirty() == true`: un lienzo recién creado se leería como «el
        // usuario ya lo ha editado» y se materializaría solo, sin que nadie
        // dibujara nada. Sellarlo aquí es lo que hace que la transición a
        // sucio signifique de verdad «lo ha tocado».
        doc.history.mark_saved();
        let id = self.next_id;
        self.next_id += 1;
        let order_hint = self.next_order;
        self.next_order += 1;
        self.slots.push(Slot {
            id,
            name: file_name(&path),
            path,
            kind: if is_design {
                ItemKind::Design
            } else {
                ItemKind::Image
            },
            mtime: None,
            thumb: None,
            thumb_failed: false,
            // Ya se conoce su tamaño (hereda el de la página anterior), así
            // que `spawn_deck_probe` (que solo mira `page.is_none()`) no
            // intenta sondear un archivo que no existe.
            page: Some(page),
            // Nace YA colocada donde `add_zone` predijo que iría el próximo
            // lienzo — NO en `DeckRect::ZERO`. `layout_dirty=true` (abajo)
            // dispara un `relayout()` de verdad en el próximo `canvas_ui`,
            // pero quien llama a `push_placeholder` normalmente encadena
            // `jump_to`/`jump_center` y centra la cámara EN ESTE MISMO
            // frame (`deck::apply_jump` + `request_center` en `main.rs`,
            // después de `canvas_ui`) — con `DeckRect::ZERO` esa lectura
            // encontraba un rect en el origen de la baraja (el del PRIMER
            // lienzo apilado) en vez del de esta ranura, y la cámara
            // centraba ahí: "añadir salta al primer lienzo". El tamaño
            // puede no coincidir exacto si el llamador pidió uno distinto
            // al de `add_zone` (el botón de la tira usa
            // `settings.last_page_size`) — se autocorrige con el próximo
            // `relayout()`, y para entonces el centrado ya apuntó al sitio
            // correcto.
            rect: self.add_zone,
            // NUNCA `Idle`: `request_loads` mandaría a cargar de disco un
            // archivo que todavía no está ahí y acabaría en `Failed`.
            content: SlotContent::Ready(Box::new(doc)),
            last_seen: 0,
            is_placeholder: true,
            order_hint,
            locked: false,
        });
        self.layout_dirty = true;
        Some(self.slots.len() - 1)
    }

    /// Descarta una provisional que no tiene archivo en disco. Si es la
    /// activa, instala la ranura lista más cercana como nueva activa.
    pub fn discard_placeholder(&mut self, id: u64, state: &mut crate::editor::EditorState) -> bool {
        let Some(index) = self.find_by_id(id) else {
            return false;
        };
        if !self.slots[index].is_placeholder {
            return false;
        }
        if index != self.active {
            self.slots.remove(index);
            if index < self.active {
                self.active -= 1;
            }
            self.layout_dirty = true;
            return true;
        }

        let Some(target) = self
            .slots
            .iter()
            .enumerate()
            .filter(|(i, slot)| *i != index && matches!(slot.content, SlotContent::Ready(_)))
            .min_by_key(|(i, _)| i.abs_diff(index))
            .map(|(i, _)| i)
        else {
            return false;
        };
        self.slots.remove(index);
        let target = if target > index { target - 1 } else { target };
        let SlotContent::Ready(incoming) =
            std::mem::replace(&mut self.slots[target].content, SlotContent::Active)
        else {
            unreachable!("ready target checked before removing placeholder");
        };
        state.put_slot(*incoming);
        self.active = target;
        self.jump_to = None;
        self.jump_center = true;
        self.layout_dirty = true;
        true
    }

    /// Intercambia la posición VISUAL de la ranura `id` con su vecina
    /// adyacente (en el orden actual, sea cual sea). Un uso de las flechas
    /// ◀/▶ es una declaración explícita de "quiero ESTE orden": cambia
    /// `self.sort` a `Manual` para que el próximo reescaneo no lo deshaga.
    /// `false` si `id` no existe o ya está en el extremo correspondiente.
    pub fn move_slot(&mut self, id: u64, dir: MoveDir) -> bool {
        let Some(idx) = self.find_by_id(id) else {
            return false;
        };
        let neighbor_idx = match dir {
            MoveDir::Prev => idx.checked_sub(1),
            MoveDir::Next if idx + 1 < self.slots.len() => Some(idx + 1),
            MoveDir::Next => None,
        };
        let Some(neighbor_idx) = neighbor_idx else {
            return false;
        };
        // Al entrar en modo manual por primera vez, normaliza TODOS los
        // hints a la posición visual actual (la que dejó Name/DateModified)
        // — si no, heredarían valores de creación sin relación con lo que
        // se ve, y el resto de la baraja "saltaría" de sitio en el próximo
        // `apply_sort`, no solo la pareja que se acaba de mover.
        if self.sort != GallerySort::Manual {
            for (i, slot) in self.slots.iter_mut().enumerate() {
                slot.order_hint = i as u64;
            }
            self.next_order = self.slots.len() as u64;
        }
        self.sort = GallerySort::Manual;
        let hint_a = self.slots[idx].order_hint;
        let hint_b = self.slots[neighbor_idx].order_hint;
        self.slots[idx].order_hint = hint_b;
        self.slots[neighbor_idx].order_hint = hint_a;
        // `apply_sort` reordena `self.slots` por posición, no por id —
        // `self.active` es un ÍNDICE, así que hay que reencontrar la activa
        // por su id estable después de reordenar (mismo motivo por el que
        // `merge_scan` restaura `self.active` por ruta tras su propio
        // `apply_sort`).
        let active_id = self.slots.get(self.active).map(|s| s.id);
        self.apply_sort();
        if let Some(active_id) = active_id {
            if let Some(i) = self.find_by_id(active_id) {
                self.active = i;
            }
        }
        true
    }

    fn apply_sort(&mut self) {
        match self.sort {
            // Natural/numérico, no byte a byte: ver la doc de `natural_cmp`
            // (settings.rs) — sin esto, "6.png".."9.png" acaban después de
            // "51.png" en una carpeta sin ceros de relleno.
            GallerySort::Name => self
                .slots
                .sort_by(|a, b| crate::settings::natural_cmp(&a.name, &b.name)),
            // Más recientes primero; sin fecha, al final. Mismo criterio que
            // `GalleryState::apply_sort`.
            GallerySort::DateModified => self.slots.sort_by(|a, b| {
                b.mtime
                    .cmp(&a.mtime)
                    .then_with(|| crate::settings::natural_cmp(&a.name, &b.name))
            }),
            // El orden que dejaron las flechas de mover (`Deck::move_slot`).
            GallerySort::Manual => self.slots.sort_by_key(|s| s.order_hint),
        }
        self.layout_dirty = true;
    }

    /// Sustituye la lista de archivos conservando por ruta todo lo que ya
    /// tenía la ranura (id, miniatura, tamaño sondeado y, sobre todo, el
    /// CONTENIDO cargado) — mismo espíritu que `GalleryState::merge_files`.
    /// Una ranura cuyo archivo desapareció del disco se descarta, SALVO que
    /// sea la activa o tenga cambios sin guardar: perder ese trabajo en
    /// silencio por un simple reescaneo sería peor que dejar un lienzo
    /// «huérfano» a la vista.
    pub fn merge_scan(&mut self, files: Vec<(PathBuf, Option<SystemTime>)>) {
        let active_path = self.active_path();
        // Las ranuras PROVISIONALES no están en disco, así que el listado
        // nunca las trae y la regla de conservación de abajo (activa o
        // sucia) las tiraría: una provisional recién creada no es ninguna de
        // las dos. Se apartan ANTES de reconciliar (también evita que
        // colisionen en el `HashMap` de abajo si su nombre asomado coincide
        // con un archivo real) y se vuelven a pegar al final — que es su
        // sitio — DESPUÉS de ordenar pero ANTES de restaurar la activa: si
        // no, una provisional activa perdería su índice en cuanto el
        // reordenado la desplazase.
        let (placeholders, rest): (Vec<Slot>, Vec<Slot>) =
            self.slots.drain(..).partition(|s| s.is_placeholder);
        let mut old: HashMap<PathBuf, Slot> =
            rest.into_iter().map(|s| (s.path.clone(), s)).collect();
        let mut next_id = self.next_id;
        let mut slots = Vec::with_capacity(files.len());
        for (path, mtime) in files {
            match old.remove(&path) {
                Some(mut existing) => {
                    existing.mtime = mtime;
                    slots.push(existing);
                }
                None => {
                    let id = next_id;
                    next_id += 1;
                    let order_hint = self.next_order;
                    self.next_order += 1;
                    slots.push(idle_slot(id, path, mtime, order_hint));
                }
            }
        }
        for (_, slot) in old {
            let keep = matches!(&slot.content, SlotContent::Active)
                || matches!(&slot.content, SlotContent::Ready(doc) if doc.history.is_dirty());
            if keep {
                slots.push(slot);
            }
        }
        self.slots = slots;
        self.next_id = next_id;
        self.apply_sort();
        self.slots.extend(placeholders);
        if let Some(p) = active_path {
            if let Some(idx) = self.find_by_path(&p) {
                self.active = idx;
            }
        }
    }

    /// Entrega una miniatura llegada de un hilo de trabajo (por ruta: el
    /// orden puede haber cambiado desde que se lanzó el escaneo).
    pub fn set_thumb(&mut self, path: &Path, tex: Option<egui::TextureHandle>) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.path == path) {
            match tex {
                Some(tex) => slot.thumb = Some(tex),
                None => slot.thumb_failed = true,
            }
        }
    }

    /// Rellena el tamaño de página sondeado de toda la carpeta, llegado en
    /// un solo mensaje (`DeckProbed`) para que haga falta un único
    /// `relayout`, no uno por archivo.
    pub fn set_probes(&mut self, sizes: Vec<(PathBuf, Option<(f64, f64)>)>) {
        let mut by_path: HashMap<PathBuf, (f64, f64)> = sizes
            .into_iter()
            .filter_map(|(p, s)| s.map(|s| (p, s)))
            .collect();
        for slot in &mut self.slots {
            if let Some(size) = by_path.remove(&slot.path) {
                slot.page = Some(size);
            }
        }
        self.layout_dirty = true;
    }
}
