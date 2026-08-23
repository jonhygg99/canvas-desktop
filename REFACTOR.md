# Refactorización modular de Canvas Desktop

Estado: **En curso** — Fases 0-8 hechas.

Objetivo: ≤ 400 líneas de código por archivo (tests aparte) y ≤ 80 líneas por
función, aplicando SRP a los archivos que hoy acumulan varias
responsabilidades. **Ninguna fase cambia comportamiento**: si aparece un bug
durante un traslado se anota aquí, no se arregla dentro del refactor.

Truco que hace barato casi todo el plan: en Rust los `impl` inherentes solo
exigen estar en el mismo **crate** que el tipo, no en el mismo módulo. Así
`impl Deck` se reparte entre `deck/cache.rs`, `deck/loading.rs`, etc. sin
traits, sin wrappers y sin tocar una sola llamada.

## Diagnóstico de partida

Líneas de código (sin contar los bloques `#[cfg(test)]`):

| Archivo | Código | Tests | Responsabilidades |
|---|---|---|---|
| `canvas-app/src/deck.rs` | 1208 | 569 | 6 |
| `canvas-app/src/editor/state.rs` | 1170 | 0 | 6 |
| `canvas-app/src/app/ui_views.rs` | 898 | 0 | `editor_view_ui`: 705 líneas, 32 parámetros |
| `canvas-app/src/gallery/ui.rs` | 844 | 0 | 4 |
| `canvas-app/src/editor/canvas_view.rs` | 798 | 0 | `canvas_ui`: 691 líneas |
| `canvas-app/src/app/messages.rs` | 790 | 0 | un `match` de 23 brazos |
| `canvas-core/src/command.rs` | 727 | 656 | 17 comandos + `History` |
| `canvas-app/src/menus.rs` | 603 | 0 | 3 |
| `canvas-io/src/export.rs` | 602 | 0 | 3 |
| `canvas-app/src/editor/slot_chrome.rs` | 579 | 0 | 2 |
| `canvas-render/src/blur.rs` | 457 | 0 | 3 |

`canvas-shell` no entra en el plan: su archivo mayor son 258 líneas y ya está
separado por plataforma.

## Fases

- [x] **Fase 0 — Red de seguridad.** Dejar el árbol verde con el toolchain
      actual y escribir este documento.
- [x] **Fase 1 — `canvas-core`.** `command.rs` → `command/`, `align.rs` →
      `geometry/`, `document.rs` → `document/`.
- [x] **Fase 2 — `canvas-io` + `canvas-render`.** `export.rs` → `export/`,
      `blur.rs` → `blur/`, `scene.rs` → `scene/`.
- [x] **Fase 3 — `deck.rs` → `deck/`.**
- [x] **Fase 4 — `editor/state.rs` → `editor/state/`.**
- [x] **Fase 5 — UI hoja.** `menus/`, `gallery/ui/`, `editor/slot_chrome/`.
- [x] **Fase 6 — `canvas_ui` → `editor/canvas/`.**
- [x] **Fase 7 — `EditorFrame` + `editor_view_ui` → `app/views/editor/`.**
- [x] **Fase 8 — `App` en sub-estados + `app/messages/`.**

