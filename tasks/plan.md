# Plan de implementación: cerrar los fallos residuales bajo presión de memoria

## Descripción

El incidente de `14.png` dejó claro que la app puede fallar bajo presión de
memoria. Ya hay cinco protecciones en producción (política de sidecar,
guard anti-blanco, presupuesto de baraja por RAM total, reducción dinámica
por RAM libre y aviso antes de Save All). Este plan cierra lo que **todavía
puede fallar**, en orden de apalancamiento:

1. **Bake parcial → corrupción silenciosa.** El guard anti-blanco solo
   detecta horneados *uniformes*. Hay dos clases de bake parcial:
   - *Imagen ausente del mapa* (la carga de la capa falló o es 0×0): el
     `drawable_image` devuelve `None` y la capa se omite en silencio. **Se
     puede detectar** contando las capas visibles omitidas al construir la
     escena.
   - *Imagen descartada por el atlas de vello* (no cabe → `xy = None`): la
     escena ve el `ImageData` perfectamente y vello la deja caer al
     renderizar, sin exponerlo. **No se puede detectar post-hoc** con vello
     0.9; se cierra en origen con un presupuesto GPU que garantice que lo
     que la app registra cabe en el atlas (Tarea 6).
2. **Crash por agotamiento del kernel/GPU** (`mach_vm_allocate_kernel
   failed`, los 4 crashes). El documento activo no tiene presupuesto: muchas
   capas con blur (4 texturas ~12-48 MB cada una) pueden agotar la VRAM.
3. **Save/Export individual sin aviso ni corte.** El aviso solo cubre Save
   All, y la medición de RAM es una foto del momento del clic: la presión
   puede dispararse durante el horneado.
4. **Precarga de la baraja bajo presión crítica**: sigue pidiendo ranuras
   que `evict` descarta de inmediato (churn de cargas).

## Decisiones de arquitectura

- **El renderer informa, el guard decide.** Se añade un contador de capas
  de imagen visibles omitidas (`skipped`) al camino de *bake* (una variante
  `bake_page_*` que lo devuelve). El camino de pantalla conserva la omisión
  silenciosa (la carga asíncrona legitima). El guard pasa de «anti-blanco»
  a «anti-incompleto»: rechaza escribir si el bake es uniforme **o** si se
  omitió alguna capa de imagen visible.
- **`bake_page` existente no cambia su firma**: delega en la variante con
  contador. Ejemplos/tests no se tocan; solo `persistence.rs` (y los tests
  GPU nuevos) usan la variante con informe.
- **Un solo umbral crítico nuevo**: `CRITICAL_FREE_RAM_BYTES = 512 MiB`
  (el suelo del presupuesto de la baraja, coherente). Por debajo: se pausa
  la precarga de fondo y Save/Export individuales fallan rápido con mensaje
  claro, sin intentar el bake. `FREE_RAM_REDUCTION_THRESHOLD_BYTES` (2 GiB)
  sigue siendo el umbral de «bajar el ritmo».
- **Presupuesto GPU del documento activo (y de la baraja)**: el
  `BlurEngine` contabiliza bytes por `(scope, capa)` de texturas de efectos
  + copias reducidas, y expulsa las menos usadas (LRU) cuando se supera un
  presupuesto escalado por RAM (misma política que la baraja: 1/16 de la
  RAM física, [256 MiB, 1 GiB], reducido por RAM libre). Acota el mayor
  componente controlable del atlas; no es una garantía matemática de que
  quepa (ver Riesgos).
- **jetsam de macOS no es prevenible por código**: se mitiga reduciendo la
  presión en origen y por la atomicidad del guardado (el original nunca se
  corrompe).

## Lista de tareas

### Fase 1 — Seguridad de archivo (lo primero, mayor apalancamiento)

