# Todo — Cerrar los fallos residuales bajo presión de memoria

Plan completo en `tasks/plan.md`.

## Fase 1 — Seguridad de archivo

- [x] Task 1: El renderer informa de capas de imagen omitidas en el bake
  (variante `bake_page_*` con contador; `bake_page` delega; tests de CPU en
  `scene/tests.rs`).
- [x] Task 2: Guard «anti-incompleto»: rechazar bakes uniformes **o** con
  capas de imagen visibles omitidas (Save y Export) + tests de tabla +
  regresión GPU.

### Checkpoint: Fase 1
- [x] `cargo test --workspace` verde; clippy `-D warnings` limpio; fmt OK.
- [x] `cargo test -p canvas-render --test gpu_bake -- --ignored` (hoy 15/15
  con las tareas de Fase 3).
- [x] Revisión humana antes de Fase 2.

## Fase 2 — Comportamiento bajo presión

- [x] Task 3: Save/Export individuales fallan rápido bajo RAM crítica
  (< `CRITICAL_FREE_RAM_BYTES`) — cierra el hueco temporal del aviso de
  Save All. (`f5b4acb`)
- [x] Task 4: Pausar la precarga de fondo de la baraja bajo RAM crítica
  (solo `jump_to`/activa siguen cargando). (`fbec642`)

### Checkpoint: Fase 2
- [x] `cargo test --workspace` verde (459).
- [x] Prueba UI con memhog: RAM < 512 MiB → Save falla rápido (banner +
  archivo intacto) y la baraja no precarga (0 vs 5 preloads).

## Fase 3 — Presupuesto GPU del documento activo

- [x] Task 5: Contabilidad de bytes GPU por scope en BlurEngine
  (`total_bytes()`, `bytes_in_scope`, `last_used`). (`7b1adec`)
- [x] Task 6: Presupuesto GPU con evicción LRU (1/16 RAM, [256 MiB, 1 GiB],
  reducido por RAM libre), excluyendo el scope en render activo; cierra el
  «bake parcial por atlas» en origen. (`9758532`)

### Checkpoint: Fase 3 (completo)
- [x] `cargo test --workspace` (463), clippy `-D warnings`, fmt verdes.
- [x] gpu_bake completo (15/15).
- [x] Prueba de presión real (memhog): sin crash, sin archivos
      blancos/parciales — Save bajo 0.71 GiB completo; Export bajo 0.68 GiB
      completo (no blanco); Export bajo RAM crítica (< 512 MiB) rechazado con
      banner y archivo intacto.
- [x] Documentar en CLAUDE.md el presupuesto GPU y el guard anti-incompleto.
