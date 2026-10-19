# Implementation Plan: primer lienzo del bulk en blanco (main v0.7.9)

## Overview

Tras un bulk de ~6 imágenes a la misma carpeta, el primer lienzo
(stem-01) aparece en blanco en la baraja; los demás, bien. Causa
verificada en main: la reserva `create_new` existe vacía antes del bake
y un escaneo intermedio la ve vacía → `thumb_failed = true`; el
reescaneo final conserva la marca si el `mtime` no cambió
(`deck/scan.rs:281` solo limpia dentro de
`if existing.mtime != mtime`), y nada vuelve a pedir ese thumb
(`on_gallery_scanned` solo consume `bulk_jump`, un único path).
El fix recuerda la tanda entera y reintenta una vez los thumbs
pegajosos, sin tocar orden ni caché.

## Architecture Decisions

- Recordar la tanda (`DeckOps::bulk_created: Vec<PathBuf>`) junto al
  `bulk_jump` existente, no sustituirlo: el salto sigue con su camino.
- Selector puro `Deck::bulk_thumb_retries()` (slot presente +
  `thumb.is_none()` + `thumb_failed`): testeable sin GPU ni AppInner.
- Reintento único y acotado vía `spawn_single_thumb`: sin bucles ni
  reescaneos extra; el flujo normal de thumbs sigue mandando.

## Task List

### Phase 1: Foundation

## Task 1: Recordar la tanda del bulk (implementado)

**Description:** Añade `bulk_created: Vec<PathBuf>` a `DeckOps` y lo
siembra al terminar el bulk en ambas ramas de `serper.rs` (misma
carpeta `:291` junto a `bulk_jump`, y otra carpeta junto a
`pending_deck`).

**Acceptance criteria:**
- [ ] La tanda entera queda recordada al terminar un bulk a la misma carpeta.
- [ ] La tanda entera queda recordada al terminar un bulk a otra carpeta.
- [ ] `bulk_jump` y el salto al primero siguen intactos.

**Verification:**
- [ ] Tests pass: `cargo test -p canvas-app deck::`
- [ ] Build succeeds: `cargo check -p canvas-app`
- [ ] Manual check: n/a (sin efecto visible aún)

**Dependencies:** None

**Files likely touched:**
- `crates/canvas-app/src/app/mod.rs`
- `crates/canvas-app/src/app/messages/serper.rs`

**Estimated scope:** Small: 1-2 files

## Task 2: Selector puro de reintentos (implementado)

**Description:** Implementa `Deck::bulk_thumb_retries()` en
`deck/scan.rs`: devuelve los paths creados con slot presente, sin
textura y con fallo. Incluye sus unit tests (selecciona solo
fallidos-sin-thumb; ignora sanos, ausentes y no-creados).

**Acceptance criteria:**
- [ ] Solo devuelve creados con slot + `thumb.is_none()` + `thumb_failed`.
- [ ] Simula reserva vacía → fallo → rescan con mismo `mtime`: lo sigue devolviendo.

**Verification:**
- [ ] Tests pass: `cargo test -p canvas-app bulk_thumb_retries`
- [ ] Build succeeds: `cargo check -p canvas-app`
- [ ] Manual check: n/a

**Dependencies:** Task 1

**Files likely touched:**
- `crates/canvas-app/src/deck/scan.rs`
- `crates/canvas-app/src/deck/tests.rs`

**Estimated scope:** Small: 1-2 files

### Checkpoint: Foundation

- [ ] `cargo test -p canvas-app deck::` verde
- [ ] `cargo clippy -p canvas-app --all-targets -- -D warnings` limpio
- [ ] La tanda se recuerda y el selector la filtra (tests en verde)

### Phase 2: Core fix

## Task 3: Reintento tras el escaneo (implementado)

**Description:** En `on_gallery_scanned`, tras `merge_scan`, se
reintenta lo seleccionado vía `spawn_single_thumb` mientras queden
pendientes: un hilo por path y escaneo (los escaneos los disparan
eventos de disco, no hay bucle) y la tanda se libera en cuanto un
escaneo ya no deja pendientes. Se eligió reintento-mientras-pendiente
frente al reintento único estricto porque el reintento puede caer en
un escaneo intermedio con el bake aún en curso; el un-hilo-por-escaneo
sigue siendo acotado y converge con los eventos del watcher.

**Acceptance criteria:**
- [x] Un creado con marca pegajosa recibe un thumb tras el reescaneo final.
- [x] Ningún path se reintenta más de una vez por escaneo (sin tormentas de mensajes).
- [x] El flujo normal de thumbs (no-bulk) no cambia.

**Verification:**
- [ ] Tests pass: `cargo test -p canvas-app`
- [ ] Build succeeds: `cargo check -p canvas-app`
- [ ] Manual check: bulk de 6 a la misma carpeta → 6/6 miniaturas al primer pintado

**Dependencies:** Tasks 1, 2

**Files likely touched:**
- `crates/canvas-app/src/app/messages/gallery.rs`

**Estimated scope:** Small: 1-2 files

### Checkpoint: Core fix

- [ ] Suite `canvas-app` verde, clippy `-D warnings`, `cargo fmt --check` OK
- [ ] Repro manual del síntoma original confirmado como resuelto
- [ ] Revisión humana antes del pulido manual final

### Phase 3: Polish

## Task 4: Verificación manual en app real

**Description:** Repro original (bulk de 6 a la misma carpeta, 6/6 al
primer pintado) más bulk a otra carpeta (`pending_deck` con thumbs).

**Acceptance criteria:**
- [ ] Bulk misma carpeta: 6/6 miniaturas al primer pintado.
- [ ] Bulk otra carpeta: thumbs presentes tras navegar al primero.

**Verification:**
- [ ] Tests pass: `cargo test -p canvas-app` (sin cambios)
- [ ] Manual check: app real con las fotos del bulk original

**Dependencies:** Task 3

**Files likely touched:** (ninguno)

**Estimated scope:** XS

### Checkpoint: Complete

- [ ] All acceptance criteria met
- [ ] Ready for review

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| El `mtime` sí cambia siempre en el disco del usuario y el bug no reproduce | Med | El reintento único es inocuo si no hay pegajosos; el manual lo confirma |
| Tormenta de reintentos si el archivo sigue vacío | Low | Acotado a un reintento por path y escaneo |
| `spawn_single_thumb` con firma distinta en main | Low | Ya verificado en el código actual antes de implementar |

## Open Questions

- Ninguna: causa y mecanismo verificados en el código de main.

## Parallelization

- Tasks 1→2→3 son secuenciales (cadena de dependencia).
- Task 4 (manual) puede correr en paralelo con cualquier otro trabajo una
  vez verde el Checkpoint Core fix.

## Definition of Done (proyecto)

`cargo test -p canvas-app` verde,
`cargo clippy -p canvas-app --all-targets -- -D warnings` limpio,
`cargo fmt --all -- --check` OK. (El `definition-of-done.md` referenciado
por el skill no existe en el disco; se usa el bar de AGENTS.md.)
