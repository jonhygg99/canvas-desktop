# Tareas: framing de una página Canvas completa para Shorts

Plan de diseño en `plan.md`. Las tareas se implementan por orden y dejan intacto el .canvas y los medios originales. Un export aprobado crea un PNG plano de la composición y su .framing de compatibilidad.

## 1. Geometría del PNG landscape dentro de la salida portrait

**Descripción:** Implementar la geometría pura, validación y cuantización según el consumidor Flashcut-Auto, separada del tamaño de página Canvas.
**Aceptación:**
- [x] Framing serializable en los rangos que valida Auto y destino 1080 × 1920.
- [x] Entrada 1920 × 1080, centrada y límites desplazados, escala 50/100/200: la misma ubicación y límites que FFmpeg.
- [x] No cambian las dimensiones 1920 × 1080 del documento.
**Verificación:** Tests puros; fixtures de entrada/salida del FFmpeg real.
**Dependencias:** Ninguna.
**Archivos previstos:** `canvas-core/src/framing.rs` (nuevo), `canvas-core/src/framing_tests.rs` (nuevo), `canvas-core/src/lib.rs`.
**Tamaño:** S.

## 2. Crear sidecar .framing v1

**Descripción:** Implementar lectura/validación y escritura atómica del contrato que Flashcut-Auto ya consume.
**Aceptación:**
- [x] Path `.framing/<nombre-completo>.json` junto al PNG.
- [x] Faltante, inválido, dimensiones distintas y versión incorrecta se distinguen sin bloquear el documento.
- [x] Los writes fallidos no corrompen un sidecar existente ni tocan el .canvas.
**Verificación:** Ida/vuelta JSON, campos desconocidos y fallo de escritura en directorio temporal; probar loader de Auto.
**Dependencias:** 1.
**Archivos previstos:** `canvas-io/src/framing.rs` (nuevo), `canvas-io/src/framing_tests.rs` (nuevo), `canvas-io/src/lib.rs`, reutilizar utilidades atómicas existentes.
**Tamaño:** M.

## Checkpoint: contrato

- [x] Geometría comprobada frente a FFmpeg.
- [x] Loader de Flashcut-Auto acepta sidecar hecho por Canvas.
- [x] La salida es un PNG plano junto a un JSON; nunca se serializa otra copia del diseño .canvas.

## 3. Render de la página completa para el preview

**Descripción:** Tomar como entrada todas las capas y el fondo de la página editada y formar una textura de composición para aplicar un framing 9:16 en preview.
**Aceptación:**
- [x] Imagen, texto, formas, grupos, recortes, efectos, transparencia y poster del vídeo visible aparecen en su orden de composición.
- [x] Preview captura los cambios no guardados en RAM al entrar; volver a Normal y entrar de nuevo actualiza la composici?n, sin guardar ni rasterizar los archivos originales.
- [x] No clona `Document` para crear un segundo documento; reutiliza o extiende el pipeline de escena/surface.
**Verificación:** App real con fixture de varias capas, comparar render de composición habitual con el asset del preview.
**Dependencias:** 1.
**Archivos previstos:** `canvas-render/src/scene/` (adapter pequeño si se requiere), `canvas-render/src/` tests del módulo elegido, `canvas-app/src/editor/canvas/paint.rs`, nuevo `canvas-app/src/editor/framing_preview.rs`.
**Tamaño:** M.

## 4. Mostrar 9:16 sin reemplazar el editor 16:9

**Descripción:** Añadir la acción **Frame for Shorts** desde la sección Page o barra del editor y presentar un marco/ventana 9:16 con la composición y el blur de fondo.
**Aceptación:**
- [x] El editor original permanece en 1920 × 1080 en todo momento.
- [x] El preview refleja todas las capas visibles a la misma resolución que el futuro PNG.
- [x] Cerrar o cancelar vuelve al editor sin modificar su página, capas, viewport o .canvas.
**Verificación:** Uso de la app real: entrar, previsualizar, mover/zoom, cancelar y comprobar el documento.
**Dependencias:** 3.
**Archivos previstos:** `canvas-app/src/editor/framing_preview.rs`, `canvas-app/src/editor/properties_panel/page.rs`, `canvas-app/src/editor/canvas/mod.rs`, `canvas-app/src/editor/canvas/paint.rs`.
**Tamaño:** M.

