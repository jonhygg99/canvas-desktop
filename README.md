# Canvas Desktop

Editor de diseño sobre lienzo (tipo Canva) **nativo**, escrito en Rust: sin
Electron, sin webview, sin JavaScript. Un único binario por plataforma sobre
`winit` + `wgpu` + `egui` + `vello`.

## Estado: primera entrega

El canvas mínimo funcionando de punta a punta:

- **Abrir** una imagen (`png`, `jpg`, `jpeg`, `webp`, `gif`, `bmp`) por
  argumentos de línea de comandos, diálogo nativo o arrastrar y soltar.
  Respeta la orientación EXIF. La página toma las dimensiones reales.
- **Galería de carpeta**: abrir una carpeta muestra sus imágenes en una
  cuadrícula de miniaturas (generadas en paralelo con `rayon` y cacheadas en
  disco). Clic para editar, botón para volver (pregunta si hay cambios).
- **Editar**: seleccionar, mover y redimensionar con manejadores de esquina
  (proporción bloqueada por defecto; `Shift` la libera), campos numéricos de
  X/Y/ancho/alto y % de escala, botones de alineación respecto a la página.
- **Desenfoque gaussiano en GPU** (shader wgpu de dos pasadas), no
  destructivo, con vista previa en vivo. Solo se aplica de verdad al guardar.
- **Guardar**: `Ctrl+S` (o el botón «💾 Guardar» del panel) actualiza el
  archivo original en disco con escritura atómica (temporal en el mismo
  directorio + fsync + `ReplaceFileW` en Windows). `Ctrl+Shift+S` para
  «Guardar como…». Asterisco en el título con cambios sin guardar y
  confirmación nativa al cerrar (Sí = guardar, No = descartar).
- **Deshacer/rehacer** por comandos (`Ctrl+Z` / `Ctrl+Y`); los gestos
  continuos se agrupan (un arrastre = un paso de deshacer).
- **Nuevo diseño** en blanco (1920 × 1080 por defecto) desde la bienvenida;
  arrastrar una imagen sobre un documento abierto la añade como capa nueva.
- **Resolución de página** editable (campos An/Al y presets) en la sección
  «Página» del panel, con deshacer.
- **Fondo desenfocado**: duplica la imagen y la pone de fondo cubriendo toda
  la página con desenfoque 50 por defecto (la imagen original se encaja
  centrada automáticamente si tapaba la página). El slider junto al checkbox
  ajusta la intensidad, y el fondo se recoloca solo al cambiar la resolución.
- **Sombra** por capa: proyectada, difusa y configurable (desplazamiento X/Y,
  difusión y opacidad), activable con un checkbox en la sección de la capa.
- **Sidecar editable**: al guardar se escribe `foto.png.canvas` junto a la
  imagen con el documento completo (capas y píxeles embebidos); al reabrir el
  PNG, las capas vuelven editables tal y como se guardaron — nada de imagen
  aplanada con fondo transparente pegado. Si la imagen cambió por fuera, la
  app avisa y deja elegir. Se puede desactivar con el checkbox «Editable
  sidecar» para no dejar archivos extra. Abrir un `foto.png.canvas` (por argv
  o doble clic) abre su imagen con las capas restauradas.
- **Aviso de sobrescritura destructiva**: el primer `Ctrl+S` de cada sesión
  sobre un archivo de disco avisa de que el original se reemplaza para
  siempre (con la calidad JPEG si aplica), con «Overwrite / Save as… instead /
  Cancel» y un «Don't ask again» persistido en los ajustes.
- **Ajustes persistidos** (`settings.json` en el directorio de configuración):
  calidad JPEG (92 por defecto), aviso de sobrescritura, sidecar por defecto,
  orden de la galería. Ventana «Settings» desde la bienvenida o el editor.
- **Guardado no-op**: `Ctrl+S` sin cambios no reescribe el archivo (un
  guardado que no cambia nada no puede costar calidad JPEG).
- **ICC y EXIF preservados**: al guardar se reinsertan tal cual el perfil de
  color y el bloque EXIF del original (fecha, GPS…), con `Orientation`
  normalizada a 1 porque los píxeles ya se guardan orientados (`img-parts`).
- **SVG y GIF**: se abren (el SVG rasterizado a su tamaño natural con
  `resvg`; del GIF animado, el primer fotograma), pero `Ctrl+S` nunca los
  sobrescribe: un diálogo lo explica y redirige a «Save as…».
- **Galería**: ignora archivos ocultos y ordena por nombre o por fecha de
  modificación (selector persistido).
- **Instancia única**: abrir un segundo archivo con la app ya abierta lo
  envía a la ventana existente por un socket local (`interprocess`); el
  segundo proceso sale con código 0.
- **Vigilancia del archivo** (`notify`): si el archivo abierto cambia en
  disco por fuera, un banner ofrece «Reload / Keep mine». Los guardados
  propios no disparan el aviso.
- **Integración con el Explorador de Windows**: botones «Register /
  Unregister» en Settings crean (y limpian) las asociaciones «Open with» bajo
  `HKCU\Software\Classes` y el menú contextual de carpetas, con
  `SHChangeNotify` para que el Explorador lo refleje al instante.

