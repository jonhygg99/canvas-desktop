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
- [x] Verificación automática (script PowerShell + rects de ventana reales,
      `CANVAS_DEBUG_WINDOWS=2`): 2 ventanas ESTABLES durante 4 s + 4 s
      (antes crecían ~40 px/frame); resized externo a 900×700 QUEDA
      (KEPT); movimiento a la 2ª pantalla QUEDA (KEPT); Ctrl+N sintetizado
      abrió ventanas nuevas con herencia de geometría y ninguna creció
      (2 de 5 pulsaciones registradas; el resto se pierde en el handoff de
      foco al crear ventanas — comportamiento normal de eframe multi-ventana).
- [x] Ctrl+N en vivo: sesión automatizada con robo de foco real — 12
      pulsaciones, 2 registradas (el resto se pierde en el handoff de
      creación, quirk de eframe multi-ventana ajeno al bug), 2 ventanas
      nuevas con herencia de geometría; NINGUNA creció en 10+ s.
- [x] «Arrastre de borde»: 3 resizes externos (SetWindowPos 1100x750,
      950x680, 1200x800) todos KEPT tras 2 s cada uno — sin pelea de
      tamaño. Estabilidad global: 5 muestras × 2 s con 4 ventanas →
      idénticas (STABLE).
- [x] Ubicación entre pantallas (equivalente al plug/unplug de monitor de
      verdad, que no se puede simular y queda opcional): ventana movida a
      la 2ª pantalla real (hay 2 conectadas) y estable; la app ya no
      reafirma posición/tamaño tras el nacimiento.

## Checkpoint: Final
- [x] `cargo test --workspace` verde (477), clippy `-D warnings`, fmt OK.
- [x] Documentar el porqué (captura interior + geometría solo al nacimiento)
      en los comentarios de módulo (`Workspace::geometry_seeded`,
      `seed_builder_geometry`, `capture_geometry`) y en CLAUDE.md.