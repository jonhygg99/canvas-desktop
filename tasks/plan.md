# Plan: en Windows la segunda ventana se redimensiona sola

Plan anterior archivado en `tasks/archive/` (bug de RAM falsa en macOS).
Estado del trabajo nuevo en `tasks/todo.md`.

## Overview

Al abrir una segunda ventana (Ctrl+N / Ctrl+T / menú «New Window») en
Windows, la ventana nueva **se va agrandando sola** (~40 px por frame, el
alto de la barra de título + bordes) hasta llenar el área de trabajo, y
sigue «luchando» por redimensionarse si se toca el monitor o se arrastra.
No es un bug del SO: es un **bucle de realimentación** dentro de la app.

### Causa raíz (verificada contra las fuentes de eframe 0.35 del registro)

1. **Captura por frame** (`app/ws_frame.rs`, final del frame de cada
   ventana): `ws.geometry` se reescribe cada frame desde el rect vivo de la
   ventana — `outer_rect.or(inner_rect)`.
2. **Reaplicación por frame** (`app/workspace_lifecycle.rs`,
   `spawn_child_viewports`): el frame raíz reconstruye CADA frame el
   `ViewportBuilder` del viewport diferido de cada hija con
   `with_position(…)` y **`with_inner_size(tamaño_capturado)`** — y lo hace
   en los dos puntos de registro de las hijas (`App::logic`, pases ocultos,
   y `root_frame`).
3. **El amplificador** (eframe 0.35, `native/wgpu_integration.rs:1282`):
   eframe llama `viewport.builder.patch(builder)` **cada frame** con el
   builder que se le pasa a `show_viewport_deferred`; `patch` (egui
   `viewport.rs:770`) compara con el del frame anterior y emite
   `ViewportCommand::InnerSize` cada vez que `inner_size` **cambió**.

La trampa: el tamaño capturado es el rect **EXTERIOR** (cliente + barra de
título + bordes), pero se reaplica como tamaño **interior**. Cada frame el
interior pasa a medir lo que antes medía el exterior → la ventana crece el
alto de la decoración → el exterior nuevo es más grande → se captura y se
reaplica → **crece ~40 px por frame** hasta que Windows la recorta al área
de trabajo. En Windows la decoración es gruesa y cada `Resized` repinta, así
que el bucle rueda continuo y es muy visible; en Ctrl+N/Ctrl+T la hija
**nace ya inflada** porque hereda el `geometry` (exterior) del padre. El
fallback `outer_rect.or(inner_rect)` además puede alternar entre ambos rect
en cambios de DPI/monitor, lo que añade oscilación de posición.

## Architecture Decisions

- **La geometría solo se aplica al NACIMIENTO de la ventana hija.** Un flag
  por workspace (`geometry_seeded`): la primera vez que `spawn_child_viewports`
  registra un viewport incluye `with_position`/`with_inner_size` con la
  geometría heredada; a partir de ahí el builder solo lleva el título. Como
  `patch` solo emite comandos cuando cambia un valor, tras el nacimiento
  **nunca** viaja un `InnerSize`/`OuterPosition` — el redimensionado manual
  del usuario queda intacto y no hay bucle posible. No depende de los
  detalles internos del diff de `patch` (aunque estén verificados): el
  builder literalmente deja de ofrecer tamaño/posición.
- **Captura consistente = tamaño interior.** `ws.geometry` pasa a guardar
  SIEMPRE el tamaño del `inner_rect` (lo que `with_inner_size`/
  `StoredWorkspace.size` significan: «tamaño interior») y la posición del
  `outer_rect.min` (con fallback al `inner_rect.min` y a conservar el valor
  anterior si no hay ninguno). Se extrae como función pura
  `capture_geometry(outer, inner) -> Option<(Pos2, Vec2)>` con tests de
  tabla (convención del repo: `ws_frame_tests.rs` con `#[path]` o `tests.rs`
  según lo que ya use la crate).
- **La herencia de Ctrl+N/T no cambia**: la hija sigue naciendo con la
  geometría del padre (ahora ya coherente: interior = interior).
- **La persistencia en `settings.json` no cambia de semántica**: se siguen
  escribiendo pos/tamaño (historial; `bootstrap` ya los descarta al arrancar
  — la app siempre abre en la home con su tamaño por defecto). Solo se
  corrige que «size» sea lo que su doc dice que es (interior).
- **La raíz no se toca**: nace de `main.rs` (`NativeOptions`) y su
  `geometry` solo se captura para persistir, nunca se reaplica.

## Task List

### Task 1: Aplicar la geometría solo al nacer la ventana hija (S)

**Description:** Añadir `geometry_seeded: bool` a `Workspace` (por defecto
`false`). En `spawn_child_viewports`, incluir `with_position` +
`with_inner_size` en el builder únicamente la primera vez que se registra el
viewport de una hija; a partir de entonces builder = título solo. El flag se
marca la primera vez, también cuando la geometría era `None` (p. ej.
`CANVAS_DEBUG_WINDOWS`). La llamada a `show_viewport_deferred` sigue
haciéndose todos los frames con el mismo `ViewportId` (requisito de egui:
si no se llama, la ventana se cierra).