Pendiente (siguientes entregas): menús nativos (`muda`), recientes + Jump
List, tema y geometría de ventana, rotación/recorte/snap, más filtros GPU,
texto/formas/SVG vectorial, panel de capas, portapapeles, exportación,
empaquetado y CI.

## Compilar y ejecutar

Necesitas Rust estable (en Windows, toolchain MSVC con las Build Tools de
Visual Studio) y una GPU con Vulkan/DX12/Metal.

```sh
# Abrir la pantalla de bienvenida
cargo run -p canvas-app

# Abrir una imagen directamente (así llegará también el «Abrir con» del SO)
cargo run -p canvas-app -- C:\ruta\a\foto.png

# Abrir una carpeta como galería
cargo run -p canvas-app -- C:\ruta\a\carpeta
```

En desarrollo, los flags que cuela cargo (todo lo que empiece por `-`) se
filtran y solo se aceptan rutas que existan en disco.

### Controles

| Acción | Entrada |
|---|---|
| Zoom | Rueda del ratón (anclado al cursor) |
| Paneo | Botón central, o espacio + arrastrar |
| Ajustar a ventana | `Ctrl+0` |
| Seleccionar / mover | Clic / arrastrar sobre la imagen |
| Redimensionar | Arrastrar manejadores de esquina (`Shift` libera proporción) |
| Deshacer / Rehacer | `Ctrl+Z` / `Ctrl+Y` o `Ctrl+Shift+Z` |
| Guardar / Guardar como | `Ctrl+S` / `Ctrl+Shift+S` |
| Añadir imagen como capa | Arrastrarla sobre el editor |

## Probar la integración «Abrir con»

- **Windows (recomendado)**: abre la app → «⚙ Settings» → «Register». Desde
  ese momento, clic derecho sobre una imagen → «Open with» → Canvas Desktop,
  y clic derecho sobre una carpeta (o su fondo) → «Open with Canvas Desktop».
  «Unregister» lo limpia. OJO: el registro apunta al exe actual; si mueves o
  recompilas el binario en otra ruta, vuelve a registrar.
- **Windows (sin registrar)**: clic derecho sobre una imagen → «Abrir con» →
  «Elegir otra aplicación» → «Buscar otra aplicación en el equipo» →
  selecciona `target\debug\canvas-desktop.exe`. También puedes arrastrar un
  archivo o carpeta sobre la ventana abierta.
- **macOS / Linux**: `cargo run -p canvas-app -- /ruta/a/foto.png` o arrastrar
  el archivo sobre la ventana. (El registro por `Info.plist` / `.desktop`
  llega con el empaquetado; los stubs de `canvas-shell` compilan pero
  devuelven `NotImplemented`.)

Con la app ya abierta, cualquier apertura nueva (Explorador, terminal,
`canvas-desktop otra.png`) reutiliza la misma ventana: el segundo proceso
reenvía la ruta por el socket local y sale con código 0.

## Verificación

```sh
cargo test                                  # tests de core, io y shell
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Ejemplos headless (sin ventana) que ejercitan el render GPU real:
cargo run -p canvas-render --example bake_blur -- entrada.png salida.png 20
cargo run -p canvas-render --example save_roundtrip -- entrada.png destino.png
```

`bake_blur` hornea una imagen con desenfoque a un PNG; `save_roundtrip`
ejecuta la misma cadena que `Ctrl+S` (hornear en GPU → codificar → escritura
atómica sobre un archivo existente) y verifica el resultado.

## Arquitectura

Workspace de Cargo; el núcleo no sabe nada del sistema operativo ni de la UI:

```
crates/
├─ canvas-core/     # documento, páginas, capas, historial de comandos. Sin UI, sin SO. Con tests.
├─ canvas-render/   # escena → vello; blur GPU (WGSL, dos pasadas); horneado offscreen + readback
├─ canvas-io/       # cargar (EXIF), guardar atómico (ReplaceFileW), miniaturas con caché
├─ canvas-shell/    # normalización de aperturas del SO → OpenPath (argv hoy; assoc/IPC después)
└─ canvas-app/      # binario `canvas-desktop`: eframe/egui + vello, estados Bienvenida/Galería/Editor
```

Decisiones fijadas:

- **vello 0.9 + eframe/egui 0.35** comparten **wgpu 29**; no actualizar uno
  sin el otro. Vello pinta a una textura `Rgba8Unorm` que egui muestra.
- El blur usa `Renderer::register_texture` de vello para componer la textura
  GPU desenfocada directamente en la escena, sin readback a CPU.
- El «fondo desenfocado» es una capa normal (imagen en transform «cover» +
  blur 50), así que hereda render, deshacer y controles sin código especial;
  las operaciones compuestas (encajar + insertar fondo, cambiar resolución +
  recolocar fondo) se agrupan con el comando `Composite` en UN paso de undo.
- La sombra por capa usa `Scene::draw_blurred_rounded_rect` de vello
  (rectangular y difusa, sin pases de GPU propios).
- Las capas se recortan al rect de la página al renderizar y al hornear.
- `kamadak-exif` se añadió al stack porque el crate `image` no aplica la
  orientación EXIF por sí solo.
- Todo lo que toca disco corre fuera del hilo de UI; los resultados llegan a
  la UI por canales (`std::sync::mpsc`).
