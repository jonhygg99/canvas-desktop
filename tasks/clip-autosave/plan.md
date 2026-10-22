# Plan: autoguardado de clips (lienzos de vídeo creados desde Download)

## Objetivo

Que un lienzo de clip se guarde solo al añadirlo al canvas (todos los de una
tanda, también los que quedan de fondo en la baraja) y que, una vez guardado,
sus ediciones posteriores se re-rendericen solas con más calma que un PNG.

Hoy el clip se queda como ranura provisional enlazando el temporal de Download
y solo se escribe al pulsar `Ctrl+S` o al salir con «Save» desde el aviso de
«cambios sin guardar», que guarda los lienzos uno por uno.

## Decisiones

- **Una sola política de destino** (`app/clip_save.rs`), compartida por el
  `Ctrl+S` (`views/editor/save_flow.rs`) y el autoguardado/la cola.
- **Cola de clips** (`ClipSaveQueue` en `SaveFlow`, ids de ranura estables) con
  **un solo render en vuelo**: cubre los clips de fondo sin cambiar de lienzo.
- **Reutilizar el motor existente** (`spawn_video_save` + `ImageInsertDest`):
  las ranuras de fondo marcan `SlotDoc::saving`/`saving_capture`, que ya viajan
  con `take_slot`/`put_slot` si el usuario salta a esa ranura a mitad de render.
- **Nunca a ciegas**: solo clips renderizables, con fuente en disco y carpeta de
  proyecto. Nada de diálogos desde el temporizador; sin aviso de sobrescritura
  (el destino es el nombre reservado o el archivo que la app misma renderizó).
- **Salir/cerrar espera** a la cola, igual que a un guardado en curso.
- **`VIDEO_QUIET = 5 s`** para re-renders (x264 cuesta mucho más que hornear).

## Fuera de alcance

- Clips añadidos desde la galería (baraja sin carpeta) y mp4 sueltos sin
  proyecto: siguen necesitando «Save as…».
- Cambiar el aviso de sobrescritura de imágenes.
- Re-renderizar durante un gesto o un slider a medio arrastrar.

## Tareas

1. Extraer `app/clip_save.rs` (composición, destino, arranque) + tests.
2. `payload_of` compartido en `editor/state/sidecar.rs`.
3. `ClipSaveQueue` + encolar al añadir clips.
4. Driver del frame (`clip_save::tick`).
5. Finalización de un render de fondo.
6. Gates de navegación/cierre con la cola ocupada.
7. `VIDEO_QUIET` en el autoguardado de clips ya guardados.
8. Estado visible en la barra + documentación.
9. Verificación en la app real con clips generados con ffmpeg.

## Riesgos

| Riesgo | Mitigación |
|---|---|
| Render largo bloquea Back/cierre | Aviso que nombra la espera; un render a la vez |
| Re-render a cada ajuste | `VIDEO_QUIET` + `is_idle_except_save` |
| Payload de ranura de fondo | Un póster (~8 MB a 1080p), movido al hilo |
| Cambio de proyecto a mitad | Generación + id de ranura; respuesta obsoleta se descarta |

Detalle completo: la propuesta aprobada en el hilo (tasks/clip-autosave/todo.md
lleva la lista ejecutable).
