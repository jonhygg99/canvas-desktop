# Tareas: autoguardado de clips

Una tarea por commit lógico; la app se prueba de verdad al final.

## 1. Extraer la política de clip a `app/clip_save.rs`

**Descripción:** mover composición, destino y arranque del render desde
`app/video_canvases.rs` y `app/views/editor/save_flow.rs` a un módulo único.
**Aceptación:**
- [x] `Ctrl+S`, «Save as…» y el póster de un clip se comportan igual que antes.
- [x] `clip_save_target` es pura y decide destino/`reserve` sin GPU.
- [x] Ningún archivo nuevo pasa de 400 líneas.
**Verificación:** `cargo test -p canvas-app`, clippy, fmt.
**Dependencias:** ninguna.

## 2. `payload_of` compartido

**Descripción:** `editor::state::sidecar::payload_of(doc, images, bg)`; el
`sidecar_payload` del editor delega.
**Aceptación:** [x] tests de sidecar sin cambios; [x] sin blobs huérfanos.
**Dependencias:** ninguna.

## 3. `ClipSaveQueue` + alta al añadir

**Descripción:** cola de ids en `SaveFlow`, llenada al crear lienzos de clip.
**Aceptación:**
- [x] 3 clips → 3 ids, sin duplicados, `busy()`.
- [x] generación distinta o ranura desaparecida → se descarta.
- [x] sin carpeta de proyecto no se encola nada (traza del motivo).
**Dependencias:** 1.

## Checkpoint A

- [x] `cargo test`, clippy `-D warnings`, fmt limpios.
- [x] Sin cambios visibles en la app todavía.

## 4. Driver del frame

**Descripción:** `clip_save::tick` tras `autosave::tick`; un render por vez.
**Aceptación:**
- [x] Nunca dos renders en vuelo.
- [x] Un clip no renderizable se descarta sin bloquear a los demás.
- [x] El render de fondo no pausa el vídeo ni corta el audio del activo.
**Dependencias:** 1-3.

## 5. Finalización de un render de fondo

**Descripción:** en `messages/video_save.rs`, adoptar ruta/nombre y marcar el
historial de la ranura de fondo; continuar la cola.
**Aceptación:**
- [x] La ranura deja de ser provisional sin cambiar de lienzo.
- [x] Ediciones durante el render dejan el historial sucio.
- [x] Generación/ranura desaparecida → respuesta descartada con traza.
**Dependencias:** 4.

## 6. Salir espera a los clips

**Descripción:** motivo nuevo en `nav_block_reason` y en `confirm_window_close`.
**Aceptación:** [x] Back aparcado con aviso y reanudado al vaciarse la cola;
[x] cerrar no abre el diálogo mientras se renderiza.
**Dependencias:** 3.

## Checkpoint B

- [ ] Uno y varios clips escriben su mp4 + sidecar sin `Ctrl+S` ni `Back`.
- [ ] Back durante el render avisa y termina navegando con el archivo completo.

Las dos casillas son del recorrido en la app real (probado al final, tarea 9);
las piezas que las sostienen están cubiertas por tests: escritura real de mp4 +
sidecar con ffmpeg (`loader/video_save_tests.rs`), cola de un render a la vez y
compuerta de navegación.

## 7. `VIDEO_QUIET` en el autoguardado

**Descripción:** espera por documento (1,5 s PNG / 5 s clip), destino de clip y
sin avisos destructivos; identidad del ciclo = ruta de la ranura.
**Aceptación:**
- [x] Clip provisional con carpeta → guarda; sin carpeta/fuente → no guarda.
- [x] Clip no dispara a los 2 s y sí a los 5 s; PNG sigue a 1,5 s.
- [x] Dos clips del mismo temporal no comparten intentos.
**Dependencias:** 1, 3.

## 8. Estado visible y documentación

**Descripción:** `Saving clips… (N left)` en la barra; trazas; docs de módulo y
`AGENTS.md`.
**Aceptación:** [x] precedencia del texto probada; [x] ninguna doc dice ya que
los clips no se autoguardan.
**Dependencias:** 4, 7.

## 9. Verificación real

**Descripción:** clips generados con ffmpeg en `%TEMP%\canvas-desktop-downloads`,
proyecto con una imagen, añadir y observar.
**Aceptación:**
- [ ] Los mp4 aparecen en la carpeta del proyecto en segundos.
- [ ] Cambiar el trim re-renderiza tras ~5 s.
- [ ] Back durante el render avisa y luego navega.
- [ ] `canvas-debug.log` sin `ERROR`/`panic`; `ffprobe` válido al cerrar.
**Dependencias:** 1-8.
**Nota:** el recorrido interactivo (añadir clips desde Download y mirar la
barra) queda pendiente de una sesión con GUI; la escritura real de mp4 ya la
cubren los tests de `loader/video_save_tests.rs` con ffmpeg de verdad, y la
adopción de una escritura de fondo en su ranura, sin tocar el documento
activo, en `app/messages/video_save_tests.rs`.

## Checkpoint final

- [x] `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo fmt --all -- --check` limpios (2026-10-22: 704 tests de `canvas-app`
      y 288 del resto del workspace en verde, más los 19 de GPU y los tests de
      vídeo con ffmpeg marcados `#[ignore]`; clippy y fmt sin diferencias).
      Si el enlazado del binario de pruebas de `canvas-app` falla con
      `LNK1103: debugging information corrupt` / `LNK1123`, el artefacto
      cacheado de `canvas-render` está roto: `cargo clean -p canvas-render` y
      volver a lanzar las pruebas (no es un fallo del código).
- [ ] App ejercitada de verdad: solo hay arranque limpio (sin `ERROR`/`panic`
      en `canvas-debug.log`); falta añadir clips reales desde Download a mano.
