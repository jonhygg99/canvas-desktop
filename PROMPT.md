# Prompt: Construir "Canvas Desktop"

> Copia todo lo que hay debajo de la línea y dáselo a la IA que vaya a construir el proyecto.
> Es un proyecto **nuevo desde cero**. No reutiliza ningún código previo.

---

## Objetivo

Construye **Canvas Desktop**, una aplicación de escritorio **nativa** para Windows, macOS y Linux: un editor de diseño sobre un lienzo (canvas), tipo Canva, pero integrado con el sistema operativo como un editor de imágenes nativo.

El flujo principal que debe funcionar es este:

1. El usuario hace clic derecho sobre un **archivo de imagen o una carpeta** en su explorador de archivos (Explorador de Windows, Finder de macOS, Nautilus/Dolphin/Files en Linux).
2. Elige **"Abrir con" → "Canvas Desktop"**, y también debe poder marcarse como aplicación predeterminada para ese tipo de archivo.
3. La aplicación abre ese archivo dentro del lienzo de edición.
4. El usuario edita: mueve, recorta, añade texto, formas, filtros, capas.
5. Al guardar (`Ctrl/Cmd + S`), **el archivo original en disco se actualiza** con el resultado editado.

Si lo que se abre es una **carpeta**, la app muestra una galería con las imágenes que contiene para elegir cuál editar.

## Stack técnico: Rust nativo

Este proyecto **no usa Electron, ni Tauri, ni ningún webview, ni JavaScript**. La app es un binario nativo compilado, sin runtime ni navegador embebido.

**Lenguaje: Rust.** Es la elección para este caso porque compila a un binario nativo único para las tres plataformas desde una sola base de código, no arrastra runtime ni recolector de basura, da acceso directo a las APIs nativas de cada sistema (registro de Windows, Cocoa en macOS, freedesktop en Linux) mediante crates oficiales, y tiene un ecosistema gráfico 2D maduro sobre GPU. El resultado son binarios de decenas de megas en vez de cientos, arranque instantáneo y consumo de memoria bajo.

Crates a usar (fija estas decisiones, no improvises alternativas sin justificarlo):

| Función | Crate |
|---|---|
| Ventana y bucle de eventos | `winit` |
| Acceso a GPU | `wgpu` (Vulkan / Metal / DX12 según plataforma, automático) |
| UI: paneles, barras, diálogos | `egui` + `eframe` |
| Menús nativos | `muda` |
| Renderizado 2D vectorial del lienzo | `vello` — **sólo desde la fase de vectores (texto y formas). NO en el MVP**, ver nota abajo |
| Tipos y geometría 2D | `kurbo` y `peniko` |
| Disposición y renderizado de texto | `parley` |
| Códecs de imagen (PNG, JPEG, WebP, GIF, BMP) | `image` |
| Metadatos EXIF y perfiles ICC | `kamadak-exif` + `lcms2` (o equivalente; ver §3) |
| Importar/rasterizar SVG | `resvg` + `usvg` |
| Exportar PDF | `pdf-writer` (o `svg2pdf` a partir del SVG exportado) |
| Serialización del documento | `serde` + `serde_json` |
| Paralelismo (miniaturas, carga) | `rayon` |
| Errores | `thiserror` en las librerías, `anyhow` en el binario |
| APIs de Windows (registro, shell) | `windows` |
| APIs de macOS (Cocoa, delegado de app) | `objc2` + `objc2-app-kit` |
| Instancia única e IPC local | `interprocess` (socket local / named pipe) |
| Vigilar cambios en disco | `notify` |
| Diálogos nativos de archivo | `rfd` |
| Rutas de configuración por plataforma | `directories` |
| Logging | `tracing` + `tracing-subscriber` |
| Empaquetado e instaladores | `cargo-packager` (o `cargo-bundle` + `cargo-wix`) |

