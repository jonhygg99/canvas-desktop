# Plan de implementación: exportación PNG de imágenes restauradas

## Descripción

Investigar y corregir el caso en que una imagen abierta desde un PNG con sidecar editable (`.canvas/`) se ve correctamente en el lienzo, pero su exportación PNG aparece blanca. El caso de referencia es `14.png`, cuyo PNG base contiene píxeles blancos y cuyo sidecar oculto contiene las capas visibles. La exportación debe representar el documento restaurado y sus píxeles, no únicamente el raster base.

## Decisiones de arquitectura

- Mantener la separación existente: `canvas-io` lee documentos y píxeles; `canvas-render` construye y hornea la escena; `canvas-app` coordina la exportación.
- Reproducir primero con el entry point real de exportación y un probe headless; no modificar el renderer por hipótesis.
- Usar el `ImageMap` del documento restaurado como fuente única de píxeles para PNG/JPEG.
- No borrar ni reescribir automáticamente el sidecar del usuario.
- Añadir pruebas de comportamiento con imágenes sintéticas: exportar un documento restaurado debe producir píxeles no blancos y conservar las capas; una imagen realmente blanca debe seguir exportándose blanca.

## Dependencias y flujo actual

```text
PNG + .canvas oculto
    -> canvas_io::open_document
    -> OpenOutcome::Restored
    -> EditorState::from_restored
    -> EditorState.doc + EditorState.images
    -> CanvasRenderer::bake_page
    -> export/save raster
```

El primer objetivo es verificar si los píxeles se pierden entre alguno de esos límites o si la exportación usa por error el PNG original.

## Tareas

### Fase 1: Reproducción y localización

#### Tarea 1: Probar la exportación real de `14.png`

**Descripción:** Ejecutar el entry point real que abre el documento restaurado y genera una salida PNG, comparando dimensiones, píxeles dominantes y contenido del sidecar con la salida.

**Criterios de aceptación:**
- [ ] El probe distingue `Flat` de `Restored` y registra el número de capas.
- [ ] La salida PNG se inspecciona con un decoder independiente.
- [ ] Se identifica el primer límite donde los píxeles visibles dejan de estar presentes.

**Verificación:**
- [ ] Ejecutar el ejemplo/probe de exportación existente o un probe temporal contra la ruta real.
- [ ] Comparar el resultado con `image::open` y estadísticas de píxeles.

**Dependencias:** Ninguna.

**Archivos probables:**
- `crates/canvas-io/src/load.rs`
- `crates/canvas-app/src/app/persistence.rs`
- `crates/canvas-render/src/lib.rs`
- `crates/canvas-io/src/export/`

**Alcance estimado:** Mediano.

#### Tarea 2: Aislar la fuente de píxeles usada por PNG

**Descripción:** Seguir la ruta de exportación raster y confirmar si usa `state.images`, `blur_overrides`, una imagen base en disco o un mapa de imágenes incompleto.

**Criterios de aceptación:**
- [ ] La fuente de cada capa exportada está identificada.
- [ ] Se comprueba que cada `LayerId` del documento tiene su entrada correspondiente en `ImageMap`.
- [ ] Se comprueba que el orden de capas y el cierre de clips/opacidades coincide con la vista.

**Verificación:**
- [ ] Test enfocado de exportación con dos capas de colores distintos.
- [ ] Probe headless con aserciones sobre el color de salida.

**Dependencias:** Tarea 1.

**Alcance estimado:** Mediano.

### Checkpoint: Reproducción

- [ ] El caso `14.png` está reproducido con el entry point real.
- [ ] Se conoce el límite exacto que pierde los píxeles.
- [ ] No se ha añadido todavía una defensa no justificada.

### Fase 2: Corrección mínima

#### Tarea 3: Corregir la pérdida demostrada de píxeles

**Descripción:** Modificar únicamente el límite responsable de que la exportación PNG ignore o pierda las capas restauradas, manteniendo intactas las rutas que ya funcionan.

**Criterios de aceptación:**
- [ ] La exportación de un documento restaurado incluye sus capas visibles.
- [ ] Una imagen base blanca con capas restauradas ya no produce un PNG blanco.
- [ ] Las imágenes sin sidecar y los diseños `.canvas` conservan su comportamiento.
- [ ] La corrección no depende de nombres, rutas de Google Drive ni del archivo `14.png`.

