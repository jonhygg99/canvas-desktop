# Plan: encuadrar una composición completa para Shorts

## Objetivo

Desde la edición de un diseño de Canvas Desktop en **1920 × 1080**, poder crear un framing **9:16** de toda la página para usar ese diseño como clip en Flashcut-Auto.

El documento editable sigue teniendo sus mismas capas, dimensiones y un único archivo `.canvas`. La herramienta muestra una vista vertical de la composición completa, permite moverla y escalarla dentro del marco, y produce dos archivos de salida:

1. Una imagen PNG aplanada de la página original en 1920 × 1080.
2. Un sidecar `.framing` v1 para ese PNG, con el encuadre 9:16. Flashcut-Auto puede leer el PNG como clip e inyectar el sidecar al plan al generarlo.

El PNG es el clip derivado que se entrega a Flashcut-Auto. No se crea, copia ni modifica otro documento `.canvas`. El sidecar no transforma el proyecto de Canvas ni los medios fuente.

Esta solución preserva íntegra la composición de origen para que el usuario decida el encuadre vertical: el preview usa la composición aplanada completa en primer plano y el mismo PNG ampliado y desenfocado como fondo. El resultado visual coincide con la operación de framing de Flashcut-Auto. No es reflujo automático de texto y capas para una composición editorial nueva.

## Flujo del usuario

1. Abrir y editar el diseño habitual de 1920 × 1080.
2. En la sección **Page** o en las acciones de exportación, elegir **Frame for Shorts (9:16)**.
3. Canvas genera una vista de la página compuesta, sin crear ni alterar archivos. Sobre ella muestra una ventana 9:16 con la composición entera centrada y fondo desenfocado, como en Flashcut-Auto.
4. Arrastrar para desplazar la composición; acercar o alejar con un deslizador, con pasos de deshacer por gesto. Preview del resultado vertical a medida que se ajusta.
5. Pulsar **Export for Flashcut-Auto**. Elegir carpeta y nombre; se sugiere la carpeta del proyecto y un nombre terminado en `-short.png`.
6. Canvas aplana la página actual en un PNG y escribe junto a ese PNG `.framing/<nombre-completo>.json`. Confirma ambos archivos y la ruta que Flashcut-Auto deberá usar como media.
7. El usuario agrega ese PNG a Flashcut-Auto. Al generar el plan, Flashcut-Auto carga el sidecar y aplica el encuadre.

## Gallery: vista normal y vista de framings

Gallery ofrecerá dos vistas seleccionables: **Normal** para ver los medios originales y **Framings** para ver sus encuadres 9:16 guardados. El usuario podrá alternar entre ambas sin duplicar ni reemplazar los medios originales. En la vista de Framings se podrá abrir un encuadre para previsualizarlo y editarlo; también habrá una acción para crear uno cuando falte. El menú contextual de cada medio, abierto con clic derecho, incluirá **Editar framing 9:16** o **Crear framing 9:16**, según su estado, para editar directamente ese elemento individual.

Un indicador discreto junto al nombre —icono de rectángulo vertical más etiqueta **9:16**— señalará que ya existe framing guardado. La misma acción de crear/editar se podrá abrir desde Gallery y desde el modo de framing del editor del canvas. Gallery encuadra un medio individual; el editor del canvas encuadra la página compuesta completa. Ambos reutilizan el contrato `.framing` de Flashcut-Auto, sin confundir el objetivo de cada encuadre.

Se podrá volver al diseño 1920 × 1080 en cualquier momento; los cambios visuales de framing y exportación no alteran el zoom ni los transforms del documento.

## Límite entre los dos programas

`.framing` está ligado a un **archivo de media** y ajusta ese archivo cuando Flashcut-Auto lo compone en un clip de salida 9:16. Por eso Canvas debe generar primero un PNG plano de la composición: aplicar el framing al archivo original de una de sus capas ignoraría las demás y los textos del diseño.

El PNG generado ya tiene todo el diseño aplanado; su sidecar contiene la posición/escala con que ese PNG de 1920 × 1080 se debe mostrar en el Short. No se toca el `.canvas` guardado ni el raster fuente que pueda acompañarlo. Los planes y renders existentes de Flashcut-Auto no cambian: el sidecar entra a un plan durante la generación; un plan explícito conserva su `clip.framing`.

**Consecuencia importante:** el framing no puede representar una adaptación vertical donde cada texto y capa se redistribuye por separado. Esta entrega coloca, mueve y escala la composición ya terminada como un todo. Un flujo de rediseño responsivo sería otra función y tendría que guardar otra disposición editable, sin duplicar el proyecto como archivo autónomo.

