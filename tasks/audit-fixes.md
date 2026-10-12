# Correcciones de la auditoría

Las correcciones están separadas por comportamiento en la rama `fix/audit-data-safety`. Las correcciones previas del árbol de trabajo tienen su propio commit de base; la configuración y los logs locales quedan fuera.

| Hallazgo | Comportamiento | Verificación |
| --- | --- | --- |
| Formato y ruta de guardado adoptados antes del éxito | Se actualizan tras guardar correctamente | Regresiones de cambio de formato y ruta |
| Cierre de la raíz descarta una ventana hija sucia | La hija pasa por la confirmación de guardado | Regresión de cierre de raíz |
| Abrir por arrastre o diálogo evita la confirmación | Usa el mismo camino de navegación protegida | Pruebas de navegación y cierre |
| Guardar todo omite formatos protegidos y provisionales | Incluye todos los documentos sucios y solicita ruta cuando corresponde | Pruebas de cola y recuento de Guardar todo |
| Navegar purga archivos necesarios para deshacer | La app conserva la papelera del proyecto | Pruebas de navegación y operaciones de archivos |
| Renombrar deja una ruta antigua en un documento cargado | La ruta del documento acompaña a la de la ranura | Regresión de renombrado de documento de fondo |
| Deshacer recupera otra copia con el mismo nombre | El registro conserva las rutas exactas de imagen y sidecar | Regresiones con dos borrados del mismo nombre |
| Guardar acepta capas que el lector rechaza | Valida dimensiones, RGBA, IDs y presupuesto antes de escribir | Regresiones que conservan el archivo anterior |
| Importar o reemplazar modifica otro documento | La respuesta conserva generación e ID de ranura | Regresión de respuestas locales caducas |
| Capas comprimidas exceden la memoria conjunta | El lector limita el RGBA conjunto antes de decodificar cada capa | Regresión con presupuesto reducido |
| El sondeo de vídeo ignora la matriz de rotación | Lee la rotación de `side_data_list` | Vídeo real rotado: sondeo y póster de 48×64 |
| El historial global conserva ediciones locales caducadas | Retira los pasos que ya no existen en el historial local | 250 ediciones, 200 deshacer; conserva otros documentos y creaciones |
| La galería lee y copia todos los blobs | Los sondeos v5 leen solo la cabecera; la lectura completa toma blobs prestados | Regresión de sondeos independientes de los blobs |
| Dos enums representan el mismo resultado de apertura | Reutiliza `canvas_io::OpenOutcome` | Suite completa de apertura y carga |

## Límites y conservación

Un archivo `.canvas` admite hasta 512 MiB en disco y 512 MiB de píxeles RGBA de capas en conjunto. Cada imagen embebida admite dimensiones de hasta 20.000 píxeles por eje. La validación de escritura rechaza dimensiones inválidas, buffers inconsistentes e IDs repetidos antes de reemplazar el destino. Los documentos legacy conservan su compatibilidad de lectura.

La papelera del proyecto requiere limpieza manual. Navegar o cerrar no elimina sus archivos. Esta conservación consume espacio en disco y permite que otra ventana siga deshaciendo sus propios borrados.

## Validación

Comandos de comprobación:

```text
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo test -p canvas-render --test gpu_bake --locked -- --ignored --test-threads=1
python -m unittest discover -s scripts -p '*_tests.py'
./scripts/release_tests.ps1
```

Pasan las 811 pruebas de la suite completa, las 19 pruebas de GPU ejecutadas aparte y las 22 pruebas Python. Clippy, formato, build de la app y ensayos de publicación pasan. Las pruebas de vídeo y escritura usan archivos y procesos reales. Los ensayos de publicación no modifican GitHub.

El binario compilado arranca y abre `fixture.png` (160×120) en una ventana nativa, con ajustes y logs en un directorio temporal y el reenvío a otras instancias desactivado. La prueba de arranque no modifica proyectos existentes.

La comprobación visual de la app nativa queda pendiente: el helper de Computer Use devuelve `Computer Use native pipe is unavailable`, error 2. Las regresiones de interacción ejecutan los handlers reales y los frames egui, pero no sustituyen esa comprobación visual.
