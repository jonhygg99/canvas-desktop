# Plan de refactorización — Ronda 2 (post-modularización de `main.rs`/`editor.rs`)

> Estado: **Las 5 fases completas y verificadas** (build real, tests,
> clippy, fmt limpios en cada una, más arranque real de la app tras cada
> fase que tocó UI).

## Progreso

1. ✅ **Fase 1 — `canvas-io/src/sidecar.rs` (1107 líneas) → `sidecar/`.**
   `mod.rs` (583, incluye todos los tests — se dejaron juntos como suite de
   integración del módulo reensamblado, no repartidos por archivo) +
   `paths.rs` (85), `trash.rs` (85), `payload.rs` (131), `io.rs` (281).
   68 tests, clippy y fmt limpios.
2. ✅ **Fase 2 — `canvas-app/src/loader.rs` (1074 líneas) → `loader/`.**
   `mod.rs` (191, tipos `AppMsg`/`LoadOutcome`/`GalleryOp`) +
   `load_ops.rs` (189), `save_ops.rs` (158), `export_ops.rs` (90),
   `gallery_ops.rs` (352), `image_import.rs` (169). 56 tests, clippy y fmt
   limpios.
3. ✅ **Fase 3 — `editor/properties_panel.rs` (1717 líneas) →
   `properties_panel/`.** `mod.rs` (715, incluye `properties_ui`/
   `properties_ui_inner`/`commit_stale_panel_edits`/`file_name_ui` + todos
   los tests, igual criterio que la Fase 1) + `page.rs` (251, incluye
   `size_popup_ui`), `effects.rs` (240), `layer_common.rs` (369),
   `content.rs` (49, dispatcher) + `content_text.rs` (108)/
   `content_shape.rs` (67). `size_popup_ui` quedó `pub(in crate::editor)`
   (no `pub(super)`) porque ahora vive dos niveles bajo `editor`, y
   `canvas_view.rs` (primo, no padre) necesita seguir llamándolo. Build
   real (`cargo run -p canvas-app` abriendo una imagen, 8s sin panics), 56
   tests, clippy y fmt limpios.
4. ✅ **Fase 4 — `app/mod.rs::ui()` (~1170 líneas en una sola función) →
   `ui_menu.rs`/`ui_modals.rs`/`ui_views.rs`.** Alcance completo (opción
   elegida sobre la reducida): la sincronización de menú (antes del
   `match`) y las ventanas de Ajustes/About (después del `match`) son
   métodos normales de `App` en `ui_menu.rs`/`ui_modals.rs`, porque ahí
   `&mut self` está libre. El cuerpo de `View::Editor` — modales de
   guardado (overwrite/readonly/export) y toda la orquestación de
   guardado/navegación de baraja/deshacer global — salió como **funciones
   libres con parámetros explícitos** en `ui_modals.rs`/`ui_views.rs`
   (`#[allow(clippy::too_many_arguments)]`), no como métodos: `state` sigue
   prestado de `self.view` durante toda esa rama, el mismo motivo por el
   que el propio código ya aplazaba `handle_menu_action` — mismo patrón que
   `editor::canvas_ui`/`deck_strip::deck_strip_ui`, que ya recibían campos
   sueltos de `self` por la misma razón. `app/mod.rs`: 1417 → 363 líneas.
   `ui_views.rs` (882 líneas) concentra ahora la orquestación de
   `View::Editor` — es la función más larga que ha dejado este refactor,
   pero partirla más multiplicaba parámetros compartidos (`open_next`,
   `pending_menu_action`, `deck_target`, `strip_action`, `canvas_action`)
   sin ganar legibilidad real. Build real (Welcome/Gallery/Editor, 6-8s
   cada uno sin panics), 56+68+71 tests, clippy y fmt limpios.
