# Descarga y edición de vídeo

Implementación incremental de las ocho mejoras acordadas. UI en inglés;
comentarios y documentación en español. Rust/egui nativo.

- [x] Timeline con miniaturas, tramo conservado y tiradores de inicio/fin.
- [x] Tiempos con milisegundos, marcar inicio/fin y controles de fotograma.
- [x] Vista previa exacta del extremo ajustado y FPS del archivo.
- [x] Zoom de timeline, bucle, Undo/Redo y Reset trim independiente del aspecto.
- [x] Ventana adaptable con preview grande y acción Create canvas explícita.
- [x] Progreso de descarga, cancelación, reintento por vídeo y tarjetas de clips.
- [x] Recorte de descarga opcional, validado y diferenciado del trim de edición.
- [x] Etiquetas Mute preview y Download without audio diferenciadas.

Cada incremento se verifica con tests de comportamiento, formato y Clippy.
Las interacciones de timeline se prueban con eventos reales de egui; los
tests de reproducción usan vídeos generados con FFmpeg. La automatización
de ventanas nativas no está disponible en esta sesión.

Verificación final: 766 tests pasan, 22 ignorados habituales; formato y
Clippy sin incidencias. La integración descarga un vídeo real desde un
servidor local con yt-dlp y verifica progreso y archivo reproducible. Las
pruebas de cancelación ejecutan un proceso real. `cargo run -p canvas-app`
arranca y crea la ventana con la GPU inicializada. Queda pendiente la
revisión visual manual de la ventana nativa.

Correcciones posteriores de Edit video:
- [x] Aislar los clics, atajos y navegación del canvas mientras se edita vídeo.
- [x] Mantener la ventana independiente de la pestaña y tamaño del sidebar.
- [x] Conservar la imagen de preview durante el arranque de reproducción.
- [x] Iconos vectoriales de anterior/siguiente fotograma.
- [x] Start/End con sliders, duración ajustable y presets 5/7/10/15 s al lado del vídeo.
- [x] Reiniciar Play al comienzo del tramo al previsualizar su último fotograma.

Regresiones verificadas con clics de egui: primer Play sin cerrar el editor,
clic sin activar el canvas de detrás y preset 7 s sin introducir texto.
Las pruebas con FFmpeg mantienen la exigencia de fluidez, textura estable,
reloj sincronizado y pausa sin actualizaciones.

Ajustes de distribución y sliders:
- [x] Sliders de Start/End con escala fija y redondeo solo al interactuar.
- [x] Regresión: cruzar Start con End y alternar presets no detiene Play ni cambia el trim durante un repintado.
- [x] Columna derecha en orden Zoom, Background blur y Trim.
- [x] Canvas size junto al transporte, después del control de audio.
- [x] Transporte con iconos vectoriales, nombres accesibles y tooltips.
- [x] Acciones inferiores a la derecha; Create canvas en el extremo derecho.

- [x] Posición X/Y debajo de Zoom: sliders, campos en píxeles y Center video.
- [x] Preview y canvas conservan el desplazamiento; el fondo permanece fijo.
- [x] Reabrir Edit video restaura la posición desde la capa existente.
- [x] Reset individual de Zoom, X, Y, Background blur, Start, End y Duration; Canvas size conserva solo el selector.
- [x] Scroll propio del panel derecho con la altura de la preview izquierda.
- [x] Preview vertical y controles de transporte centrados en su columna.
- [x] Download without audio activado al inicializar el panel.

En Windows hay que cerrar la app antes de recompilar el ejecutable.

Recortes múltiples del mismo clip:
- [x] Reproducción centrada independientemente de Canvas size, alineado a la derecha.
- [x] En ventanas estrechas, Canvas size pasa a una segunda fila sin solapar el transporte.
- [x] Add trim añade un tramo independiente; la lista permite seleccionarlo o eliminarlo.
- [x] Cada tramo conserva sus tiempos y su propio Undo/Redo.
- [x] Create N canvases crea un lienzo por recorte, compartiendo zoom, posición, fondo y tamaño.
- [x] Cada canvas conserva su póster y trim al navegar, y se marca como pendiente de guardar.

Validación: pruebas con eventos reales de egui para Add trim/Create N canvases,
regresión del centrado y creación/navegación de todos los recortes. La revisión
visual nativa sigue pendiente porque el puente Computer Use falla al iniciarse.