## Verificación al final de cada fase

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p canvas-app          # y ejercitar la feature afectada
```

La Fase 2 añade los ejemplos headless de GPU (`bake_blur`, `bake_filters`,
`save_roundtrip`, `export_probe`, `text_probe`), que son lo único que cubre el
camino GPU.

### Guion de smoke test manual

1. Abrir una imagen suelta; zoom (`Ctrl+0`, `Ctrl+Alt+0`), rueda, paneo.
2. Abrir una carpeta; saltar entre lienzos (tira, `PageUp`/`PageDown`, clic).
3. Editar una capa (mover, redimensionar, rotar); `Ctrl+Z` / `Ctrl+Y`.
4. `Ctrl+S` y `Ctrl+Shift+S`; comprobar el aviso de sobrescritura.
5. Exportar a PNG y a PDF.
6. Renombrar y borrar un archivo desde la galería; deshacer el borrado.
7. Crear un lienzo nuevo desde la zona "+" y editarlo (materialización).
8. Modificar el archivo abierto desde fuera y comprobar el banner de recarga.

## Notas de las fases

### Fase 0

El árbol **no estaba verde** al empezar, por deriva de toolchain (rustc/clippy
1.97.1, rustfmt 1.9.0). Correcciones mecánicas, ninguna con cambio de
comportamiento:

- `clippy::replace_box` (lint nuevo) — `gallery/mod.rs`:
  `self.folders = Box::new(...)` → `*self.folders = ...`, evita realojar.
- `clippy::assertions_on_constants` — `sidebar.rs`: los dos `assert!` del test
  pasan a `const { assert!(..) }`.
- `clippy::needless_borrow` — `gallery/ui.rs`: dos `&parent` → `parent`.
- `clippy::large_enum_variant` — `app/mod.rs`: `View::Gallery` pasa a
  `Box<GalleryState>` (328 bytes frente a 32 de la segunda variante).
- `cargo fmt --all` sobre todo el árbol (10 archivos): normaliza el formato
  antes de empezar a mover código, para que los diffs de las fases siguientes
  sean movimientos puros y no ruido de reformateo.

Línea base tras la Fase 0: **208 tests en verde**, clippy y fmt limpios.

`PLAN.md` y `PROMPT.md` aparecen borrados sin commitear en el árbol de trabajo
desde antes de empezar; el refactor no los toca.

### Fase 1

`canvas-core` queda con 285 líneas en su archivo de código más largo (antes
727). Dos desvíos menores del plan, para no dejar archivos de 25 líneas:
`SetContent` va con `appearance` en vez de un `content.rs` propio, y
`Composite` va con `History` en vez de un `composite.rs` propio.

Los tests de cada carpeta quedan en un único `tests.rs`: muchos cruzan varias
familias a la vez (agrupar y luego deshacer) y repartirlos los haría menos
legibles. `History::redo` pasa a `pub(super)` para que esos tests sigan
pudiendo sembrar un comando que falla al rehacer.

### Fase 2

- `canvas-io/src/export.rs` (455 + 146) → `export/{mod,pdf}.rs` +
  `export/svg/{mod,image,text,shape,util}.rs`. Los ayudantes del SVG
  (`place_transform_svg`, `hex`, `alpha`, `esc`, `n`) pasan a `pub(super)`.
- `canvas-render/src/blur.rs` (457) → `blur/{mod,params,engine,passes}.rs`.
  `blur.wgsl` y `color_filter.wgsl` se mueven a `blur/` con el código que los
  hace `include_str!`.
- `canvas-render/src/scene.rs` (453) → `scene/{mod,raster,shape,text}.rs`.

Verificado además con los cinco ejemplos headless de GPU: `text_probe`,
`export_probe`, `bake_filters`, `bake_blur`, `save_roundtrip`, todos en verde
sobre GPU real.

#### Excepción documentada: `append_document`

`scene/mod.rs::append_document` se queda en ~180 líneas, por encima del
objetivo de 80. Es deliberado. Los brazos `LayerContent::Image` y
`LayerContent::Svg` contienen cuatro `continue` que saltan al bucle exterior,
así que extraerlos a funciones NO es un movimiento mecánico: cambiaría el flujo
de control. Solo se extrajo el brazo `Shape` (43 líneas, sin `continue`), que
sí lo es. Es código GPU sin cobertura de tests unitarios: no es sitio para
arriesgar.

#### 🐛 Bug preexistente encontrado (NO corregido aquí)

`scene/mod.rs::append_document`, brazos `Image` y `Svg`: cuando la textura de
la capa todavía no está cargada (`blurred`/`images` no la tienen) o mide 0x0,
el `continue` salta tanto el `if fade { scene.pop_layer(); }` como el
`i += 1` del final del bucle. Consecuencias:

1. **Bucle infinito**: `i` nunca avanza, el hilo de render se cuelga.
2. `push_layer`/`pop_layer` desbalanceados si la capa tenía `opacity < 1.0`.

Es alcanzable mientras una imagen se está cargando de forma asíncrona. Se deja
tal cual a propósito (el refactor no cambia comportamiento); merece su propio
arreglo con un test de regresión.

### Fase 3

`deck.rs` (1208 + 569) → `deck/{mod,geometry,model,layout,cache,loading,scan,nav,tests}.rs`.
Archivo de código más largo: 1208 → 289.

Lo único que cambia además de la ubicación son visibilidades: los ayudantes
que ahora cruzan módulo (`Slot::size`, `Slot::last_seen`,
`DeckRect::intersects`, `idle_slot`, `file_name`, `slot_kind`, los
presupuestos de caché y los límites de carga) pasan a `pub(super)`, visibles
solo dentro de `deck`. `SeedItem` deja de reexportarse desde `deck` porque
nadie fuera lo nombraba.

### Fase 4

`editor/state.rs` (1170) → `editor/state/`:

| Archivo | Líneas | Contenido |
|---|---|---|
| `mod.rs` | 272 | `struct EditorState`, `DeckNav`, y `file_name`/`is_dirty`/`is_idle`/`take_slot`/`put_slot` |
| `history.rs` | 275 | `GlobalStep`, `DeleteRecord` y todo el deshacer/rehacer local + global |
| `layer_factory.rs` | 199 | alta y sustitución de capas |
| `background.rs` | 153 | la capa de «fondo desenfocado» |
| `shortcuts.rs` | 151 | `handle_shortcuts` |
| `constructors.rs` | 137 | `base`, `from_image`, `new_blank`, `new_blank_image` |
| `sidecar.rs` | 67 | ida y vuelta con el sidecar |

**Trampa de visibilidad**: en `state.rs`, `pub(super)` significaba «visible en
`editor`». Desde un submódulo de `state` eso pasaría a significar «visible en
`state`», que es más restrictivo y rompe a `canvas_view`/`properties_panel`.
Los métodos movidos usan `pub(in crate::editor)`, que preserva exactamente la
visibilidad original. Los campos del struct siguen en `mod.rs`, así que su
`pub(super)` se queda como estaba.

### Fase 5

- `menus.rs` (603) → `menus/{mod,fallback}.rs` + `menus/native/{mod,build}.rs`.
  El bloque `#[cfg(windows)] mod native { … }` en línea pasa a ser un
  directorio de verdad, y su `fn build` (168 líneas de construcción del árbol
  de menús con muda) se separa del ciclo de vida. El fallback no-Windows se
  comprobó compilándolo temporalmente en Windows, ya que aquí la CI de
  Linux/macOS es lo único que lo cubre.