> **Nota sobre `vello` y `egui`, léela antes de escribir la primera línea.** No los mezcles en el MVP. `egui` pinta con su propio backend (`egui-wgpu`) y `vello` ejecuta sus propias pasadas de cómputo sobre `wgpu`: hacerlos convivir exige renderizar vello a una textura offscreen y componerla dentro de egui mediante un paint callback de `egui-wgpu`. Es factible, pero es integración fina entre dos renderers, y `vello` está en 0.x rompiendo API entre versiones. **El MVP no necesita nada de eso**: una imagen con blur, redimensionado y alineación es una textura y un shader, no hay ni un solo path vectorial. Empieza pintando la textura directamente con `egui-wgpu`. Cuando llegue la fase de texto y formas, valida la integración de vello en un prototipo mínimo aislado **antes** de construir nada encima.

Organiza el proyecto como un **workspace de Cargo** con crates separados, de forma que el núcleo del editor no sepa nada del sistema operativo ni de la UI:

```
canvas-desktop/
├─ Cargo.toml                  # workspace
├─ crates/
│  ├─ canvas-core/             # documento, capas, elementos, historial. Sin UI, sin SO.
│  ├─ canvas-render/           # escena -> GPU. Sin UI.
│  ├─ canvas-io/               # cargar/guardar imágenes, escritura atómica, ICC, EXIF, .canvas
│  ├─ canvas-shell/            # integración con el SO (assoc, argv, open-file, recientes)
│  └─ canvas-app/              # binario: egui + winit, ata todo lo anterior
├─ packaging/
│  ├─ windows/                 # WiX/NSIS + script de registro
│  ├─ macos/                   # Info.plist, iconos
│  └─ linux/                   # .desktop, mimetypes
└─ assets/
```

`canvas-core` debe ser testeable sin abrir ninguna ventana: escribe tests unitarios de verdad sobre el modelo del documento, el historial y las transformaciones.

## Primera entrega: el canvas mínimo funcionando

**Empieza por aquí y no pases al resto hasta que esto funcione de verdad, ejecutando la app.** El objetivo de esta primera entrega es un lienzo usable de punta a punta: abrir, ver, transformar, guardar. Todo lo demás del documento viene después.

Alcance exacto de esta primera entrega:

1. **Abrir una imagen o una carpeta**, por ahora basta con recibir la ruta por `argv` (la integración con el "Abrir con" del sistema llega más adelante) y por el diálogo nativo de Abrir.
2. **Galería de carpeta**: si lo que se abre es una carpeta, muestra **todos los archivos de imagen que contiene** en una cuadrícula de miniaturas, con su nombre. Al hacer clic en una, se abre en el lienzo. Debe haber una forma clara de volver a la galería sin perder los cambios sin guardar (pregunta antes). Genera las miniaturas en un hilo aparte: la cuadrícula debe poder desplazarse con fluidez aunque la carpeta tenga cientos de imágenes. Reglas concretas: sólo `png`, `jpg`, `jpeg`, `webp`, `gif`, `bmp`, `svg` y `canvas`; **no recursiva**, sólo el primer nivel de la carpeta; ignora los archivos ocultos; ordena por nombre con opción de ordenar por fecha de modificación; si la carpeta no contiene ninguna imagen, muestra un estado vacío que lo explique, nunca una ventana en blanco.
3. **Redimensionar**: hacer la imagen más grande o más pequeña dentro del lienzo. Con manejadores (handles) en las esquinas para arrastrar, y también con un campo numérico de ancho/alto y un porcentaje de escala. Con un candado de **proporción bloqueada** activado por defecto (arrastrar una esquina mantiene la relación de aspecto; `Shift` la libera). Muestra las dimensiones en píxeles mientras se arrastra.
4. **Efecto blur (desenfoque)**: aplicable a la imagen seleccionada, con un deslizador de intensidad y vista previa en vivo mientras se arrastra. Impleméntalo como un shader de `wgpu` (desenfoque gaussiano en dos pasadas, horizontal y vertical), no en CPU píxel a píxel. Es **no destructivo dentro de la sesión**: mientras el documento está abierto es un parámetro de la capa, ajustable o eliminable, y los píxeles en memoria siguen intactos. Al guardar sobre la imagen, el blur se rasteriza de forma definitiva (ver §3).
5. **Opciones de posición y alineación**: botones para alinear la imagen respecto a la página — izquierda, centro horizontal, derecha, arriba, centro vertical (middle), abajo — más un "centrar en la página" que hace las dos cosas a la vez. Además, campos numéricos de X e Y para posicionar con precisión. Cuando haya varias capas seleccionadas, alinear las unas respecto a las otras.
6. **Guardar**: `Ctrl/Cmd + S` actualiza el archivo original en disco, con la escritura atómica y el aviso de sobrescritura descritos en §3. `Ctrl/Cmd + Shift + S` abre "Guardar como…". El título de la ventana marca con un asterisco si hay cambios sin guardar, y cerrar con cambios pendientes pide confirmación.