#### Task 1: El renderer informa de capas de imagen omitidas en el bake
**Descripción:** En el camino de horneado (no en pantalla), contar las
capas visibles `Image`/`Svg` cuyo `drawable_image` devuelve `None` y
exponerlo al llamador: variante `bake_page_*` que devuelve el contador;
`bake_page` actual delega con un contador desechable. En el build de escena,
una función interna con contador (`append_document_counting`), pública sin
él.

**Criterios de aceptación:**
- [ ] `bake_page` (firma actual) sigue funcionando igual y devuelve lo mismo.
- [ ] La variante con informe devuelve 0 omitidas cuando todas las capas
      tienen imagen, y ≥ 1 cuando una capa visible no la tiene.
- [ ] Los SVG y las capas ocultas cuentan igual que las imágenes (solo las
      VISIBLES).

**Verificación:**
- [ ] Tests: `cargo test -p canvas-render` (test de CPU en `scene/tests.rs`:
      doc con capa visible sin imagen → contador 1; con imagen → 0).
- [ ] Build: `cargo clippy -p canvas-render --all-targets -- -D warnings`.
- [ ] Manual: `cargo run -p canvas-render --example save_roundtrip` sin
      regresión.

**Dependencias:** Ninguna.
**Archivos:** `crates/canvas-render/src/scene/document.rs`,
`crates/canvas-render/src/scene/mod.rs`, `crates/canvas-render/src/lib.rs`,
`crates/canvas-render/src/scene/tests.rs`.
**Tamaño:** Medio (3-5 archivos).

#### Task 2: Guard «anti-incompleto»: rechazar bakes con capas omitidas
**Descripción:** `bake_came_out_blank` se convierte en
`bake_came_out_blank_or_incomplete`: además del caso uniforme, rechaza
escribir cuando el bake informa ≥ 1 capa de imagen visible omitida (aunque
el resultado sea no-uniforme), con el mismo error claro («the file was not
overwritten»). Aplica a Save y Export.

**Criterios de aceptación:**
- [ ] Bake uniforme con capas visibles → bloqueado (comportamiento actual).
- [ ] Bake NO uniforme pero con 1 capa visible omitida → bloqueado.
- [ ] Bake completo (0 omitidas, no uniforme) → permitido.
- [ ] Diseño vectorial monocromo sin capas de imagen → permitido.

**Verificación:**
- [ ] Tests: casos de tabla nuevos en `persistence_tests.rs`.
- [ ] Regresión GPU en `crates/canvas-render/tests/gpu_bake.rs` (ignored):
      documento con una capa cuya imagen falta en el mapa → horneado
      informa omitida (se puede asertar el contador, no los píxeles).
- [ ] `cargo test --workspace`, clippy `-D warnings`, fmt.

**Dependencias:** Task 1.
**Archivos:** `crates/canvas-app/src/app/persistence.rs`,
`crates/canvas-app/src/app/persistence_tests.rs`,
`crates/canvas-render/tests/gpu_bake.rs`.
**Tamaño:** Medio (3 archivos).