## Contrato de salida

PNG aplanado en 1920 × 1080; el sidecar registra el formato de salida vertical usado por Flashcut-Auto:

```
Shorts/mi-diseno.png
Shorts/.framing/mi-diseno.png.json
```

```json
{
  "schemaVersion": 1,
  "width": 1080,
  "height": 1920,
  "framing": {
    "xPct": 0.0,
    "yPct": 0.0,
    "scalePct": 100
  }
}
```

- Ruta y forma del JSON compatibles con `crates/app/src/framing.rs` de Flashcut-Auto; no crear un contrato nuevo.
- X/Y finitos en `-100..=100`; escala entre `50..=200`.
- La escala se mide desde cover y la vista conserva el fondo cover desenfocado, según `crates/renderer/src/ffmpeg/framing.rs`.
- Un PNG fuente no debe escribirse en la carpeta `.framing`; el sidecar es JSON y la carpeta va junto al PNG.
- Si el nombre de salida ya existe, solicitar otro nombre o una confirmación de sobrescritura; no alterar `.canvas` para resolver colisiones.
- Fallos al generar PNG/sidecar deben dejar resultado identificable y recuperable. La UI debe indicar si quedó incompleta la pareja para que no se importe por error.

## Diseño técnico

- **canvas-core:** geometría pura para encuadrar el PNG 1920 × 1080 en la salida 1080 × 1920; límites y validación compatibles con Flashcut-Auto.
- **canvas-render:** crear el preview GPU del conjunto de capas, con salida final igual al framing FFmpeg. Reutilizar el render de página donde resulte seguro, sin clonar el documento a otro `.canvas` ni convertir capas en un documento alternativo.
- **canvas-io:** generar los PNG de salida con el mismo camino atómico y protección de tamaño que los exports existentes; lectura/escritura validada del sidecar .framing v1.
- **canvas-app:** modo de preview temporal, controles y flujo de exportación; guardado asíncrono, ruta elegida explícitamente y estado claro de éxito/error.
- Vista/gestos de framing separados del viewport de edición. No tocar tamaño de página, posición de capas, crop, historial del diseño ni preview que vean otros documentos/ventanas.
- Usar la versión actual del documento activo para aplanar. Si tiene cambios sin guardar, el PNG debe incluirlos porque sale del estado en memoria; no se debe guardar automáticamente el documento fuente como parte de **Export for Flashcut-Auto**.
- Capa de vídeo dentro del diseño: incluir su poster/frame actualmente mostrado en el PNG. La exportación de esta fase es una imagen estática; render de movimiento, audio, transiciones o timeline queda fuera del alcance.
- El editor no vuelve a indexar toda la biblioteca. El único nuevo media es el PNG pedido por el usuario.

## Alcance

Primera versión: diseños 1920 × 1080, salida vertical 1080 × 1920, PNG aplanado + sidecar compatible. El preview y la exportación representan una sola página completa, todos sus elementos compuestos juntos.

Fuera de la primera versión: layout responsivo por capas, salida animada o .mp4, exportación de texto/capas separadas, importación o escritura de planes de producción, tocar sidecars de las imágenes fuente y batch de toda la biblioteca.

## Criterios de aceptación

- Abrir el modo conserva el diseño 1920 × 1080 y previsualiza **todas las capas visibles** como una composición vertical con primer plano y fondo desenfocado.
- Arrastrar y escalar cambia el framing del resultado compuesto, no el documento, las capas individuales ni los medios fuente.
- La pareja exportada es un PNG 1920 × 1080 y JSON .framing válido; el PNG contiene todas las ediciones actuales del lienzo.
- Flashcut-Auto acepta el sidecar y coloca esos valores en un plan nuevo. La salida renderizada del clip coincide con el preview del editor.
- PNG/JSON van a la ruta elegida. `.canvas` mantiene hash, tamaño y contenido; los medios fuente también conservan su hash.
- Cambiar de modo, cancelar el diálogo o fallar el render no deja cambios no deseados ni archivos parciales presentados como exportación correcta.
- Undo de gestos regresa el preview a los valores anteriores; Undo normal de capas continúa modificando solo el diseño.
- Errores de permisos, colisión de nombre, framing inválido o PNG demasiado grande son claros y no bloquean el editor.

## Verificación de implementación

Validación de geometría con fixtures independientes y la composición FFmpeg de Flashcut-Auto, incluyendo imágenes landscape 16:9, desplazamientos cerca de los límites y escala 50/100/200.

