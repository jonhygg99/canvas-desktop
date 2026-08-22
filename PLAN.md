# PLAN.md

**Estado: En curso — Fase 2 completada**

## 📋 Plan — Modularizar los God Objects de canvas-app (editor.rs, main.rs)

### 🎯 Objetivo y alcance

- Modularizar `crates/canvas-app/src/editor.rs` (4825 líneas) y
  `crates/canvas-app/src/main.rs` (3348 líneas) siguiendo SRP: cada archivo
  nuevo con un propósito único (estado, viewport, interacción, overlays,
  panel de propiedades, navegación, mensajes, persistencia, ventana).
- Fuera de alcance por ahora: `deck.rs` (1646 líneas), `sidecar.rs` (1107),
  `loader.rs` (1064) — no tienen el mismo nivel de mezcla de
  responsabilidades; se revisan en un plan futuro si hace falta.
- Sin cambios de comportamiento: es refactor puro, sin nueva funcionalidad.

### Diagnóstico

**`editor.rs` (4825 líneas)** mezcla:

| Responsabilidad | Bloques actuales |
|---|---|
| Estado de documento / undo-redo (lógica de negocio, sin UI) | `EditorState` (176–1327): `add_image_layer`, `set_blurred_background`, `handle_shortcuts`, `undo`/`redo`, `record_edit_step` |
| Cámara/viewport del canvas | `Viewport`, `AutoFit`, `Gesture` (32–175) |
| Panel de propiedades (UI) | `properties_ui`, `page_ui`, `size_popup_ui`, `blur_control`, `color_adjustments_ui`, `shadow_ui`, `layer_properties_ui`, `content_properties_ui` (1328–2375, 4111–4302) |
| Operaciones de orden/alineación de capas (lógica, no UI) | `ZOrder`, `reorder_layer`, `apply_alignment` (2376–2443) |
| Render + interacción del lienzo principal | `canvas_ui`, `CanvasAction`, `replace_url_popup_ui` (2444–3168) |
| Hit-testing / drag / resize / rotate | `layer_interaction`, `corner_at`, transforms página↔pantalla (3744–4110) |
| Chrome de slots vecinos del deck dentro del canvas | `draw_slot_chrome`, `draw_slot_header`, iconos, tooltips, rename, add-zone (3169–3743) |
| Overlays de selección/grid/reglas | `draw_selection_overlay`, `draw_grid`, `draw_rulers`, `show_drag_tag` (4303–4444) |

**`main.rs` (3348 líneas)** mezcla:

| Responsabilidad | Bloques actuales |
|---|---|
| Máquina de estados de la app (eframe::App) | `App`, `View`, `Nav`, `impl eframe::App` |
| Navegación / apertura de archivos / deck | `open_path`, `navigate`, `request_nav`, `resolve_deck`, `new_design`, `add_canvas`, `toggle_deck_axis`, `cycle_strip_side` |
| Manejo de mensajes de hilos en background | `handle_messages` — 750 líneas en una sola función (904–1656) |
| Guardado / exportación (orquestación de hilos) | `start_save`, `start_save_all`, `start_save_design`, `start_export`, `build_slot_doc`, `resolve_canvas_sidecar` |
| Acciones de menú | `handle_menu_action` |
| Ventana / SO | `sync_title`, `confirm_close`, `load_app_icon`, `handle_dropped_files`, `thumbnail_cache_dir` |
| Entry point real | `fn main()`, `shell_registration_flag` |

### Arquitectura propuesta

No se crean crates nuevas; se modulariza dentro de `canvas-app` convirtiendo
los dos archivos en carpetas de módulos:

```
crates/canvas-app/src/
├─ main.rs                     # solo entry point: parseo de args, run_native, shell_registration_flag
├─ app/
│  ├─ mod.rs                   # struct App, View, Nav, impl eframe::App::ui (dispatch)
│  ├─ navigation.rs            # open_path, navigate, request_nav, resolve_deck, apply_deck_prefs,
│  │                           #   new_design, add_canvas, toggle_deck_axis, cycle_strip_side
│  ├─ messages.rs               # handle_messages, partida por variante de AppMsg
│  ├─ persistence.rs            # start_save, start_save_all, start_save_design, start_export,
│  │                           #   build_slot_doc, seed_gallery_from_deck, resolve_canvas_sidecar, is_jpeg_path
│  ├─ menu_actions.rs           # handle_menu_action
│  └─ window.rs                 # sync_title, confirm_close, load_app_icon, handle_dropped_files,
│                               #   thumbnail_cache_dir, push_recent, remember_page_size, dirty_canvas_names
│
├─ editor/
│  ├─ mod.rs                   # re-exports públicos (EditorState, canvas_ui, properties_ui, CanvasAction…)
│  ├─ state.rs                  # EditorState (struct + impl): modelo/undo, sin egui salvo firma mínima
│  ├─ viewport.rs               # Viewport, AutoFit, Gesture, page_to_screen/screen_to_page,
│  │                           #   layer_corners_screen, rotation_handle_screen
│  ├─ layer_ops.rs              # ZOrder, sibling_position, reorder_layer, apply_alignment (lógica pura)
│  ├─ interaction.rs            # layer_interaction, corner_at, Corner — hit-testing y gestos de drag/resize/rotate
│  ├─ canvas_view.rs            # canvas_ui, CanvasAction, replace_url_popup_ui — orquesta viewport+interaction+overlay
│  ├─ overlay.rs                # draw_selection_overlay, draw_grid, draw_rulers, show_drag_tag
│  ├─ slot_chrome.rs            # draw_slot_chrome, draw_slot_header, iconos, tooltips, rename overlay, add-zone
│  ├─ properties_panel.rs       # properties_ui, page_ui, size_popup_ui, blur_control, color_adjustments_ui,
│  │                           #   shadow_ui, layer_properties_ui, content_properties_ui, file_name_ui, format_dims
│  └─ tests.rs                  # el actual #[cfg(test)] mod tests (undo/redo, paste, replace…)
│
├─ deck.rs / loader.rs / gallery.rs / menus.rs / ...   # sin cambios en esta fase
```

Regla de dependencia: `state.rs`, `layer_ops.rs` e `interaction.rs` no deben
importar nada de `slot_chrome.rs`/`overlay.rs` (evita acoplamiento nuevo);
`canvas_view.rs` es el único módulo que compone todo.

### Mapeo de migración

- `editor.rs:32-175` (Viewport/AutoFit/Gesture) + `3744-3787` + `3760-3772` → `editor/viewport.rs`
- `editor.rs:176-1327` (`EditorState` + impl) → `editor/state.rs`
- `editor.rs:1328-2375`, `4111-4302` (paneles de propiedades) → `editor/properties_panel.rs`
- `editor.rs:2376-2443` (`ZOrder`, reorder, alignment) → `editor/layer_ops.rs`
- `editor.rs:2444-3168` (`canvas_ui`, `CanvasAction`, popup de reemplazo por URL) → `editor/canvas_view.rs`
- `editor.rs:3169-3743` (chrome de slots del deck) → `editor/slot_chrome.rs`
- `editor.rs:3788-4110` (`layer_interaction`, `corner_at`) → `editor/interaction.rs`
- `editor.rs:4303-4444` (overlays de selección/grid/reglas) → `editor/overlay.rs`
- `editor.rs:4506-4825` (tests) → `editor/tests.rs`
- `main.rs:189-380`, `2171-3348` (`App` struct + `impl eframe::App`) → `app/mod.rs`
- `main.rs:380-663` (navegación/apertura/deck) → `app/navigation.rs`
- `main.rs:904-1656` (`handle_messages`) → `app/messages.rs`
- `main.rs:725-757`, `1879-2137` (guardado/export) → `app/persistence.rs`
- `main.rs:757-903` (`handle_menu_action`) → `app/menu_actions.rs`
- `main.rs:1656-1781` (drag&drop, confirm close, título, icono) → `app/window.rs`
- `main.rs:1-190` (entry point, `shell_registration_flag`) → se quedan en `main.rs`, reducido a ~150 líneas

