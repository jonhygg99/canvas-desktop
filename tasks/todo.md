# Todo — En Windows la segunda ventana se redimensiona sola

Plan completo en `tasks/plan.md` (el plan anterior de RAM macOS quedó
archivado en `tasks/archive/`). Diagnóstico: bucle de realimentación —
`ws_frame` captura cada frame el rect EXTERIOR de la ventana y
`spawn_child_viewports` lo reaplica cada frame como tamaño INTERIOR del
builder del viewport diferido; eframe 0.35 llama `builder.patch(builder)`
cada frame y emite `InnerSize` ante cualquier cambio, así que la hija crece
~40 px (la decoración) por frame hasta que Windows la recorta. Verificado en
`egui-0.35.0/src/viewport.rs:770` y `eframe-0.35.0/src/native/wgpu_integration.rs:1282`.

## Task 1: Aplicar la geometría solo al nacer la ventana hija
- [x] `Workspace.geometry_seeded: bool` (por defecto `false`).
- [x] En `spawn_child_viewports`: `with_position`/`with_inner_size` solo la
      primera vez que se registra el viewport; luego builder = título solo.
- [x] La llamada a `show_viewport_deferred` con el mismo `ViewportId` sigue
      todos los frames (si no, la ventana se cierra).
- [x] Ctrl+N/Ctrl+T: la hija nace con el tamaño del padre (herencia intacta —
      el primer frame del builder se siembra con la geometría heredada).

## Checkpoint: Task 1
- [x] `cargo test -p canvas-app`, clippy `-D warnings`, fmt OK (252 tests,
      workspace 473).
- [ ] Manual Windows: Ctrl+N ×5 → ninguna ventana crece; arrastrar borde → se queda.

## Task 2: Captura de geometría consistente (interior) + tests
- [x] Función pura `capture_geometry(outer, inner) -> Option<(Pos2, Vec2)>`
      (módulo de `ws_frame.rs`): posición del `outer_rect.min` (fallback
      `inner_rect.min`), tamaño SIEMPRE del `inner_rect.size()`; ambos
      `None` → `None` (el llamador conserva el previo).
- [x] `StoredWorkspace.size` persistido = tamaño interior (coherente con
      `with_inner_size` y su doc).
- [x] Tests de tabla en `app/ws_frame_tests.rs` (`#[path]`): exterior+interior
      → pos exterior / tamaño interior; solo interior → ambos de él;
      ninguno → `None`; solo exterior → `None` (el tamaño nunca sale del
      exterior). 4 tests verdes.

## Checkpoint: Tasks 1-2
- [x] `cargo test --workspace` verde (473), clippy `-D warnings`, fmt OK.
- [ ] Manual Windows: conectar/desconectar segundo monitor sin brincos.

## Task 3: Verificación UI real en Windows
- [ ] Segunda ventana (menú, Ctrl+N, Ctrl+T) abre con el tamaño del padre y
      NO se redimensiona sola (10+ segundos sin tocarla).
- [ ] Redimensionado/arrastre manual queda como la dejó el usuario.
- [ ] Desconectar el monitor secundario: sin peleas de posición/tamaño;
      conmutador Ctrl+Tab y cierre de ventanas intactos.

## Checkpoint: Final
- [ ] `cargo test --workspace` verde, clippy `-D warnings`, fmt OK.
- [ ] Documentar el porqué (captura interior + geometría solo al nacimiento)
      en los comentarios de módulo/CLAUDE.md.