**Verificación:**
- [ ] Test de regresión con sidecar sintético y capas coloreadas.
- [ ] Exportación headless de PNG y comprobación de píxeles.
- [ ] `cargo test -p canvas-io` y `cargo test -p canvas-render`.

**Dependencias:** Tareas 1–2.

**Alcance estimado:** Mediano.

#### Tarea 4: Revisar la sincronización de texturas y `ImageMap`

**Descripción:** Si la causa está en el renderer, asegurar que cada capa visible se registra con los píxeles actuales y que los reemplazos/undo no dejan entradas obsoletas. Solo se tocará si el probe de la Tarea 2 lo demuestra.

**Criterios de aceptación:**
- [ ] Una nueva exportación después de cargar o reemplazar una imagen usa los píxeles actuales.
- [ ] El mapa no se consulta con un ID que no pertenece al documento exportado.
- [ ] Los efectos activos siguen aplicándose a la misma capa.

**Verificación:**
- [ ] Test de reemplazo seguido de exportación.
- [ ] Test de cambios de tamaño de textura si la ruta GPU está implicada.
- [ ] Tests GPU ignorados del renderer cuando corresponda.

**Dependencias:** Tarea 3, solo si resulta necesaria.

**Alcance estimado:** Grande; dividir si afecta a más de cinco archivos.

### Checkpoint: Corrección

- [ ] La salida PNG restaurada contiene contenido visible.
- [ ] El caso blanco genuino continúa siendo blanco.
- [ ] Undo/redo y efectos no regresan.
- [ ] El diff sigue limitado al flujo afectado.

### Fase 3: Simplificación y cierre

#### Tarea 5: Reducir tests y código a un contrato compacto

**Descripción:** Sustituir comprobaciones redundantes o acopladas por la menor colección clara de casos: sin sidecar, sidecar válido, sidecar desactualizado y exportación con capas visibles.

**Criterios de aceptación:**
- [ ] Los tests describen resultados observables, no detalles internos innecesarios.
- [ ] Los casos de frontera están cubiertos sin duplicar fixtures.
- [ ] Se elimina cualquier rama defensiva añadida durante la investigación que no sea necesaria.

**Verificación:**
- [ ] Suite enfocada completa.
- [ ] Revisión del diff y `cargo fmt`.

**Dependencias:** Tareas 3–4.

**Alcance estimado:** Pequeño.

#### Tarea 6: Validación final y UI manual

**Descripción:** Ejecutar build/test del workspace y comprobar manualmente que `14.png` se abre y se exporta con contenido visible.

**Criterios de aceptación:**
- [ ] `14.png` muestra contenido al abrirse.
- [ ] Exportar como PNG produce un archivo con los mismos elementos visibles del lienzo.
- [ ] Reabrir el PNG exportado no lo muestra blanco salvo que el documento sea realmente blanco.

**Verificación:**
- [ ] `cargo test --workspace --locked`.
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- [ ] `cargo fmt --all -- --check`.
- [ ] Prueba manual: abrir `14.png`, exportar PNG y comparar visualmente.

**Dependencias:** Tareas 1–5.

**Alcance estimado:** Mediano.

### Checkpoint: Completo

- [ ] El caso real `14.png` deja de exportarse en blanco.
- [ ] El contrato de sidecar/restauración/exportación tiene regresión automatizada.
- [ ] Todas las verificaciones pasan.
- [ ] El diff está listo para revisión.

## Riesgos y mitigaciones

| Riesgo | Impacto | Mitigación |
|---|---:|---|
| El PNG base realmente es blanco | Alto | Comparar siempre contra las capas del sidecar y no inventar píxeles.
| La exportación usa un `ImageMap` incompleto | Alto | Afirmar correspondencia `LayerId` ↔ píxeles antes de hornear.
| Texturas GPU obsoletas | Medio | Probar exportación tras reemplazo y cambios de tamaño; tocar GPU solo con reproducción.
| Sidecar desactualizado | Medio | Respetar `hash_matches`; no restaurar sidecars stale automáticamente.
| Cambios demasiado amplios | Medio | Mantener la corrección en el límite que pierda los píxeles y eliminar defensas no demostradas.

## Preguntas abiertas

- ¿La exportación PNG del caso real se ejecuta desde `bake_page`, desde `save`, o desde otro entry point?
- ¿El blanco aparece al exportar el documento restaurado completo o solo al exportar una capa individual?
- ¿La salida exportada conserva las dimensiones y los efectos del documento restaurado?