### Pasos

1. ✅ **Fase 0 — `editor.rs` → `editor/mod.rs` (andamiaje puro).** `git mv` a
   `editor/mod.rs`, sin tocar lógica. `app/` de `main.rs` se aborda en la
   Fase 5 (ver nota abajo). Build, 55 tests, clippy y fmt limpios.
2. ✅ **Fase 1 — Extraer lógica pura sin egui.** `Viewport`/`AutoFit` +
   funciones de coordenadas → `editor/viewport.rs`; `ZOrder`/`reorder_layer`/
   `apply_alignment`/`sibling_position` → `editor/layer_ops.rs`. `Gesture`
   se quedó en `mod.rs` (se revisa en la Fase 3). Build, 55 tests, clippy y
   fmt limpios.
3. ✅ **Fase 2 — Extraer `EditorState`.** `EditorState`/`GlobalStep`/
   `DeckNav`/`DeleteRecord` → `editor/state.rs`. `Gesture` se quedó en
   `mod.rs` (privado, pero visible desde el módulo hijo `state` por la regla
   de Rust de que un ítem privado es visible en su módulo Y sus
   descendientes). Build, 55 tests, clippy y fmt limpios.
4. **Fase 3 — Extraer interacción y overlays del canvas.** `interaction.rs`,
   `overlay.rs`, `slot_chrome.rs`, `canvas_view.rs`. Parte más grande y con
   más estado egui compartido — probar manualmente drag/resize/rotate,
   selección múltiple y slots vecinos del deck strip.
5. **Fase 4 — Extraer panel de propiedades.** `properties_panel.rs`. Bajo
   riesgo funcional, alto volumen de líneas.
6. **Fase 5 — Repetir el proceso en `main.rs`.** `messages.rs` primero (más
   fácil de partir por variante de `AppMsg`), luego `navigation.rs`,
   `persistence.rs`, `menu_actions.rs`, `window.rs`.
7. **Fase 6 — Limpieza final.** `cargo clippy --workspace --all-targets -- -D
   warnings` y `cargo fmt --all -- --check` sobre todo el workspace; revisar
   que no queden `pub` innecesarios expuestos solo por la partición.

### Estimación

Fases 0–1 rápidas (~30 min combinadas). Fases 2–3 largas (más de 1h cada una
por el volumen de estado compartido y pruebas manuales de gestos). Fases 4–6
medias.

### Riesgos y decisiones abiertas

- `canvas_view.rs`/`interaction.rs`/`slot_chrome.rs` comparten mucho estado
  mutable de `EditorState` y del `Viewport` — mayor riesgo al partir
  `canvas_ui` es romper el orden de borrowing. Se hace en su propia fase (3)
  con pruebas manuales obligatorias de drag/resize/rotate/selección antes de
  seguir.
- Pendiente decidir: ¿un commit por fase, o uno solo al final?
- `deck.rs` queda fuera de este plan por ahora; se puede añadir después si
  hace falta.
- Nota: el `PLAN.md` original del proyecto (tracking de fases del roadmap
  completo, último commit `d399aa7`) estaba borrado sin stagear en el árbol
  de trabajo al momento de escribir este archivo. Por decisión del usuario,
  este archivo lo reemplaza con solo el plan de refactor; el contenido
  anterior sigue recuperable vía `git show d399aa7:PLAN.md` o `git checkout
  d399aa7 -- PLAN.md` si hiciera falta.

### Verificación

- `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check` tras cada fase.
- Prueba manual en `cargo run -p canvas-app`: abrir imagen, editar
  (mover/rotar/reordenar capas), deshacer/rehacer, guardar, exportar —
  especialmente después de las Fases 2, 3 y 5.