## 5. Interacción y undo del framing global

**Descripción:** Arrastre/desplazamiento, escala, centro/restablecer y undo por gesto; los valores aplican a la composición completa.
**Aceptación:**
- [x] Un drag continuo es una sola operación de undo.
- [x] La página, transform/crop de cada layer y media source quedan intactos.
- [x] límites X/Y y escala muestran los mismos resultados en la ventana y en las pruebas.
**Verificación:** Tests de geometría inversa/gestos y uso con mouse/teclado en el editor.
**Dependencias:** 1, 4.
**Archivos previstos:** `canvas-app/src/editor/framing_preview.rs`, nuevo `canvas-app/src/editor/framing_geometry.rs`, `canvas-app/src/editor/properties_panel/page.rs`, atajos solo si Ctrl+Z entra en conflicto con el historial del documento.
**Tamaño:** M.

## 6. Exportar el PNG plano sin copiar el .canvas

**Descripción:** Añadir **Export for Flashcut-Auto**: seleccionar un destino, aplanar la página actual en 1920 × 1080 y preparar un PNG que sirva de media en Auto.
**Aceptación:**
- [x] PNG muestra todas las ediciones actuales del diseño en memoria.
- [x] Nunca sobrescribe el raster original ni el .canvas y respeta colisiones/nombre de salida.
- [x] Cancelar, fallar, PNG vacío o falta de memoria deja un error claro y ninguna salida marcada como completa.
**Verificación:** Exportar fixture con cambios no guardados; comprobar capas/píxeles y hashes del .canvas y fuentes.
**Dependencias:** 3, 4.
**Archivos previstos:** `canvas-app/src/editor/framing_export.rs` (nuevo), `canvas-app/src/app/views/editor/file_ops.rs` o acción nueva según el dispatcher, `canvas-app/src/loader/export_ops.rs`, `canvas-io/src/export/`, `canvas-io/src/save.rs`.
**Tamaño:** M.

## 7. Guardar el sidecar junto al PNG y enlazar las operaciones

**Descripción:** Al confirmar export, escribir el sidecar del PNG con el valor mostrado, mantener ambos estados atómicos/localizables y confirmar dónde abrir/importar el resultado.
**Aceptación:**
- [x] PNG landscape más sidecar 1080 × 1920 son aceptados por Flashcut-Auto.
- [x] El sidecar usa el nombre del PNG exacto y la geometría de la última revisión confirmada.
- [x] Repetir export con otro destino/nombre no toca archivos previos; conflicto requiere resolución clara.
**Verificación:** Abrir pareja exportada con `framing list` o API del Auto y renderizar el clip del plan de prueba.
**Dependencias:** 2, 5, 6.
**Archivos previstos:** `canvas-app/src/editor/framing_export.rs`, `canvas-app/src/app/messages/` mensaje de resultado, `canvas-app/src/loader/` worker, `canvas-io/src/framing.rs`, `canvas-io/src/lib.rs`.
**Tamaño:** M.

## Checkpoint: primera composición

- [x] Abrir .canvas 1920 × 1080; preview completo; mover; exportar PNG + .framing.
- [x] Inyectar sidecar en un plan de prueba de Flashcut-Auto y renderizar.
- [x] Confirmar que .canvas y fuentes no cambian.
- [x] Comparar el render del Auto con el preview de Canvas antes de ampliar el alcance.

## 8. Cuidar cancelaciones y la sesión de exportación