Verificar en una carpeta temporal que Canvas produce una pareja nueva, que Flashcut-Auto carga el JSON y que renderiza el PNG como 9:16. Comparar primer plano y composición final; tolerancia de 2 píxeles después de contabilizar redondeo de dimensiones pares y documentar diferencias de blur si las hay. Revisar por hash que el .canvas abierto y sus medios fuente no cambiaron.

Durante implementación, probar en la app real una página con varios tipos de capa, transparencia, fondo y una capa de vídeo pausada. Comprobar cerrar/cancelar, exportar de nuevo con nombre distinto y abrir el archivo resultante en Flashcut-Auto.

Gates finales del repositorio:

```powershell
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p canvas-app -- "<carpeta-de-prueba-copiada>"
```

Validar Flashcut-Auto con sus crates de núcleo/CLI y el límite actual de toolchain consignado en su `AGENTS.md`; no cambiar planes de producción.

## Tareas

Lista y criterios de implementación en `todo.md`.

## Riesgos y mitigaciones

| Riesgo | Efecto | Mitigación |
|---|---|---|
| Tratar la página de 1920 × 1080 como si fuera un media individual | Se pierde composición, texto y capas | Aplanar la página completa en el mismo renderer y adjuntar el sidecar al PNG plano |
| Hornear al raster original al exportar | Puede sobrescribir la fotografía base y las capas editables | Export PNG a un destino nuevo, separado del archivo fuente |
| Duplicar el documento .canvas | Dificulta saber cuál es la versión de trabajo | Mantener un solo .canvas; la única derivación es el PNG de media solicitado |
| El preview no coincide con FFmpeg | El Short cambia tras importar | Compartir geometría, comparar salida compuesta y probar redondeo, blur y color |
| Render o sidecar fallido parcialmente | Se importa una pareja incompleta | Estado por etapas, errores visibles y archivos temporales hasta finalizar la operación |
| Diseñar cada capa como vertical sin modelo de layout | Expectativas falsas y edición compleja | Dejar claro que esta versión encuadra la composición aplanada; layout vertical por capa es otro objetivo |

## Referencias locales revisadas

- Canvas Desktop: `CLAUDE.md`; editor de página `crates/canvas-app/src/editor/properties_panel/page.rs`; CanvasRenderer y composición de escena descritos en CLAUDE; guardado/export `crates/canvas-app/src/app/views/editor/save_flow.rs` y `crates/canvas-io/src/export/`; formato `.canvas` en `crates/canvas-io/src/sidecar/`.
- Flashcut-Auto: `crates/app/src/framing.rs` (formato/lectura del sidecar); `crates/domain/src/validate/framing.rs` (rangos); `crates/renderer/src/ffmpeg/framing.rs` (render vertical y blur).

## Estado de la entrega (4 de octubre de 2026)

Implementaci?n repartida en incrementos de geometría, persistencia, editor, Gallery y verificación. **Save framing** conserva la configuración del diseño en `.framing/<nombre-completo-del-canvas>.json`; **Export for Flashcut-Auto** entrega el PNG plano con su propio sidecar. El icono de rect?ngulo vertical aparece antes del nombre en Gallery, propiedades y cabecera del lienzo cuando existe un framing válido.

La composición se captura del estado en memoria al entrar en Framing 9:16 usando el mismo renderer de página que la exportación. Durante este modo se ajusta el encuadre y su historial; para modificar capas se vuelve a Normal view y se entra de nuevo, capturando la composición actualizada. El documento editable conserva dimensiones, capas y viewport. El PNG es estático, incluidos los frames de vídeo visibles.

Gallery reutiliza la miniatura/poster del medio y el preview incrustado del `.canvas`; la exportación desde el editor usa la composición a resolución completa. Lecturas, blur y escritura se ejecutan fuera del hilo de UI; la captura GPU completa se realiza al entrar. Cada sesión tiene su canal de resultados y Gallery descarta previews obsoletos mediante generaciones, limita trabajos concurrentes a cuatro y conserva hasta 64 previews con texturas.

Validación realizada:

- Tests de geometría 50/100/200 %, validación JSON, conservación de originales, fallos y colisiones de exportación, undo por gesto, sesiones aisladas, resultados obsoletos, filtros, clic real de egui en Save framing y bot?n del sidebar.
- Suite completa del workspace, Clippy con `-D warnings` y formato.
- Fixture GPU real de 1920 × 1080 con imagen, texto y forma agrupados, recorte, grayscale, blur, transparencia, capa oculta y poster de vídeo: PNG y sidecars sin alterar el `.canvas`. Los 17 tests GPU también pasan, incluidos scopes de documentos distintos y frames de vídeo cambiantes.
- App nativa de Windows: vistas Normal/Framings, indicador, men? contextual, apertura, arrastre y guardado de un framing individual; PNG original conservado por hash.
- CLI de Flashcut-Auto: `framing list` reconoce el sidecar, `framing apply` lo incorpora y `render` genera un MP4 real de 1080 × 1920 de la composición multicapas. Un framing expl?cito en el plan se conserva aunque el sidecar tenga otro valor. Los hashes del `.canvas` y del PNG no cambian.

La matriz de 27 combinaciones (escalas 50/100/200 % y X/Y -100/0/100) pasa contra FFmpeg: límites de la composición dentro de dos píxeles y coordenadas muestreadas de la fuente correctas. El rect?ngulo de referencia del preview y el MP4 real de Auto también coinciden dentro de dos píxeles. La comparación raster presenta una diferencia media absoluta RGB de (15,92; 7,81; 10,21) sobre 255: hay diferencias de blur reducido, interpolaci?n y conversión de color de MP4. Se conserva el color del PNG exportado; no se promete identidad de píxeles con el MP4.

El aislamiento se comprueba tanto en el renderer (scopes distintos con los mismos IDs de capa) como en la UI (canales por sesión; respuestas tard?as tras cerrar un documento no alcanzan otro). La comprobación nativa de Windows se realiz? en una instancia de prueba separada, sin cerrar la instancia del usuario ni tocar su material.

Validación reproducible (Python con Pillow y FFmpeg en PATH):

```powershell
cargo run -p canvas-render --example framing_probe -- target/framing-probe
cargo test -p canvas-render --test gpu_bake -- --ignored --test-threads=1
python crates/canvas-render/examples/framing_probe/verify_ffmpeg.py target/framing-probe C:/Users/jonhy/Documents/code-projects/Flashcut-Auto/target/debug/cli.exe
```

El script crea una subcarpeta nueva para cada validación y conserva PNG de referencia, frame de Auto, MP4, planes y manifiesto. No se modificaron planes de producci?n.

## Vídeos individuales desde Gallery

Al abrir Create/Edit framing sobre un vídeo, el diálogo muestra Play/Pause, Restart, barra de tiempo y tiempo actual/duración original. **Trim original video** permite ajustar In y Out y restablecer el intervalo completo. El vídeo seleccionado conserva sus bytes: el trim selecciona un intervalo de su reproducción, no reescribe el archivo fuente ni crea otro vídeo. **Save framing** guarda posición, escala e intervalo juntos con escritura atómica; al reabrir se recupera el intervalo y el playhead comienza en In. Play se detiene en Out y vuelve a In al reiniciarse después de terminar.

El vídeo y el fondo desenfocado usan el mismo fotograma, publicado junto con su blur por un worker con mailbox de un frame. El decoder reutiliza `canvas_io::VideoFrameStream`, limita el preview a un máximo de 960 píxeles y se cancela al pausar, buscar o cerrar. Las texturas se actualizan en su sitio. El audio de preview empieza silenciado y tiene un sink/dispositivo propio de la sesión; los fallos de audio se muestran sin bloquear el vídeo. Los controles se desplazan verticalmente si no caben.

La extensión opcional del sidecar v1 es `"trim": { "startS": 1.0, "endS": 4.0 }`. Los sidecars anteriores siguen siendo válidos. Actualizar solo el framing conserva un trim existente. Flashcut-Auto acepta el sidecar extendido e importa el encuadre; **su consumidor actual no aplica el trim de Canvas**. No se añadió exportación de un clip nuevo, según la aclaración del usuario.

El framing de la composición completa del editor continúa siendo una captura estática; reproducir el vídeo individual de Gallery no sustituye las demás capas del diseño. Hay una prueba específica para esta separación.

Validación: tests del workspace, tests de vídeo con FFmpeg real (Play/Pause mediante eventos de egui en el diálogo de Gallery, seek, fondo, trim, guardado/reapertura, EOF y bytes originales), Clippy y formato. Se inició la app nativa en una carpeta de prueba con vídeo y audio sintéticos. Los tests que necesitan FFmpeg se ejecutan explícitamente para no exigir ese binario en CI:

```powershell
cargo test -p canvas-app framing::video -- --include-ignored
```
