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

Verificación final: 756 tests pasan, 22 ignorados habituales; formato y
Clippy sin incidencias. La integración descarga un vídeo real desde un
servidor local con yt-dlp y verifica progreso y archivo reproducible. Las
pruebas de cancelación ejecutan un proceso real. `cargo run -p canvas-app`
arranca y crea la ventana con la GPU inicializada. Queda pendiente la
revisión visual manual de la ventana nativa.