**Objetivos de rendimiento del MVP** (verificables, no "que vaya rápido"): abrir un JPEG de 24 MP en menos de 2 segundos; arrastrar el deslizador de blur sobre esa misma imagen sin bajar de 60 fps; una galería de 500 imágenes que se desplaza con fluidez desde el primer fotograma, con las miniaturas apareciendo progresivamente y sin bloquear nunca la UI.

Con eso funcionando, el resto del documento amplía cada pieza: el "Abrir con" del sistema, el resto de capas (texto, formas), los demás filtros, el panel de capas y el empaquetado.

## Requisitos detallados

### 1. Integración "Abrir con" en los tres sistemas operativos

Formatos a asociar: `png`, `jpg`, `jpeg`, `webp`, `svg`, `gif`, `bmp`. Añade además una extensión propia de proyecto, **`.canvas`**, para diseños editables.

Cada sistema necesita un tratamiento distinto:

- **Windows**: registra las asociaciones bajo `HKCU\Software\Classes` usando el crate `windows`: un ProgID `CanvasDesktop.Image`, sus `shell\open\command` apuntando a `"<ruta_exe>" "%1"`, y las entradas `OpenWithProgids` por cada extensión. Para el menú contextual de **carpetas**, registra `HKCU\Software\Classes\Directory\shell\CanvasDesktop` (con subclave `command`) y también `Directory\Background\shell\CanvasDesktop` para el clic derecho dentro de una carpeta abierta. Tras escribir en el registro, notifica al shell con `SHChangeNotify(SHCNE_ASSOCCHANGED, ...)` o los cambios no aparecen hasta reiniciar el Explorador. La ruta llega en `std::env::args()`. Haz este registro tanto desde el instalador como desde un botón "Registrar integración con el Explorador" en los ajustes de la app (y su correspondiente botón para desregistrar).
- **macOS**: declara `CFBundleDocumentTypes` con sus `CFBundleTypeExtensions` y `LSItemContentTypes` en el `Info.plist` del bundle `.app`. **Aquí está la trampa principal**: en macOS la ruta NO llega por `argv`. Llega al delegado de la aplicación por `application:openURLs:`, y puede dispararse **antes** de que tu ventana exista. Instala un `NSApplicationDelegate` propio con `objc2` lo antes posible en el arranque y **encola** las rutas recibidas hasta que la app esté lista para consumirlas. Para carpetas, declara un tipo con `LSItemContentTypes: public.folder`. Ten en cuenta que `winit` instala su propio delegado: tienes que envolverlo o inyectar el método sin romper el suyo — resuélvelo explícitamente y deja un comentario en el código explicando cómo.
- **Linux**: genera un `.desktop` con `MimeType=image/png;image/jpeg;image/webp;image/svg+xml;image/gif;image/bmp;inode/directory;` y `Exec=canvas-desktop %U`, instálalo en `~/.local/share/applications/` y ejecuta `update-desktop-database`. Empaqueta como AppImage y `.deb`. La ruta llega por `argv`.