5. ✅ **Fase 5 — `gallery.rs` (904 líneas) → `gallery/{mod,ui}.rs`.**
   `mod.rs` (estado: `GalleryState`/`GalleryItem`/`ItemKind`/
   `FolderNavigation`/`GalleryAction`, la ranura de copiar/pegar entre
   carpetas, y los tests) + `ui.rs` (renderizado: `show`/
   `folder_panel_contents`/`gallery_cell`/`begin_rename`, reexportado como
   `gallery::show`/`gallery::next_folder_panel_side` para no tocar ningún
   call site). `gallery_cell_size`/`gallery_column_count` pasaron de
   privadas a `pub(super)` (los tests de `mod.rs` los necesitan, y un hijo
   no ve los privados de sus hermanos — al revés del patrón ya usado en
   las fases anteriores). Build real (galería abierta, 6s sin panics), 56
   tests, clippy y fmt limpios.

## Plan completo — resumen final

`sidecar.rs` (1107) → 5 archivos; `loader.rs` (1074) → 6 archivos;
`properties_panel.rs` (1717) → 7 archivos; `app/mod.rs::ui()` (~1170 líneas
en una función) → repartido en 3 archivos nuevos (`app/mod.rs` quedó en 363
líneas); `gallery.rs` (904) → 2 archivos. Verificado limpio (build real,
tests, clippy, fmt) en cada fase, con arranque real de la app (Welcome/
Gallery/Editor) tras las fases que tocaron UI (3, 4, 5).

## 0. Contexto — por qué esta ronda 2