**Acceptance criteria:**
- [ ] La hija nace con la geometría heredada (Ctrl+N/T idéntico a hoy).
- [ ] Tras el nacimiento el builder no lleva tamaño/posición: `patch` no
      emite `InnerSize`/`OuterPosition` nunca más (verificable con el
      logging de comandos o leyendo `raw.viewports[id].builder`… mejor con
      la verificación de comportamiento: la ventana no crece frame a frame).
- [ ] Arrastre/redimensionado manual de la hija: el SO manda, la app no
      revierte ni reafirma nada.

**Verification:**
- [ ] `cargo test -p canvas-app` verde; `cargo clippy -p canvas-app --all-targets -- -D warnings` limpio; `cargo fmt --check` OK.
- [ ] Manual en Windows: Ctrl+N ×5 seguidas → las ventanas conservan el
      tamaño del padre y ninguna crece; arrastrar un borde → se queda.
- [ ] Sin regresión: Ctrl+Tab/conmutador y foco entre ventanas intactos.

**Dependencies:** None

**Files likely touched:**
- `crates/canvas-app/src/app/workspace.rs` (campo nuevo)
- `crates/canvas-app/src/app/workspace_lifecycle.rs` (builder)

**Estimated scope:** Small (2 files)

### Task 2: Captura de geometría consistente (interior) con tests (S)

**Description:** Reemplazar la captura `outer_rect.or(inner_rect)` del final
de `ws_frame` por la función pura `capture_geometry`: posición =
`outer_rect.min` (fallback `inner_rect.min`), tamaño = `inner_rect.size()`;
ambos `None` → conservar la geometría anterior. Tests de tabla fijando el
contrato (rect exterior e interior presentes → posición del exterior, tamaño
del interior; solo interior → ambos de él; ninguno → `None` para que el
llamador conserve el previo).

**Acceptance criteria:**
- [ ] `StoredWorkspace.size` persistido es SIEMPRE el tamaño interior
      (coherente con su documentación y con `with_inner_size`).
- [ ] Sin alternancia exterior/interior en cambios de DPI o `outer_rect`
      momentáneamente ausente.
- [ ] Tabla de tests nueva cubre los tres casos + el de conservar previo.

**Verification:**
- [ ] `cargo test -p canvas-app` verde (tests de la tabla incluidos).
- [ ] Manual en Windows: conectar/desconectar un segundo monitor no hace
      que las ventanas brinquen ni se redimensionen.

**Dependencies:** Task 1

**Files likely touched:**
- `crates/canvas-app/src/app/ws_frame.rs` (helper + captura)
- `crates/canvas-app/src/app/ws_frame_tests.rs` (nuevo, con `#[path]` según
  convención del crate) o `tests.rs` del módulo

**Estimated scope:** Small (2 files)

### Task 3: Verificación UI real en Windows (XS)

**Description:** Ejercitar el flujo completo en la máquina del usuario
(Windows): segunda ventana vía menú, Ctrl+N, Ctrl+T; redimensionado manual;
monitor secundario conectado/desconectado; maximizar; rearranque.

**Acceptance criteria:**
- [ ] Segunda ventana abre con el tamaño del padre y NO se redimensiona sola
      (observar 10+ segundos sin tocarla).
- [ ] Redimensionar/arrastrar a mano queda como el usuario la dejó.
- [ ] Sin monitor secundario tras desconectarlo: la app sigue usable y las
      ventanas no pelean por posición/tamaño.

**Verification:**
- [ ] Manual en Windows, con la app real (`cargo run -p canvas-app`).
- [ ] Repaso rápido del conmutador Ctrl+Tab y del cierre de ventanas.

**Dependencies:** Tasks 1-2

**Files likely touched:** ninguno (verificación)

**Estimated scope:** XS

### Checkpoint: Final
- [ ] `cargo test --workspace` verde, clippy `-D warnings`, fmt OK.
- [ ] Verificación manual en Windows: ninguna ventana se redimensiona sola,
      tamaño heredado al nacer, redimensionado manual respetado.
- [ ] Documentar en CLAUDE.md el porqué (captura interior + geometría solo
      al nacimiento) si el comentario del módulo no basta.

## Risks and Mitigations

| Risk | Impact | Mitigación |
|------|--------|------------|
| Regresión: la hija nace con tamaño equivocado | Med | El camino del primer frame es idéntico al actual (misma geometría heredada); el flag solo corta la reaplicación posterior |
| Bug de la ventana que «se cierra si no se registra» cada frame | Alto | La llamada a `show_viewport_deferred` con el mismo `ViewportId` se mantiene TODOS los frames; solo desaparecen tamaño/posición del builder |
| `patch` cambia de semántica al subir eframe | Bajo | El fix no depende del diff: tras el nacimiento el builder no ofrece tamaño/posición, no hay nada que difiere |
| Oscilación de posición por DPI/monitor | Bajo | Posición solo al nacimiento + captura con fallback estable (Task 2) |
| La raíz arrastra el bucle | Ninguno | La raíz nunca se registra vía builder (nace de `main.rs`); su geometría solo se persiste |

## Open Questions

- ¿Conviene además **recortar** la geometría heredada al área de trabajo del
  monitor (monitores desconectados entre sesiones)? Hoy `bootstrap` descarta
  lo persistido y la herencia es en vivo (padre visible), así que no hace
  falta — se deja como mejora opcional si se reactiva la restauración.