# Implementation Plan: "changed on disk" + Back muerto + lienzo extremadamente lento (main v0.7.9)

## Overview

Tres causas verificadas en el código, en el mismo plan porque comparten sesión de trabajo:

1. **Banner ⚠ tras guardar** (bulk a la misma baraja): los guardados
   atómicos solo se discriminan por ventana de tiempo
   (`ignore_fs_events_until`) y `on_saved` nunca limpia `external_change`.
2. **Back to gallery mudo**: con el lienzo limpio y todo guardado, el clic
   muere en una compuerta silenciosa (`request_nav` aparcaba en
   `deferred_nav` sin feedback, o `saved_exit_ready` cancelaba `after_save`).
3. **Lienzo extremadamente lento**: `BlurEngine::sync_display`
   (`crates/canvas-render/src/blur/sync.rs`) materializaba la copia reducida
   **antes** de comprobar si la caché `display` ya estaba vigente y
   descartaba el resultado. Cada frame, por cada capa visible sin efectos con
   lado mayor > `MAX_FX_DIM` (2048): `data.to_vec()` (copia RGBA completa) +
   `image::imageops::thumbnail` + allocación. Medido con el probe headless
   `fx_hotpath_probe` sobre una foto de 12 MP (4032×3024, 46,5 MB RGBA):
   **103 ms por capa y por frame** solo en la copia reducida.

## Architecture Decisions

- **Trazas primero** (XS, solo logging): confirman qué ruta muerde en la app
  real antes de tocar el watcher y la navegación.
- **El propio guardado nunca es "cambio externo"**: al completar `on_saved`
  con éxito el disco contiene por construcción lo recién escrito → se
  limpia el flag. Decisión de la otra vía (el evento del watcher) extraída a
  función pura testeable.
- **Back siempre produce navegación o mensaje visible**, nunca silencio.
- **Rendimiento: decidir antes de copiar píxeles.** La caché `display`
  conserva su semántica (las imágenes grandes siguen entrando capadas al
  atlas de vello); sólo cambia el orden: primero la decisión
  `Keep/Rebuild/Drop` por `Blob::id()`, y sólo en `Rebuild` se materializa
  la reducción. Mismo patrón puro-y-table-tested que `capped_dims`.
- **La sonda manda**: `CANVAS_DEBUG_PERF=1` mide antes/después (mismo
  espíritu que `CANVAS_DEBUG_ATLAS`). Las optimizaciones posteriores sólo
  se implementan si la medición las justifica — y no lo hicieron (Task 8).
- **El gate de RAM de guardado/exportación NO se cachea**: mide en vivo en
  el momento del bake (contrato A09/A10 en AGENTS.md). Solo se throttlea la
  resolución de los presupuestos del bucle de render.

## Task List

### Phase 1: Diagnose

## Task 1: Trazas para discriminar (implementado)

**Description:** `tracing` sin cambios de conducta en
`on_source_changed_on_disk` (evento, documento abierto, dentro/fuera de la
ventana de gracia), en `build_slot_doc` (hash del sidecar que no coincide),
en `on_saved` (OK/fallo + estado del historial), en `request_nav` (nombres
sucios, diálogo en vuelo) y en `saved_exit_ready` (motivo de la
cancelación).

**Acceptance criteria:**
- [x] Reproducir bulk→editar→guardar→Back deja en el log la ruta exacta que muerde.
- [x] Cero cambios de conducta (solo logs).

