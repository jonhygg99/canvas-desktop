# Download: vídeos repetidos, nombres y guardado

## Alcance

Diagnóstico del flujo `Edit video → Create canvas(s)` y `Insert on canvas`,
creación de ranuras, nombres visibles, Ctrl+S, Save as, Save all y respuestas
asíncronas. No se modifica código de producto ni medios del usuario.

Regla: varios lienzos pueden leer el mismo vídeo, pero no deben compartir
identidad ni destino de escritura por accidente. El original de Download no
se renombra, sobrescribe ni elimina como consecuencia de guardar un lienzo.

## Hallazgos

### 1. El nombre del origen se confunde con el del lienzo

- `build_video_canvas` fija `Document::source_path` al MP4 de Download.
- `EditorState::file_name` usa esa ruta para el título, toolbar, avisos y
  sugerencias de guardado; todos los recortes del mismo clip dan el mismo nombre.
- `stash_and_push` deduplica `Slot::path/name` solo si hay carpeta existente.
  El título del documento no usa ese nombre deduplicado antes del primer save.
- Sin carpeta, la ranura conserva una ruta `Untitled N.ext`, pero se sustituye
  su etiqueta por el nombre del clip. Inserciones sucesivas de un solo recorte
  repiten esa etiqueta y la sugerencia `<origen>-trim.mp4`.
- El lápiz del toolbar lanza un renombrado sobre `doc.source_path`: antes del
  primer save puede renombrar el MP4 temporal compartido, rompiendo los enlaces
  de otros recortes y la tarjeta de Download.

No cambiar la ruta de `VideoContent` solo para corregir una etiqueta.

### 2. La «reserva» del destino no es una reserva atómica

`reserve_clip_slot_path` comprueba disco y ranuras del Deck actual, pero no
crea una reserva. Dos ventanas sobre la misma carpeta pueden elegir la misma
ruta antes de guardar. Un archivo creado externamente después también puede
ser sustituido sin confirmación. Al agotar 10.000 candidatos devuelve el
nombre original, aunque esté ocupado, en vez de un error.

`export_video_composition` utiliza `<stem>.exporting` fijo y sustituye el
destino al finalizar. Dos renders al mismo destino comparten el temporal;
además `clip.mp4` y `clip.mov` comparten `clip.exporting`.

La deduplicación dentro de una tanda y un solo Deck ya existe: no rehacerla.

### 3. Formato Canvas sin carpeta: riesgo de sobrescribir el vídeo

`build_video_canvas(..., Canvas)` deja `is_design=true` y `source_path=clip.mp4`.
Solo `stash_and_push` CON carpeta corrige esa ruta a `.canvas`. Sin carpeta,
Ctrl+S entra en `start_save_design` con el MP4 temporal como destino;
`write_design` escribe el contenedor sin exigir extensión `.canvas`.
Resultado posible: un diseño Canvas ocupa el nombre del vídeo original.

Además, el `.canvas` guardado conserva enlaces de vídeo al staging. Los blobs
embeben pósters PNG, no el MP4: borrar el clip en Download o la limpieza de
staging deja el diseño guardado sin fuente reproducible.

### 4. Insertar puede saltarse la protección de un guardado en curso

`deck::apply_jump` exige `state.is_idle()`, pero `try_append_video_canvases`
extrae y sustituye el documento activo sin ese guard. `request_nav` tampoco
aplaza por `saving`; si se acepta Discard durante un render, puede llegar aquí.

`VideoCompositionRendered` lleva destino y resultado, no generación/id de
ranura. Su handler procesa el documento que esté activo al recibirlo. El
helper de adopción valida un id, pero el handler obtiene el id ACTUAL, no el
que inició el render. Existe también un fallback que adopta el destino aun
cuando el helper rechazó la adopción. Puede mutar/marcar como guardado otro
recorte si se permitió el cambio de activo.

### 5. El cierre del guardado de vídeo difiere del guardado normal

- `on_saved` avanza `save_all_queue`; `on_video_composition_rendered` no.
  El conductor elimina después las entradas limpias, pero una salida diferida
  puede quedarse sin ejecutar al vaciarse la cola fuera del callback.
- El callback de vídeo no renueva watcher/ventana de gracia ni solicita la
  miniatura que sí actualiza el guardado normal.
- Reenlaza y resetea trim/geometría ANTES de validar `saving_capture`:
  detecta ediciones nuevas después de haber mutado esos valores. El aviso de
  cambios pendientes no basta para conservar un trim/transform editado durante
  el render.
- `reset_rendered_trim` pone el vídeo a página completa y conserva los píxeles
  anteriores en memoria. Hasta recibir un fotograma del render, la imagen
  anterior puede verse estirada. No atribuir a este punto el crash GPU del log
  sin una reproducción específica.

## Plan mínimo, por prioridad

### P0 — Proteger medios y cerrar las carreras de identidad

1. Corregir el destino de diseños Canvas en TODOS los caminos de creación,
   con/sin carpeta y uno/varios recortes. Sin destino de diseño válido, pedir
   Save as; rechazar escribir un contenedor Canvas sobre un archivo de vídeo
   también en el límite de escritura.
2. Aplazar la inserción mientras hay save/export o una edición no consolidada;
   no descartar la petición. Reutilizar la navegación diferida existente.
3. Etiquetar el trabajo de render y su respuesta con generación + id estable
   de ranura + revisión capturada. Una respuesta obsoleta no modifica otro
   documento. No adoptar un resultado rechazado usando el activo como fallback.