Escribe en `canvas-shell` una **capa de abstracción** que normalice las tres vías (argv en arranque en frío, segunda instancia, `openURLs` de macOS) en un único evento interno `OpenPath(PathBuf)`, para que el resto de la app no sepa en qué sistema está. Expón un trait `ShellIntegration` con una implementación por plataforma detrás de `#[cfg(target_os = ...)]`.

**Instancia única**: abrir un segundo archivo con la app ya abierta debe reutilizar la ventana existente, no arrancar otro proceso. Implementa un lock por socket local / named pipe con `interprocess`: si el lock ya está tomado, envía la ruta al proceso vivo por ese canal y sal con código 0. El proceso vivo la recibe y la trata como un `OpenPath`.

En desarrollo, `argv` puede traer flags de cargo: filtra lo que empiece por `-` y quédate solo con rutas que existan en disco.

### 2. Apertura de archivos y carpetas

- **Archivo de imagen**: crea un documento nuevo, ajusta el tamaño de la página a las dimensiones reales de la imagen (aplicando su orientación EXIF y respetando su perfil de color, ver §3) y coloca la imagen como capa a tamaño completo.
- **Archivo `.canvas`**: carga el documento con todas sus capas y elementos editables.
- **Carpeta**: muestra una galería con miniaturas de las imágenes que contiene, con las reglas de filtrado y orden definidas en la primera entrega. Genera las miniaturas **en un hilo aparte** (usa `rayon` o hilos propios) y cachéalas en el directorio de caché del usuario; la UI nunca debe bloquearse leyendo el disco. Al hacer clic en una miniatura, se abre en el editor.
- **Arrastrar y soltar** un archivo o carpeta sobre la ventana debe hacer exactamente lo mismo que "Abrir con".
- La carga de archivos grandes va en un hilo de trabajo, con indicador de progreso. La UI se mantiene a 60 fps.

### 3. Guardado que actualiza el archivo original

Este es el requisito central: **el canvas actualiza el archivo de imagen en disco**.

**Modelo de guardado — hay exactamente dos vías, y son distintas:**

- **`Ctrl/Cmd + S` sobre una imagen** → rasteriza el documento y **sobrescribe el archivo original**, mismo formato y misma ruta. Es **destructivo e irreversible**: los píxeles originales dejan de existir, y el blur y cualquier otro filtro quedan cocidos para siempre. No se guarda ningún archivo lateral ni respaldo.
- **`Ctrl/Cmd + Shift + S` → "Save as…"** con diálogo nativo (`rfd`). Si el usuario elige la extensión **`.canvas`**, se guarda el documento **editable completo**, con los píxeles de cada capa de imagen embebidos dentro del propio archivo (así el `.canvas` es autocontenido y sigue funcionando aunque el original se mueva o se borre). Esta es la **única** forma de conservar la edición.

**Aviso de sobrescritura destructiva — obligatorio.** La primera vez en cada sesión que el usuario pulsa `Ctrl/Cmd + S` sobre una imagen abierta desde disco, muestra un diálogo que explique en una frase que el archivo original se reemplaza permanentemente y no se puede deshacer. Botones: **Overwrite** / **Save as… instead** / **Cancel**, más una casilla "Don't ask again" que se recuerda en los ajustes. Si el archivo es un JPEG, el diálogo indica además la calidad de recompresión que se va a usar. Sin este aviso, el primer `Ctrl+S` de un usuario destruye su foto sin que lo haya entendido.

**Reglas de integridad, todas obligatorias:**