- `gallery/ui.rs` (844) → `gallery/ui/{mod,cell,folder_panel,shell}.rs`.
- `editor/slot_chrome.rs` (579) → `slot_chrome/{mod,header,icons}.rs`.

Misma trampa de visibilidad que en la Fase 4: lo que era `pub(super)` en un
archivo hijo directo de `editor` pasa a `pub(in crate::editor)` al bajar un
nivel. Afecta a `SlotHeader` y sus campos, `slot_header_layout` y
`draw_header_tooltips`.

### Fase 6

`editor/canvas_view.rs` (798, con `canvas_ui` de 691 líneas) →
`editor/canvas/`:

| Archivo | Líneas | Contenido |
|---|---|---|
| `mod.rs` | 277 | `CanvasAction` y `canvas_ui`, ya solo orquestación |
| `context_menu.rs` | 216 | el menú de clic derecho |
| `picking.rs` | 155 | qué pasa al pulsar: cabecera, zona «+», salto de lienzo |
| `camera.rs` | 151 | centrado, autofit, zoom, rueda, paneo |
| `paint.rs` | 122 | escena vello, blit y overlays |
| `url_popup.rs` | 50 | «Replace from URL» |

**Regla que se respetó al pie de la letra**: las funciones extraídas se llaman
en el MISMO orden en que estaban sus bloques dentro de `canvas_ui`. Nada se
fusionó, nada se reordenó, nada se "aprovechó para simplificar". Lo único que
cambia en los cuerpos movidos es que `action` pasa de captura del closure a
`&mut Option<CanvasAction>` (de ahí los `*action = …`) y que `visible` llega
como `&[usize]` en vez de `Vec`.