**Verification:**
- [x] Tests pass: `cargo test -p canvas-app` (619/619)
- [x] Build succeeds: `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Manual check: repro en app real con log adjunto (pendiente del usuario)

**Files touched:** `messages/document.rs`, `messages/save.rs`,
`app/navigation.rs`, `app/persistence.rs`

## Task 2: Decisión pura de "cambio externo" (implementado)

**Description:** `should_flag_external_change(open, event, own_save)` en
`messages/document.rs` + tabla de tests (solo el abierto; nunca dentro de la
ventana de gracia; sin documento abierto nunca).

**Acceptance criteria:**
- [x] Solo el documento abierto con ese path se marca.
- [x] Dentro de la ventana de gracia nunca se marca.
- [x] Conducta idéntica salvo lo testeado.

**Verification:**
- [x] Tests pass: `cargo test -p canvas-app external_change` (tabla 4 casos)
- [x] Clippy/fmt limpios

## Task 3: `on_saved` limpia el flag obsoleto (implementado)

**Description:** Al completar `on_saved` con éxito se limpia
`external_change`: el disco contiene por construcción lo recién escrito, así
que un flag previo (evento propio tardío fuera de ventana) es obsoleto. Se
traza el cambio.

**Acceptance criteria:**
- [x] Guardar un lienzo grande del bulk no deja banner pegado (la limpieza corre en el camino OK).
- [x] Una edición externa REAL posterior sigue levantando el banner (el flag solo se limpia al guardar; el watcher lo vuelve a poner).
- [x] Ediciones durante la escritura siguen pendientes (A04 intacto: `marked` y su mensaje no cambian).

**Verification:**
- [x] Tests pass: suite `canvas-app`
- [ ] Manual check: bulk→editar→Ctrl+S sin banner; toque externo CON banner (pendiente del usuario)

## Task 4: Back nunca muere en silencio (implementado)

**Description:** `nav_block_reason` (pura, con tabla) nombra la compuerta;
`park_nav` aparca sin perder la navegación y deja el motivo en
`save_error` **sin pisar** un error ya visible (feedback una vez, no spam);
con diálogo en vuelo se repinta y se traza en vez de retornar mudo;
`saved_exit_ready` ya deja su motivo visible y ahora también se traza.

**Acceptance criteria:**
- [x] Cada clic a Back produce navegación o mensaje visible, nunca nada.
- [x] Back funciona con y sin cambios pendientes (los tests de `request_nav`/`saved_exit_ready` siguen verdes).

**Verification:**
- [x] Tests pass: `cargo test -p canvas-app navigation::`
- [x] Build succeeds
- [ ] Manual check: Back limpio, con sucio, con Save en curso y con Save-all pendiente (pendiente del usuario)

**Files touched:** `app/navigation.rs`, `app/messages/save.rs`,
`app/persistence.rs`

### Phase 2: Performance (lienzo lento)

## Task 5: Sonda de rendimiento env-gated (implementado)

**Description:** `perf_probe.rs` (nuevo módulo) + instrumentación en
`editor/canvas/paint.rs`. Con `CANVAS_DEBUG_PERF=1` publica una línea cada
60 frames con ms totales, ms de codificación de escena, ms de render,
slots visibles, capas sincronizadas y **`display_rebuilds`** (copias
reducidas materializadas desde el reporte anterior). Sin la variable no
hay logs; el coste es dos lecturas de reloj monótono por frame.

**Acceptance criteria:**
- [x] Sin la variable no hay ni un log ni trabajo pesado (`OnceLock`).
- [x] Con la variable encendida, cada reporte trae ms y `display_rebuilds`.
- [x] Base medida: ver `todo.md` (probe headless + números).

**Verification:**
- [x] Tests pass: `cargo test -p canvas-app perf_probe` (cadencia + delta)
- [x] `cargo run -p canvas-render --example fx_hotpath_probe` → números reales

## Task 6: Fix core de la caché `display` (implementado)

**Description:** `DisplayPlan { Keep, Rebuild, Drop }` + `display_plan`
(puras, por `Blob::id()`) y `exceeds_fx_dim` (sin materializar). `sync_display`
decide primero y sólo llama a `capped_image` en `Rebuild`. Contador
`display_rebuilds` expuesto por `CanvasRenderer` como prueba de regresión.

**Acceptance criteria:**
- [x] En reposo `capped_image` no se llama: `display_rebuilds == 1` tras el primer frame y 0 copias después (probe: `{:.3} ms/frame` → 0.000).
- [x] Primera vez que aparece una capa grande se reduce una sola vez.
- [x] Editar/reemplazar los píxeles (nuevo `Blob::id()`) SÍ reconstruye (tabla `Rebuild`).
- [x] Imagen ≤ 2048 y capa con efectos: conducta idéntica (tabla `Drop`/`Keep`).
- [x] Cambiar radio de blur sigue re-horneando (la rama de efectos no se tocó).

**Verification:**
- [x] Tests pass: `cargo test -p canvas-render --lib` (27/27, 2 nuevos)
- [x] `cargo run -p canvas-render --example fx_hotpath_probe`:
  46,5 MB RGBA → primer frame 111 ms (copia + registro), reposo 0.000 ms/frame,
  copia reducida suelta 103 ms (lo que costaba CADA frame antes),
  1 sola materialización.
- [x] App real con `CANVAS_DEBUG_PERF=1` sobre un PNG de 3000×2000:
  `frame=1 total_ms=82.15 encode_ms=75.86 render_ms=6.29 visible_slots=1
  layers_synced=1 display_rebuilds=1` — una sola copia reducida y el frame
  siguiente ya no toca píxeles.
- [x] `cargo test -p canvas-render --test gpu_bake -- --ignored` → 19/19 en GPU real

**Files touched:** `blur/mod.rs`, `blur/sync.rs`, `blur/engine.rs`, `lib.rs`

## Task 7: Eliminar churn por frame (implementado)

**Description:** (a) `CanvasSurface` cachea el `wgpu::TextureView` (la vista
mantiene viva la textura) y sólo lo recrea al redimensionar. (b)
`surface::cached_render_budget` cachea por viewport los presupuestos del
bucle de render (efectos GPU y baraja) con cadencia de 1 s
(`deck::ram_probe_due`); el gate de guardado/exportación sigue midiendo en
vivo.

**Acceptance criteria:**
- [x] Un frame en reposo no hace la syscall de RAM en el bucle de render (cacheado 1 s).
- [x] `persistence.rs` no cambia: guardar sigue midiendo en el momento del bake.
- [x] El resize sigue mostrando la textura correcta (la vista se recrea con el tamaño).

**Verification:**
- [x] Tests pass: `cargo test -p canvas-app surface::` (mide una vez por intervalo)
- [x] Clippy/fmt limpios

## Task 8: Escena de los lienzos vecinos — NO se implementa (medido)

**Description:** Decisión tomada con la medición del probe, como pedía el
plan: re-codificar cada frame la escena de los slots **no activos** cuesta
**0.001 ms/frame** para un documento con una foto de 12 MP
(`append_document`, 60 frames), frente a los 103 ms/frame que costaba la
copia reducida. Es ruido: no compensa el riesgo de escenas cacheadas
obsoletas. Se cierra sin código nuevo y queda la medición en `todo.md`.

**Acceptance criteria:**
- [x] Medición documentada (`fx_hotpath_probe`).
- [x] Sin código nuevo (riesgo/beneficio desfavorable).

### Checkpoint: Performance

- [x] Base y resultado medidos y anotados (`todo.md`).
- [x] `display_rebuilds == 0` en reposo (probe: 1 total, 0 posteriores).
- [x] Sin regresión visual: la copia reducida se sigue usando para la escena.
- [x] Suite + clippy + fmt verdes.

### Phase 3: Polish

## Task 9: Verificación y cierre (implementado en lo automático)

**Description:** Suite del workspace, clippy, fmt y actualización de
`plan.md`/`todo.md` con las mediciones. La matriz manual en app real
(watcher/diálogos nativos, Back en los 4 estados, arrastre de una foto de
12 MP) queda pendiente de ejecutarse en la máquina del usuario.

**Acceptance criteria:**
- [x] `cargo test` (workspace), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
- [x] Plan y todo reflejan lo hecho y las mediciones.
- [ ] Matriz manual de 5 casos + rendimiento percibido (usuario).

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Limpiar el flag oculta un cambio externo real entre el bake y el rename | Low | Ventana mínima (el rename es inmediato al bake) + el hash del sidecar lo delataría al recargar |
| Repintar con diálogo en vuelo no lo trae al frente en algún SO | Med | El `save_error` como respaldo garantiza feedback visible |
| La caché `display` no se reconstruye tras editar la capa y se ve la foto vieja | Med | `DisplayPlan` keyed por `Blob::id()` + tabla `Rebuild`; el probe exige `display_rebuilds == 1` |
| Throttlear la RAM rompe el gate anti-guardado-bajo-presión | Med | Sólo se throttlea el bucle de render; `start_save`/`start_export` miden en vivo y sus tests no cambian |
| La vista wgpu cacheada se invalida al redimensionar | Low | Se recrea junto con la textura en `ensure`; el id de egui se re-registra en el mismo sitio |

## Open Questions

- Ninguna bloqueante. La matriz manual (Task 9) es lo único que falta.

## Definition of Done (proyecto)

`cargo test` (workspace) verde,
`cargo clippy --workspace --all-targets -- -D warnings` limpio,
`cargo fmt --all -- --check` OK, más las mediciones de rendimiento anotadas
en `tasks/bulk-save-external/todo.md` y la verificación manual en app real
(regla de oro de AGENTS.md, pendiente del usuario).