- **Escritura atómica**: escribe primero en un archivo temporal **en el mismo directorio** (no en `/tmp`, o el renombrado cruzaría sistemas de ficheros), haz `fsync`, y luego renombra sobre el original. Una caída a mitad de guardado nunca debe corromper la imagen del usuario. En Windows, `std::fs::rename` falla si el destino existe: usa `ReplaceFileW` o la estrategia equivalente, y trátalo explícitamente.
- **Recompresión de JPEG**: sobrescribir un `.jpg` lo recomprime, y cada guardado degrada la foto de forma acumulativa. Calidad configurable en los ajustes, **92 por defecto**, mostrada en el aviso de sobrescritura. Si el documento no está sucio, `Ctrl+S` no debe reescribir nada: un guardado que no cambia nada no puede costar calidad.
- **Perfil de color ICC**: lee el perfil incrustado al abrir y **presérvalo tal cual al guardar**. El crate `image` no gestiona ICC por sí solo, así que extrae y reinserta el bloque de perfil explícitamente. Guardar un Adobe RGB sin su perfil cambia los colores de la foto de forma visible; si en algún caso decides convertir a sRGB, avisa al usuario antes.
- **Metadatos**: aplica la orientación EXIF al cargar, y **conserva los metadatos al guardar** (EXIF, fecha de captura, GPS, autor). Guardar sin ellos le borra al usuario la fecha y la localización de sus fotos, que suele importarle más que el propio píxel.
- **SVG y GIF animado**: se pueden **abrir** (el SVG se rasteriza a la resolución de la página; del GIF se toma el primer fotograma), pero **`Ctrl+S` está deshabilitado sobre ellos** y redirige a "Save as…". Un lienzo raster no puede reescribir un SVG vectorial, y sobrescribir un GIF animado lo aplanaría a un único fotograma, destruyendo la animación. Explícalo en el diálogo en vez de fallar en silencio.
- **Formato `.canvas`**: es el formato de datos del usuario, versiónalo desde el primer día. Campo `version` en la raíz del JSON desde la v1. Al abrir una versión **más nueva** que la que entiende el binario, niégate con un error claro ("this file was created with a newer version of Canvas Desktop") en lugar de cargarlo a medias. Al abrir versiones más antiguas, migra. Sin esto, la v2 no podrá abrir los archivos de la v1.
- **Estado sucio**: asterisco en el título de la ventana, y en macOS además `setDocumentEdited:`. Si el usuario cierra con cambios sin guardar, muestra un diálogo nativo con Save / Discard / Cancel.
- **Vigilancia del archivo** con `notify`: si el archivo cambia en disco mientras está abierto, avisa y ofrece recargar. Ignora los eventos que provoque tu propio guardado.
- **Nunca pierdas datos del usuario**: si el guardado falla (disco lleno, permisos, archivo de solo lectura, unidad de red desconectada), dilo con un error claro y accionable, y deja el documento en memoria intacto y marcado como sucio.

### 4. El editor

Como es un proyecto desde cero, el editor hay que construirlo. Modelo de documento en `canvas-core`:

- **Documento** → una o varias **páginas** → árbol de **capas**.
- Tipos de capa: `Image`, `Text`, `Shape` (rect, elipse, polígono, línea, path), `Svg`, `Group`.
- Propiedades comunes de toda capa: posición, tamaño, rotación, escala, opacidad, visibilidad, bloqueo, modo de fusión, sombra.
- **Historial de deshacer/rehacer** basado en comandos (patrón Command con `apply`/`revert`), no en clonar el documento entero. Agrupa las operaciones continuas: arrastrar una capa 200 píxeles es **un** paso de deshacer, no 200.

Funciones del editor:

- Selección, mover, escalar y rotar con manejadores (handles) y guías de alineación magnéticas.
- Texto con fuentes del sistema y controles de tipografía: familia, tamaño, peso, cursiva, interletraje, interlineado, alineación, color.
- Formas con relleno, borde, grosor y radio de esquina.
- Recorte de imagen, volteo horizontal/vertical y máscaras.
- Filtros: brillo, contraste, saturación, escala de grises, sepia, desenfoque, temperatura. Aplicados en GPU mediante shaders de `wgpu`, no en CPU píxel a píxel.
- Panel de capas: reordenar arrastrando, renombrar, ocultar, bloquear, agrupar y desagrupar.
- Alineación y distribución de varias capas seleccionadas.
- Portapapeles: cortar, copiar, pegar y duplicar, incluyendo **pegar imágenes desde el portapapeles del sistema**.
- Zoom y paneo: rueda para zoom, barra espaciadora o botón central para paneo, `Ctrl/Cmd + 0` para ajustar a ventana. Cuadrícula y reglas opcionales.
- Exportar a PNG, JPEG, SVG y PDF, con selector de escala (1x, 2x, 3x).

### 5. Experiencia de escritorio

- **Toda la interfaz va en inglés**, sin sistema de traducción ni i18n: los textos van directos en el código. (Este documento está en español, pero la UI del producto no.)
- Menús nativos de la plataforma (`muda`): File (New, Open, Open Folder, Save, Save As, Export, Open Recent, Quit), Edit (Undo, Redo, Cut, Copy, Paste, Duplicate, Delete, Select All), View (Zoom, Fit to Window, Grid, Rulers, Full Screen), Help. En macOS respeta la convención del menú de aplicación y su barra global.
- Atajos correctos por plataforma: `Cmd` en macOS, `Ctrl` en Windows y Linux.
- **Archivos recientes** persistidos, enganchados a la Jump List de Windows y al Dock de macOS.
- Restaura tamaño y posición de la ventana entre sesiones.
- Tema claro y oscuro siguiendo el del sistema por defecto.
- Pantalla de bienvenida cuando la app se abre sin archivo: New Design, Open File, Open Folder y la lista de recientes.
- Soporte de pantallas HiDPI y de monitores con escalados distintos (mover la ventana entre monitores no debe romper el renderizado).

### 6. Empaquetado y distribución

- **Windows**: instalador MSI o NSIS (x64 + arm64) que registre asociaciones y menú contextual de carpetas, y que las limpie al desinstalar.
- **macOS**: bundle `.app` dentro de un `.dmg`, universal (x86_64 + aarch64), con el `Info.plist` correcto. Deja preparadas firma y notarización, pero desactivables por variable de entorno para poder compilar sin certificado.
- **Linux**: AppImage y `.deb`, con `.desktop`, mimetypes e iconos en los tamaños estándar.
- CI con GitHub Actions que compile las tres plataformas en cada tag.

## Criterios de aceptación — Primera entrega

Estos son los únicos que cuentan al principio. Todos se verifican **ejecutando la app**, sin haber tocado todavía las asociaciones de archivo ni los instaladores:

1. `canvas-desktop <ruta-de-una-carpeta>` abre la cuadrícula de miniaturas con todas las imágenes de esa carpeta, y se desplaza con fluidez con 500 imágenes dentro.
2. Al hacer clic en una miniatura, la foto se abre en el lienzo a su tamaño real y con su orientación EXIF ya aplicada.
3. `canvas-desktop <ruta-de-una-imagen>` la abre directamente en el lienzo.
4. Arrastrar una esquina la redimensiona manteniendo la proporción, y `Shift` la libera. Los campos numéricos de ancho/alto y el porcentaje de escala hacen lo mismo.
5. El deslizador de blur muestra el desenfoque en vivo mientras se arrastra, sin bajar de 60 fps sobre una imagen de 24 MP, y volver el deslizador a 0 devuelve la imagen nítida original.
6. Los botones de alineación colocan la imagen a la izquierda / centro / derecha / arriba / middle / abajo de la página, y "centrar" hace las dos cosas.
7. `Ctrl+S` muestra el aviso de sobrescritura la primera vez. Al confirmar, se cierra la app, se abre el archivo con el visor de fotos del sistema y **se ve el cambio**.
8. Cerrar con cambios sin guardar pide confirmación.
9. Matar el proceso a mitad de un guardado deja el archivo original intacto y decodificable, nunca a medias. Verificado con un test de integración que mata el proceso durante la escritura y luego intenta decodificar el original.
10. `cargo test` pasa. `cargo clippy -- -D warnings` pasa sin avisos. `cargo fmt --check` pasa.

