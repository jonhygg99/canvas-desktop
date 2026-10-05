# Descarga y edición de vídeo

Implementación incremental de las ocho mejoras acordadas. UI en inglés;
comentarios y documentación en español. Rust/egui nativo.

- [x] Timeline con miniaturas, tramo conservado y tiradores de inicio/fin.
- [x] Tiempos con milisegundos, marcar inicio/fin y controles de fotograma.
- [x] Vista previa exacta del extremo ajustado y FPS del archivo.
- [x] Zoom de timeline, bucle, Undo/Redo y Reset trim independiente del aspecto.
- [ ] Ventana adaptable con preview grande y acción Create canvas explícita.
- [ ] Progreso de descarga, cancelación, reintento por vídeo y tarjetas de clips.
- [ ] Recorte de descarga opcional, validado y diferenciado del trim de edición.
- [x] Etiqueta Mute preview; pendiente Download without audio.

Cada incremento se verifica con tests de comportamiento, formato y Clippy.
Las interacciones de timeline se prueban con eventos reales de egui; los
tests de reproducción usan vídeos generados con FFmpeg. La automatización
de ventanas nativas no está disponible en esta sesión.