Ya existió una Ronda 1 de modularización (ver historial de commits:
"Extract handle_messages into app/messages.rs", "Move App/View/Nav and the
eframe::App::ui loop into app/mod.rs", "Mark the canvas-app modularization
plan as complete"). Esa ronda resolvió el problema original:

- `editor.rs` (4825 líneas) → `editor/` (8 archivos + `mod.rs` de 28 líneas).
- `main.rs` (3348 líneas) → `app/` (6 archivos) + `main.rs` de 161 líneas.

**Verificado en este análisis:** esa ronda 1 sigue intacta y limpia. Ya NO
hay un único "God Object" monolítico. Lo que sí hay, tras medir líneas por
archivo en las 5 crates (`crates/*/src/**/*.rs`, 23248 líneas totales), es
un segundo nivel de archivos que crecieron *dentro* de la nueva estructura y
que vuelven a mezclar responsabilidades:

| Archivo | Líneas | Problema real |
|---|---:|---|
| `canvas-app/src/editor/properties_panel.rs` | 1717 | Un solo archivo con la UI de propiedades de **todos** los tipos de capa (imagen, texto, forma, página, efectos) |
| `canvas-app/src/app/mod.rs` | 1417 | Correcto como archivo, pero `impl eframe::App::ui()` es **una sola función de ~1170 líneas** (menú, 3 modales, enrutamiento de vistas, ventana "About") |
| `canvas-core/src/command.rs` | 1383 | ~20 `Command` distintos en un archivo (baja prioridad, ver §5) |
| `canvas-app/src/editor/state.rs` | 1171 | `EditorState`: cohesivo por diseño (Fase 2 de la ronda 1), no se toca |
| `canvas-io/src/sidecar.rs` | 1107 | Mezcla rutas/legacy, papelera local, encode/decode de payload, y lectura/escritura de disco |
| `canvas-app/src/loader.rs` | 1074 | ~30 funciones `spawn_*` de I/O en background, sin separar por dominio (carga/guardado/export/galería/import de URL) |
| `canvas-app/src/gallery.rs` | 904 | Mezcla estado (`GalleryState`, scan, navegación) con renderizado egui |

Este plan ataca esas siete áreas, priorizando impacto y riesgo.

## 1. Diagnóstico por archivo

### `app/mod.rs` — función `ui()` de ~1170 líneas
Una sola función maneja, en orden estricto (hay comentarios explícitos en
el código advirtiendo sobre el orden de evaluación de modales, p. ej. "NUNCA
mientras haya un modal de guardado pendiente"):
1. Barra de menú superior.
2. `CentralPanel` con enrutamiento Welcome/Loading/Gallery/Editor.
3. Modal de guardar-antes-de-salir / overwrite.
4. Modal de export.
5. Ventana "About".

### `editor/properties_panel.rs`
`properties_ui` → `properties_ui_inner` → ramifica en:
`file_name_ui`, `page_ui` + `page_size_presets_ui`, `blur_control`,
`color_adjustments_ui`, `shadow_ui`, `layer_properties_ui` (transform/crop
compartido), `content_properties_ui` (switch sobre
`LayerContent::{Image,Text,Shape,Svg,Group}`, esta última rama sola mide
~550 líneas).

### `canvas-io/sidecar.rs`
Cuatro responsabilidades independientes en un archivo: (a) resolución de
rutas (`sidecar_dir/path`, legacy, `find_sidecar`), (b) papelera local
(`trash_dir`, `move_to_local_trash`, `restore_from_local_trash`,
`purge_local_trash`), (c) construcción/encode del payload (`blank_design`,
`encode_payload`, `make_preview`, `fnv1a64`), (d) lectura/escritura real a
disco (`write_sidecar`, `read_sidecar`, `read_design`, `read_preview`,
`delete_sidecar`).

### `canvas-app/loader.rs`
`AppMsg`/`LoadOutcome`/`GalleryOp` (tipos compartidos) + ~30 funciones
`spawn_*` que se agrupan naturalmente en 4 dominios: carga, guardado,
export, y operaciones de galería (rename/delete/restore/duplicate/scan/
thumbs), más un sub-grupo de import de imagen por URL/reemplazo.

### `canvas-app/gallery.rs`
`GalleryState`/`FolderNavigation` (modelo + lógica de scan/orden) están
entrelazados con `show`/`folder_panel_contents`/`gallery_cell` (renderizado
egui puro).

## 2. Nueva arquitectura propuesta

```
crates/canvas-app/src/
├─ app/
│  ├─ mod.rs            # App/View/Nav, App::new, fn ui() como DISPATCHER corto
│  ├─ ui_menu.rs         # bloque de barra de menú superior (extraído de ui())
│  ├─ ui_modals.rs       # modal guardar/overwrite, modal export, ventana About
│  ├─ ui_views.rs        # CentralPanel: enrutamiento Welcome/Loading/Gallery/Editor
│  ├─ window.rs          # (sin cambios)
│  ├─ persistence.rs     # (sin cambios)
│  ├─ menu_actions.rs    # (sin cambios)
│  ├─ navigation.rs      # (sin cambios)
│  └─ messages.rs        # (sin cambios)
│
├─ editor/
│  ├─ properties_panel/
│  │  ├─ mod.rs          # properties_ui, commit_stale_panel_edits, dispatcher
│  │  ├─ page.rs         # page_ui, page_size_presets_ui
│  │  ├─ effects.rs      # blur_control, color_adjustments_ui, shadow_ui
│  │  ├─ layer_common.rs # layer_properties_ui (transform/crop compartido) + file_name_ui
│  │  ├─ content_image.rs # rama Image de content_properties_ui
│  │  ├─ content_text.rs  # rama Text
│  │  └─ content_shape.rs # rama Shape (+ Svg si aplica)
│  └─ (resto sin cambios: state.rs, viewport.rs, layer_ops.rs, interaction.rs,
│      overlay.rs, slot_chrome.rs, canvas_view.rs)
│
├─ loader/
│  ├─ mod.rs             # AppMsg, LoadOutcome, GalleryOp (tipos públicos)
│  ├─ load_ops.rs        # spawn_load_image/_design/_slot, spawn_deck_probe, probe_page_sizes
│  ├─ save_ops.rs        # spawn_save/_design, spawn_reserve_canvas_path, spawn_pick_save/design_path
│  ├─ export_ops.rs       # spawn_export_raster/_vector, spawn_pick_export_path
│  ├─ gallery_ops.rs      # spawn_gallery_op/_scan, spawn_document_rename/_delete, spawn_restore_from_trash, spawn_single_thumb
│  └─ image_import.rs     # spawn_load_image_as_layer, spawn_pick_replacement_image, spawn_load_replacement_image_from_url, download_url_to_temp, helpers de label/extension
│
└─ gallery/
   ├─ mod.rs              # GalleryState, FolderNavigation, GalleryAction (estado + lógica)
   └─ ui.rs               # show, folder_panel_contents, gallery_cell, begin_rename (egui)

crates/canvas-io/src/
└─ sidecar/
   ├─ mod.rs              # CanvasPayload/RestoredDocument públicos + re-exports
   ├─ paths.rs            # sidecar_dir/path, legacy_sidecar_path, sidecar_file_name, find_sidecar
   ├─ trash.rs            # trash_dir, local_trash_path, move/restore/purge_local_trash
   ├─ payload.rs          # blank_design, encode_payload, preview_scale, make_preview, fnv1a64
   └─ io.rs                # write_sidecar/_design, read_canvas_file/_sidecar/_design/_preview, delete_sidecar, write_blank_canvas
```

**Baja prioridad / opcional (no incluido en el alcance por defecto, ver §5):**
`canvas-core/src/command.rs` → `command/{transform,layer_lifecycle,group,page,history}.rs`.

## 3. Mapeo de migración

| Origen (función/bloque) | Destino |
|---|---|
| `sidecar_dir`, `sidecar_path`, `legacy_sidecar_path`, `sidecar_file_name`, `find_sidecar` | `sidecar/paths.rs` |
| `trash_dir`, `local_trash_path`, `move_to_local_trash`, `restore_from_local_trash`, `purge_local_trash`, `hide_dir` (win/otros) | `sidecar/trash.rs` |
| `fnv1a64`, `preview_scale`, `make_preview`, `blank_design`, `encode_payload` | `sidecar/payload.rs` |
| `write_sidecar`, `write_design`, `read_canvas_file`, `read_sidecar`, `read_design`, `read_preview`, `delete_sidecar`, `write_blank_canvas` | `sidecar/io.rs` |
| `spawn_load_image`, `spawn_load_design`, `spawn_load_slot`, `spawn_deck_probe`, `probe_page_sizes` | `loader/load_ops.rs` |
| `spawn_save`, `spawn_save_design`, `spawn_reserve_canvas_path`, `spawn_pick_save_path`, `spawn_pick_design_path` | `loader/save_ops.rs` |
| `spawn_export_raster`, `spawn_export_vector`, `spawn_pick_export_path` | `loader/export_ops.rs` |
| `spawn_gallery_op`, `spawn_gallery_scan`, `spawn_document_rename`, `spawn_document_delete`, `spawn_restore_from_trash`, `spawn_single_thumb`, `duplicate_into`, `rename_with_sidecar`, `trash_with_sidecar`, `trash_locally_with_sidecar` | `loader/gallery_ops.rs` |
| `spawn_load_image_as_layer`, `spawn_pick_replacement_image`, `spawn_load_replacement_image_from_url`, `download_url_to_temp`, `image_label_from_path`, `image_label_from_url`, `extension_from_url` | `loader/image_import.rs` |
| `AppMsg`, `LoadOutcome`, `GalleryOp` | `loader/mod.rs` |
| `page_ui`, `page_size_presets_ui` | `properties_panel/page.rs` |
| `blur_control`, `color_adjustments_ui`, `shadow_ui` | `properties_panel/effects.rs` |
| `file_name_ui`, `layer_properties_ui` | `properties_panel/layer_common.rs` |
| Rama `LayerContent::Image(..)` de `content_properties_ui` | `properties_panel/content_image.rs` |
| Rama `LayerContent::Text(..)` | `properties_panel/content_text.rs` |
| Rama `LayerContent::Shape(..)`/`Svg(..)` | `properties_panel/content_shape.rs` |
| `properties_ui`, `commit_stale_panel_edits`, dispatcher de `content_properties_ui` | `properties_panel/mod.rs` |
| Bloque de menú superior dentro de `ui()` | `app/ui_menu.rs` |
| Bloques de modal guardar/overwrite/export + ventana About dentro de `ui()` | `app/ui_modals.rs` |
| Bloque `CentralPanel` de enrutamiento Welcome/Loading/Gallery/Editor dentro de `ui()` | `app/ui_views.rs` |
| `App`, `View`, `Nav`, `App::new`, `fn ui()` reducido a dispatcher | se queda en `app/mod.rs` |
| `GalleryState`, `FolderNavigation`, lógica de scan/orden, `GalleryAction` | `gallery/mod.rs` |
| `show`, `folder_panel_contents`, `show_folder_panel`, `gallery_cell`, `begin_rename`, helpers de layout (`gallery_column_count`, `gallery_cell_size`) | `gallery/ui.rs` |

## 4. Plan de ejecución (fases, orden de menor a mayor acoplamiento/riesgo)

1. **Fase 1 — `sidecar.rs` → `sidecar/`.** Puro I/O, sin egui, sin estado
   compartido con el resto de la app. Riesgo bajo. `git mv` + split mecánico.
2. **Fase 2 — `loader.rs` → `loader/`.** Funciones `spawn_*` independientes
   entre sí (comparten solo `Sender<AppMsg>`/`egui::Context` como
   parámetros, no estado). Riesgo bajo-medio.
3. **Fase 3 — `editor/properties_panel.rs` → `properties_panel/`.** Cada
   función ya recibe `&mut EditorState` explícito, así que dividir por tipo
   de contenido es mecánico. Riesgo medio: probar manualmente edición de
   propiedades de cada tipo de capa (imagen/texto/forma) y de página tras el
   split.
4. **Fase 4 — `app/mod.rs::ui()` → `ui_menu.rs` / `ui_modals.rs` /
   `ui_views.rs`.** La más delicada: el código tiene comentarios explícitos
   advirtiendo sobre el ORDEN estricto de evaluación de los modales
   (guardar-antes-de-salir bloquea el export, etc.). Extraer a funciones que
   reciban `&mut self` y se llamen en el mismo orden exacto, sin cambiar
   ninguna condición. Riesgo alto: requiere prueba manual obligatoria de
   guardar/sobrescribir/exportar/cerrar con cambios sin guardar antes de
   continuar.
5. **Fase 5 (opcional) — `gallery.rs` → `gallery/{mod,ui}.rs`.** Separar
   estado de renderizado. Riesgo bajo-medio.

Cada fase: `git mv` primero (sin tocar lógica) cuando aplique, luego split
mecánico, y en cada sub-paso: `cargo test`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo fmt --all -- --check`, y build real
(`cargo run -p canvas-app`) ejercitando la funcionalidad tocada — igual que
en la ronda 1.

### Estimación
Fases 1–2: rápidas (~20–30 min cada una). Fase 3: media (~45 min). Fase 4:
larga (>1h, por las pruebas manuales obligatorias de todos los flujos de
guardado/export). Fase 5: rápida-media.

### Riesgos y decisiones abiertas
- **Fase 4 es la de mayor riesgo real** de esta ronda: si el orden de
  evaluación de los modales se altera al extraer funciones, se puede
  reintroducir el bug que el comentario actual ya advierte evitar. Se
  recomienda hacerla al final y con diffs pequeños revisables uno a uno.
- ¿Incluir la Fase 5 (`gallery.rs`) en este ciclo o dejarla para después?
  Recomiendo incluirla — es de bajo riesgo y sigue el mismo patrón que ya
  se usó en `editor/` (separar estado de UI).
- `command.rs` (~1383 líneas, ~20 `Command`s) y `deck.rs` (~1646 líneas,
  pero ~500 son tests) quedan **fuera de alcance** por ahora: son archivos
  con una sola responsabilidad de fondo (comandos de undo/redo; estructura
  de datos del deck virtualizado) — partirlos no mejora SRP, solo reparte
  líneas. Se puede reconsiderar si crecen mucho más.
- Pendiente decidir: ¿un commit por fase, o uno solo al final? (en la ronda
  1 se hizo un commit por fase).

### Verificación
- `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check` tras cada fase.
- Prueba manual en `cargo run -p canvas-app`:
  - Fase 1–2: abrir/guardar diseño, duplicar/renombrar/borrar/restaurar
    desde la galería, exportar PNG/JPEG/SVG/PDF.
  - Fase 3: editar propiedades de cada tipo de capa (imagen: crop/blur/
    sombra/ajustes de color; texto; forma; página: tamaño/presets).
  - Fase 4 (crítica): cerrar la app con cambios sin guardar (modal
    guardar), sobrescribir un archivo existente, exportar con un modal de
    guardado pendiente, abrir la ventana "About".
  - Fase 5: navegar carpetas, buscar, renombrar/eliminar desde la galería.