## Criterios de aceptación — Producto completo

**No trabajes en nada de este bloque hasta que el anterior esté verificado ejecutando la app.**

1. Clic derecho sobre un `.png` en el Explorador de Windows → "Abrir con" → Canvas Desktop → la imagen aparece en el lienzo a su tamaño real.
2. Lo mismo funciona en macOS desde Finder y en Linux desde el gestor de archivos.
3. Clic derecho sobre una carpeta → "Abrir con Canvas Desktop" → aparece la galería de imágenes de esa carpeta.
4. Abrir un segundo archivo con la app ya abierta reutiliza la misma instancia.
5. Se añade texto a una foto, se hace "Save as… → `.canvas`", se cierra la app y al reabrir ese `.canvas` el texto **sigue siendo una capa editable y movible**, y el blur sigue siendo ajustable.
6. Abrir un `.svg` y pulsar `Ctrl+S` no destruye el archivo vectorial: redirige a "Save as…".
7. Guardar sobre un JPEG conserva su perfil ICC, su fecha de captura y sus coordenadas GPS.
8. Los instaladores de las tres plataformas se generan, instalan y registran las asociaciones, y al desinstalar las limpian.

## Instrucciones de trabajo

Construye en este orden, y **verifica ejecutando la app de verdad en cada paso**, no solo compilando. Los pasos 1 a 8 son la primera entrega descrita arriba: no empieces el 9 hasta que un usuario pueda abrir una carpeta, elegir una foto, redimensionarla, desenfocarla, centrarla y guardarla.

1. Workspace de Cargo + ventana con `eframe` que abra y pinte una textura con `egui-wgpu`. **Sin `vello`** (ver la nota del stack). **Que arranque antes de seguir.**
2. `canvas-core`: modelo de documento, capas, historial. Con tests.
3. Cargar una imagen desde una ruta pasada por `argv` y renderizarla en el lienzo, con orientación EXIF aplicada. Zoom y paneo.
4. Selección, arrastre y redimensionado con manejadores y proporción bloqueada.
5. Alineación y posición: los botones de izquierda/centro/derecha/arriba/middle/abajo y los campos de X/Y.
6. Filtro blur en GPU, no destructivo en sesión, con vista previa en vivo.
7. Guardar sobre el archivo original: escritura atómica, aviso de sobrescritura, preservación de ICC y metadatos, estado sucio y confirmación al cerrar.
8. Galería de carpetas con miniaturas en hilo aparte.
9. Formato `.canvas` versionado y autocontenido, con "Save as…" y reapertura editable.
10. Asociaciones de archivo y menú contextual de carpetas, plataforma por plataforma.
11. Resto de herramientas del editor: texto, formas, demás filtros, panel de capas. Aquí entra `vello`: valida antes su integración con `egui` en un prototipo aislado.
12. Empaquetado e instaladores.

Reglas:

- Nada de `unwrap()` ni `expect()` en las rutas de código que tocan archivos del usuario. Usa `Result` con `thiserror` en las librerías y `anyhow` en el binario.
- Todo lo que toque disco va fuera del hilo de UI.
- El código específico de plataforma vive **solo** en `canvas-shell`, detrás de `#[cfg]`. Ningún `#[cfg(windows)]` suelto en la lógica del editor.
- Si un crate del stack no da lo que necesitas, **dilo y propón la alternativa antes de improvisar**; no lo sustituyas en silencio.
- Documenta en el `README.md` cómo probar la integración "Abrir con" en cada sistema, incluido cómo hacerlo en desarrollo sin instalar la app.