### P1 — Un nombre por lienzo, un destino por escritura

4. Mantener el vídeo como fuente de lectura, y usar el nombre de la ranura para
   representar el lienzo y sugerir su destino en UI. Deduplicar también en
   sesiones sin carpeta y entre inserciones sucesivas, no solo dentro de una
   tanda. Conservar el esquema de sufijos existente, sin otra configuración.
5. Renombrar una provisional en memoria; no renombrar su fuente de Download.
   Para archivos ya guardados, conservar la operación de renombrado actual.
6. Reservar el destino al primer save con `create_new`, reutilizando los
   helpers de canvas-io y el worker existente. Saltar colisiones y comunicar
   el destino obtenido a la ranura correcta. No llenar el proyecto de MP4
   vacíos al insertar. Usar un temporal único en el directorio de destino,
   mantener la sustitución atómica y limpiar únicamente reservas propias
   tras fallo/cancelación. Agotamiento de nombres = error, nunca sobrescritura.

### P1 — Finalización coherente

7. Aplicar al render las mismas garantías de Saved: avance de Save all,
   ejecución de salida diferida solo al guardar todos, watcher y miniaturas.
   Compartir solo la pequeña parte común necesaria, no rediseñar SaveFlow.
8. Validar la revisión antes de mutaciones de adopción/reset. Si se editó
   durante el render, preservar esas ediciones y mantener el documento sucio.
   Mantener la preview coherente con el archivo adoptado antes de reanudar.

### P2 — Fuente persistente para diseños guardados

9. Evitar que un `.canvas` dado por guardado dependa exclusivamente de staging:
   conservar una fuente de vídeo duradera fuera de la carpeta temporal y
   reenlazar el diseño. Antes de elegir su ubicación, respetar la regla actual
   de no copiar el MP4 bruto al proyecto; si requiere cambiar esa regla,
   confirmar el contrato con el usuario. No basta con quitar Clear ni con
   ignorar el error de fuente ausente.

## Archivos y llamadores afectados

- `app/video_canvases.rs`: ambos constructores, append, naming y adopción.
- `editor/state/mod.rs`, `deck/model.rs`: solo si hace falta transportar el
  nombre/destino; mantenerlo con el SlotDoc al navegar.
- `layers_panel/mod.rs`, `ytdlp/edit.rs`, `ytdlp/trim_collection.rs`:
  llamadores de inserción simple y múltiple; preservar los trims.
- `app/navigation.rs`, `app/views/editor/save_flow.rs`, `file_ops.rs`:
  guards, elección de destino y rename provisional.
- `app/workspace.rs`, `app/window.rs`, toolbar y propiedades:
  etiquetas, avisos y sugerencias sin tocar la fuente de vídeo.
- `loader/mod.rs`, `loader/save_ops.rs`, `app/messages/save.rs`:
  contrato de respuesta y finalización sobre el dueño correcto.
- `canvas-io/src/video.rs`, helpers de rutas y escritura de diseños:
  temporales únicos, reserva real y validación de formato.
- Pruebas existentes de video_canvases, mensajes, persistencia, deck y
  `canvas-io/tests/video.rs`. Fixtures sintéticos con FFmpeg/tempfile;
  sin usar ni modificar vídeos reales del usuario. Sin dependencias nuevas
  ni cambio de esquema de sidecar salvo necesidad demostrada.

## Criterios de aceptación

- Insertar/crear 10 veces el mismo vídeo: 10 ids, nombres distinguibles y
  destinos distintos, con carpeta y sin ella; uno y varios trims por tanda.
- Crear varios recortes NO cambia ni el contenido ni el nombre del original.
- Dos ventanas guardando a la misma carpeta no comparten destino ni temporal;
  un archivo aparecido tras insertar se conserva byte por byte.
- Ctrl+S y Save as respetan el formato; Canvas nunca escribe sobre MP4.
- Rename provisional no rompe Download ni los demás recortes.
- Save all guarda cada lienzo una vez y continúa navegación/cierre solo al
  terminar todos; fallo/cancelación conserva los documentos pendientes.
- Insertar durante render y recibir respuestas tardías no cambia ni limpia
  otro lienzo. Ediciones de trim/geometría durante render siguen presentes.
- Guardar/reabrir conserva duración, dimensiones y composición; repetir save
  no aplica un recorte doble. Preview correcta sin tener que volver a galería.
- Un diseño editable guardado no pierde el vídeo al limpiar Download.

## Verificación de este diagnóstico

- `cargo check -p canvas-app --tests`: pasa.
- `cargo test -p canvas-io --test video -- --test-threads=1`: 6 pasan con
  FFmpeg/ffprobe presentes, incluida composición, audio, seek y rotación.
- `cargo test -p canvas-app app::video_canvases::tests -- --test-threads=1`:
  bloqueado antes de ejecutar pruebas: GNU ld no encuentra `-lshlwapi`.
- Las pruebas actuales de naming cubren colisiones en una carpeta y en una
  tanda, no el nombre del toolbar, ventanas concurrentes ni Canvas sin carpeta.
- El log histórico muestra renders exitosos a destinos con sufijo: repetir
  el nombre visible no demuestra por sí solo que todos escribieran el mismo
  archivo. También contiene un error GPU fatal separado, sin causa determinada.
- No se reprodujo la interacción en la ventana nativa ni se ejecutó la suite
  completa, Clippy o fmt: es diagnóstico y plan, no una reparación verificada.
