# Tareas: primer lienzo del bulk en blanco (main)

Plan de diseño en `plan.md` (formato planning-and-task-breakdown).
Base: main v0.7.9. Sin tocar `tasks/plan.md`, `tasks/todo.md`
(framing, completos) ni los planes raíz `plan-*.md`.

## Task 1: Recordar la tanda del bulk

- [x] `DeckOps::bulk_created` + siembra en `serper.rs` (misma y otra carpeta)
- [x] Verificación: `cargo check -p canvas-app` limpio

## Task 2: Selector puro de reintentos

- [x] `Deck::bulk_thumb_retries()` + 2 unit tests en `deck/tests.rs`
- [x] Verificación: `cargo test -p canvas-app bulk_thumb_retries` 2/2

## Checkpoint: Foundation

- [x] Tests `deck::` verdes (62), clippy limpio, fmt OK

## Task 3: Reintento tras el escaneo

- [x] Reintento en `on_gallery_scanned` (un hilo por path/escaneo, libera al vaciarse)
- [x] Verificación: suite 612/612 + clippy `-D warnings` + fmt; pendiente manual bulk de 6 → 6/6

## Checkpoint: Core fix

- [x] Suite verde; pendiente repro manual y revisión humana

## Task 4: Verificación manual en app real

- [ ] Bulk misma carpeta 6/6; bulk otra carpeta con thumbs

## Checkpoint: Complete

- [ ] All acceptance criteria met, ready for review