`canvas_ui` sigue en 158 líneas de cuerpo, por encima del objetivo de 80: lo
que queda es la secuencia de orquestación en sí, y trocearla más solo movería
el orden significativo a otro archivo sin ganar nada.

### Fase 7

Dos pasos dentro de la misma fase.

**1. `EditorFrame`.** `app/frame.rs` agrupa los 25 campos de `App` que
`editor_view_ui` recibía sueltos. La firma pasa de **32 parámetros a 6**. Es
un struct de préstamos independientes (`&'a mut` campo a campo), no un
`&mut App`: eso es justo lo que permite seguir usando `state` —prestado de
`self.view`— a la vez que `deck`, `renderer` y compañía, cosa que un
`&mut self` no permitiría. Compiló a la primera.

**2. El troceado.** `app/ui_views.rs` (898) → `app/views/`:

| Archivo | Líneas | Contenido |
|---|---|---|
| `editor/deck_nav.rs` | 294 | acciones de la tira y de la cabecera, salto de baraja, «Save all», pasos globales pendientes |
| `editor/save_flow.rs` | 153 | guardar / guardar como |
| `editor/mod.rs` | 128 | `editor_view_ui`, ya solo orquestación |
| `gallery.rs` | 120 | vista de galería |
| `editor/panels.rs` | 110 | tira, capas, propiedades, área central |
| `editor/file_ops.rs` | 101 | renombrar, borrar, materializar provisional |
| `editor/modals.rs` | 57 | sobrescritura, readonly, exportación |
| `welcome.rs` | 45 | vista de bienvenida |
| `loading.rs` | 21 | vista de carga |

Misma regla que en la Fase 6: los submódulos se llaman en el MISMO orden en
que estaban sus bloques. Lo único que cambia en los cuerpos movidos es que
`open_next` y `pending_menu_action` pasan a ser parámetros de salida
(`*open_next = …`).

### Fase 8

**`app/messages.rs` (791) → `app/messages/`.** El `match` de 23 brazos queda
como despacho de una línea por brazo en `mod.rs` (105 líneas); el cuerpo de
cada respuesta vive en el submódulo de su dominio: `document.rs` (244),
`save.rs` (177), `gallery.rs` (173), `load.rs` (168), `export.rs` (36),
`shell.rs` (34).

Único cambio de forma en los cuerpos movidos: los `continue` que saltaban al
siguiente mensaje pasan a `return`. Es exactamente equivalente — el `match`
era lo último del cuerpo del `while`, así que continuar y salir del brazo son
lo mismo.

**Sub-estados de `App`.** Los 35 campos planos se agrupan en cuatro structs
por dominio: `SaveFlow` (11), `ExportFlow` (3), `DeckOps` (4) y `MenuMirror`
(3). Los nombres de campo NO cambian, solo se les antepone el del sub-estado
(`self.save_requested` → `self.save.save_requested`): renombrarlos además
habría hecho el diff mucho más difícil de revisar.

Efecto secundario bueno: `EditorFrame` baja de 25 préstamos sueltos a 11,
porque lo que ya viaja agrupado en `App` viaja agrupado también aquí.