### Checkpoint: Fase 1
- [ ] `cargo test --workspace` verde (con los tests nuevos).
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` limpio.
- [ ] `cargo test -p canvas-render --test gpu_bake -- --ignored` 12/12.
- [ ] Ningún archivo puede quedar corrupto: guard cubre uniforme + omitidas.
- [ ] Revisión humana antes de pasar a la Fase 2.

### Fase 2 — Comportamiento bajo presión

#### Task 3: Save/Export individuales fallan rápido bajo RAM crítica
**Descripción:** En `start_save` y `start_export` (el camino común de Save,
Save As y cada ranura de Save All), si la RAM libre medida en el momento
del bake está por debajo de `CRITICAL_FREE_RAM_BYTES`, no se intenta el
horneado: `save_error` claro sin tocar el archivo. Cierra también el hueco
temporal del aviso de Save All (la medición se hace en el bake, no solo en
el clic del menú).

**Criterios de aceptación:**
- [ ] Save bajo RAM crítica → error claro, sin intento de bake, archivo
      intacto.
- [ ] Export bajo RAM crítica → igual.
- [ ] Save/Export con RAM normal → comportamiento actual.

**Verificación:**
- [ ] Decisión pura `should_abort_save_for_low_memory(free: Option<u64>)`
      con tests de tabla (None → no aborta; < crítico → aborta).
- [ ] `cargo test -p canvas-app`, clippy, fmt.

**Dependencias:** Ninguna (paralelizable con Task 4).
**Archivos:** `crates/canvas-app/src/app/persistence.rs`,
`crates/canvas-app/src/app/persistence_tests.rs`.
**Tamaño:** Pequeño (2 archivos).

#### Task 4: Pausar la precarga de fondo bajo RAM crítica
**Descripción:** Con RAM libre < `CRITICAL_FREE_RAM_BYTES`, `request_loads`
no pide ranuras de fondo (solo el destino de un `jump_to` pendiente y la
activa siguen cargando): evita el churn cargar→expulsar bajo presión.

**Criterios de aceptación:**
- [ ] Bajo presión crítica, sin `jump_to` → no se pide ninguna ranura de
      fondo.
- [ ] Bajo presión crítica, con `jump_to` → se pide solo el destino.
- [ ] Con RAM normal → comportamiento actual (precarga completa).

**Verificación:**
- [ ] Función pura `should_pause_background_preload(free)` + tests de
      tabla; test de `request_loads` con la presión inyectada.
- [ ] Constante `CRITICAL_FREE_RAM_BYTES` en `deck/system.rs` (pub(crate)).
- [ ] `cargo test -p canvas-app`, clippy, fmt.

**Dependencias:** Ninguna (paralelizable con Task 3).
**Archivos:** `crates/canvas-app/src/deck/system.rs`,
`crates/canvas-app/src/deck/loading.rs`, `crates/canvas-app/src/deck/tests.rs`,
`crates/canvas-app/src/app/views/editor/deck_nav.rs`.
**Tamaño:** Pequeño (3-4 archivos).

### Checkpoint: Fase 2
- [ ] `cargo test --workspace` verde.
- [ ] Prueba UI manual con `memhog` (como la del modal): RAM < 512 MiB →
      Save individual falla rápido con mensaje, la baraja no precarga.

### Fase 3 — Presupuesto GPU del documento activo (el mayor, se divide)

#### Task 5: Contabilidad de bytes GPU por scope en BlurEngine
**Descripción:** `LayerFx` expone sus bytes (4 texturas × w×h×4) y
`DisplayEntry` los suyos; `BlurEngine` mantiene un total por `scope` y un
total global, con `bytes_in_scope(scope)` / `total_bytes()` y el contador de
`last_used` por entrada (monótono, sin reloj).

**Criterios de aceptación:**
- [ ] `total_bytes()` crece al sincronizar efectos y baja en `forget_scope`.
- [ ] `bytes_in_scope` distingue scopes (dos documentos no se mezclan).
- [ ] Tests de unidad con texturas falsas (sin GPU) si es posible, o en el
      ejemplo headless.

**Verificación:**
- [ ] `cargo test -p canvas-render`, clippy, fmt.

**Dependencias:** Ninguna (se puede empezar en paralelo con Fase 1).
**Archivos:** `crates/canvas-render/src/blur/mod.rs`,
`crates/canvas-render/src/blur/engine.rs`, `crates/canvas-render/src/blur/sync.rs`.
**Tamaño:** Pequeño-Medio.

#### Task 6: Presupuesto GPU con evicción LRU, escalado por RAM libre
**Descripción:** Cuando el total de `BlurEngine` supera el presupuesto
(1/16 de la RAM física, [256 MiB, 1 GiB], reducido por RAM libre — misma
política que la baraja), se expulsan las entradas (scope, capa) menos usadas
que no pertenezcan al scope en render activo, des-registrando sus texturas
de vello. Esto acota el mayor componente controlable del atlas: con blur
activo, la escena del documento pasa a usar la original (o la copia
reducida) hasta que la capa vuelva a entrar en vista. El resultado es que
un documento con muchas capas con efectos ya no puede agotar la VRAM ni
llenar el atlas: la clase «bake parcial por atlas» se cierra en origen.

**Criterios de aceptación:**
- [ ] Un documento con N capas con blur cuyo footprint supera el presupuesto
      no crashea ni llena el atlas: el bake informa 0 omitidas.
- [ ] La evicción no toca el scope activo del lienzo visible (no parpadea).
- [ ] Al volver a ver una capa expulsada, se re-sincroniza y reaparece.
- [ ] El presupuesto se reduce bajo RAM libre baja (misma tabla que la
      baraja).

**Verificación:**
- [ ] Test GPU (gpu_bake.rs, ignored): doc con muchas capas de blur →
      horneado completo, 0 omitidas, sin crash.
- [ ] `cargo run -p canvas-render --example bake_blur` sin regresión.
- [ ] Prueba UI manual: documento con varios blurs grandes + baraja llena,
      navegar, guardar — sin crash y guardado completo.

**Dependencias:** Task 5.
**Archivos:** `crates/canvas-render/src/blur/mod.rs`,
`crates/canvas-render/src/blur/engine.rs`, `crates/canvas-render/src/blur/sync.rs`,
`crates/canvas-render/src/lib.rs`, `crates/canvas-app/src/app/views/editor/canvas/paint.rs`,
`crates/canvas-render/tests/gpu_bake.rs`.
**Tamaño:** Grande — si supera un turno, dividir en 6a (evicción por vista +
LRU) y 6b (acople con RAM libre + presupuesto escalado).

### Checkpoint: Fase 3 (completo)
- [ ] `cargo test --workspace` verde; `cargo clippy --workspace --all-targets
      -- -D warnings` limpio; fmt OK.
- [ ] `cargo test -p canvas-render --test gpu_bake -- --ignored` completo.
- [ ] Prueba de presión real (memhog): abrir documento pesado + baraja,
      guardar y exportar — sin crash, sin archivos blancos/parciales.
- [ ] Documentar en CLAUDE.md el presupuesto GPU y el guard anti-incompleto.

## Riesgos y mitigaciones

| Riesgo | Impacto | Mitigación |
|--------|---------|------------|
| El contador de omitidas no cubre los descartes del atlas de vello (no expuestos en 0.9) | Medio (corrupción parcial) | Tarea 6 lo cierra en origen (presupuesto que garantiza que cabe); el contador cubre la clase «imagen ausente» |
| El presupuesto GPU no es una garantía matemática de que el atlas quepa (empaquetado + fragmentación + el propio vello registra fuentes originales) | Bajo-Medio | Acota el componente controlable más grande (fx + display); el guard anti-incompleto queda como red de seguridad |
| Evicción LRU visible (parpadeo) si toca el scope activo | Bajo | La evicción excluye el scope en render activo; re-sync bajo demanda |
| jetsam de macOS mata la app bajo presión | No prevenible por código | Reducción de presión en origen (Tareas 3, 4, 6) + atomicidad del guardado (el archivo nunca se corrompe) |
| Cambio de firma de bake_page rompe ejemplos/tests | Bajo | `bake_page` actual delega y no cambia; la variante con informe es aditiva |

## Preguntas abiertas

- **Umbral crítico:** ¿512 MiB de RAM libre es el punto correcto para
  «detener trabajo extra»? Se puede afinar tras la prueba de presión.
- **Techo del presupuesto GPU:** se propone el mismo [256 MiB, 1 GiB] que
  la baraja (1/16 de la RAM física). ¿Preferirías escalarlo por VRAM en
  vez de por RAM en máquinas con GPU discreta pequeña (4 GB)?
- **Evicción de `display` (copias CPU):** se propone incluirla en el
  presupuesto (es CPU, barato de regenerar). ¿Alguna objeción?