**Descripción:** Operaciones de render por revisión, identidad de ventana y ruta; limpiar temporales y evitar que un resultado de export viejo aparezca asociado a otro documento.
**Aceptación:**
- [x] Render cancelado/navegación no cambia el archivo actual ni reemplaza una exportación confirmada.
- [x] La UI distingue renderizando, listo para exportar, escritura, terminado y error.
- [x] Guardar el .canvas, undo o redimensionar página durante el preview actualiza/descarta correctamente el resultado cacheado.
**Verificación:** Pruebas de respuesta asíncrona obsoleta, export cancelado y guardar otra revisión; dos ventanas.
**Dependencias:** 4, 6, 7.
**Archivos previstos:** `canvas-app/src/editor/framing_export.rs`, nuevo `canvas-app/src/app/messages/framing.rs`, `canvas-app/src/app/messages/mod.rs`, `canvas-app/src/app/workspace.rs`.
**Tamaño:** M.

## 9. Paridad visual y documento del flujo

**Descripción:** Verificar todas las capas de entrada, orientación/perfiles del PNG, escalas y operación completa desde el lienzo hasta Flashcut-Auto.
**Aceptación:**
- [x] Encuadre del PNG coincide con FFmpeg; blur comparado en output real y diferencias se describen.
- [x] Flashcut-Auto lee los sidecars y el plan resultante preserva planes con valor explícito.
- [x] UI explica con una frase que el PNG es un asset derivado y el .canvas sigue siendo el proyecto editable.
**Verificación:** Ejecutar app real y render de Auto en carpeta temporal; `cargo test`, clippy `-D warnings`, fmt.
**Dependencias:** 1–8.
**Archivos previstos:** probe de framing en `canvas-render/examples/`, pruebas de export/render del módulo correspondiente, `CLAUDE.md`.
**Tamaño:** M.

## 10. Ver y administrar framings desde Gallery

**Descripción:** Añadir en Gallery un selector entre la vista normal de medios y la vista de framings 9:16. Desde Gallery se pueden crear, previsualizar y editar los framings guardados, usando el mismo editor y sidecar que el modo de framing del canvas.
**Aceptación:**
- [x] Gallery permite alternar claramente entre **Normal** y **Framings** sin duplicar ni reemplazar el medio original.
- [x] En **Framings** se muestran los encuadres guardados; los elementos sin encuadre permiten crear uno.
- [x] Un encuadre existente se puede previsualizar y editar desde Gallery.
- [x] Clic derecho sobre un medio abre un menú contextual con **Editar framing 9:16** si existe, o **Crear framing 9:16** si falta; la acción se aplica al elemento individual seleccionado.
- [x] Icono de rectángulo vertical más etiqueta **9:16** junto al nombre cuando el framing está guardado; sin indicador cuando falta.
- [x] Gallery encuadra el medio individual; el editor encuadra la composición completa del canvas. Ambos leen/escriben el formato compatible `.framing`.
- [x] Crear, cambiar de vista o cancelar no modifica los medios originales.
**Verificación:** Probar ambas vistas con medios con/sin sidecar; crear y editar desde Gallery, comprobar la salida en Flashcut-Auto y verificar por hash que los originales no cambian.
**Dependencias:** 2, 4, 5, 7.
**Archivos previstos:** `canvas-app/src/` módulos de Gallery y framing existentes; inspeccionar arquitectura antes de fijar rutas concretas.
**Tamaño:** M.

## Checkpoint: final

- [x] Todos los criterios del plan cumplidos y función probada en Windows.
- [x] Solo el PNG exportado y su .framing se agregan a la carpeta elegida.
- [x] El usuario puede abrir el PNG como clip de imagen y los valores se respetan en el Short.
- [x] Calidad, clippy, fmt, docs y revisión completos.

## Evidencia de cierre

Las diez tareas est?n entregadas y verificadas. Ver `plan.md`, secci?n ?Estado de la entrega?, para el comportamiento final, fixture multicapas, 27 casos FFmpeg, aislamiento entre documentos y render real de Flashcut-Auto. Las casillas de render multicapas y comparaci?n visual se cerraron tras ampliar y ejecutar el fixture GPU y comparar con el MP4 real.

El blur del preview se calcula a resoluci?n reducida; la geometr?a y el contrato portable est?n verificados, pero no se promete identidad de p?xeles del desenfoque de FFmpeg. El test GPU est? en `crates/canvas-render/examples/framing_probe.rs` y sus salidas de prueba en `target/framing-probe`, fuera del contenido del usuario.
