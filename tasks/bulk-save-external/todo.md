# Tareas: "changed on disk" + Back muerto + lienzo lento (main)

Plan de diseño en `plan.md`. Base: main v0.7.9, rama `fix/bulk-first-blank`.
Sin tocar `tasks/plan.md`, `tasks/todo.md` (framing, completos), los planes
raíz `plan-*.md` ni `tasks/archive/window-geometry-*` (otra sesión).

## Diagnose

- [x] Trazas en watcher (`on_source_changed_on_disk`), hash del sidecar
      (`build_slot_doc`), `on_saved` (OK/fallo + historial), `request_nav`
      (sucios, diálogo en vuelo) y `saved_exit_ready` (motivo)
- [x] `should_flag_external_change` pura + tabla de 4 casos
- [ ] Repro con log en la app real (usuario)

## Fixes

- [x] `on_saved` OK limpia `external_change` obsoleto (el disco tiene lo recién escrito)
- [x] `nav_block_reason` + `park_notice` puros; `park_nav` deja el motivo en `save_error` sin pisar errores
- [x] Con diálogo en vuelo: repinta + traza (antes `return` mudo)
- [x] `saved_exit_ready` ya mostraba el motivo; ahora también lo traza
- [ ] Manual: bulk→editar→Ctrl+S sin banner; toque externo CON banner;
      Back limpio / sucio / guardando / Save-all pendiente

## Rendimiento (medido)

Sonda: `cargo run -p canvas-render --example fx_hotpath_probe`
(foto sintética 4032×3024 = 46,5 MB RGBA, 1 capa sin efectos, 60 frames):

```
primer frame (con copia reducida): 111.26 ms
frame en reposo (60 frames):         0.000 ms/frame
codificar escena (60 frames):        0.001 ms/frame
copia reducida suelta:             103.03 ms (2048×1536)
copias reducidas materializadas:         1
```

- Antes: `sync_display` copiaba el RGBA completo (`to_vec()`) y lo reducía
  con `imageops::thumbnail` en CADA frame y por capa grande, y tiraba el
  resultado → ~103 ms por capa y frame (con 3 lienzos visibles, ~0,3 s/frame).
- Después: la copia se materializa UNA vez (`display_rebuilds == 1`); los
  frames en reposo no copian píxeles (0.000 ms).
- Task 8 (cachear la escena de los vecinos): **no se implementa** —
  codificar la escena cuesta 0.001 ms/frame, ruido frente a los 103 ms de la
  copia. Decisión y medición aquí, sin código nuevo.

- [x] `DisplayPlan`/`display_plan`/`exceeds_fx_dim` + tests (tabla Keep/Rebuild/Drop)
- [x] `sync_display` decide por `Blob::id()` antes de tocar píxeles
- [x] `CanvasRenderer::display_rebuilds()` como contador de regresión
App real (`CANVAS_DEBUG_PERF=1`, PNG sintético 3000×2000 en `%TEMP%`):

```
perf lienzo: frame pintado frame=1 total_ms=82.15 encode_ms=75.86 render_ms=6.29 \
             visible_slots=1 layers_synced=1 display_rebuilds=1 display_rebuilds_total=1
```

La copia reducida se paga en el primer frame (una vez) y el frame se pinta;
antes del fix cada frame habría vuelto a copiar los 24 MB de la imagen.

- [x] `CanvasSurface` cachea el `TextureView` (uno por tamaño, no por frame)
- [x] Presupuestos de RAM del render cacheados 1 s (`surface::cached_render_budget`
      + `deck::ram_probe_due`); el gate de guardado sigue midiendo en vivo
- [x] Sonda `CANVAS_DEBUG_PERF=1` (`perf_probe.rs`): frame, encode, render,
      slots, capas y `display_rebuilds` cada 60 frames
- [x] `cargo test -p canvas-render --test gpu_bake -- --ignored` → 19/19
      (blur, scopes, presión de atlas, save_roundtrip, resize bajo el mismo id)
- [ ] Manual: foto de 12 MP, paneo/zoom/arrastre con `CANVAS_DEBUG_PERF=1`
      (el arranque con una foto grande ya está verificado arriba)

## Verificación automática

- [x] `cargo test -p canvas-render --lib` → 27/27
- [x] `cargo test -p canvas-app` → 619/619 (6 ignorados)
- [x] `cargo clippy --workspace --all-targets -- -D warnings` → limpio
- [x] `cargo fmt --all -- --check` → limpio
