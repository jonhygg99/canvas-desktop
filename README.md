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
  app avisa y deja elegir. Se puede desactivar con el checkbox «Sidecar
  editable» para no dejar archivos extra.

Pendiente (siguientes entregas): sidecar `.canvas` editable, integración
«Abrir con» del sistema e instancia única, texto/formas/más filtros, panel de
capas, exportación SVG/PDF, menús nativos y empaquetado.

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

## Probar la integración «Abrir con» (en desarrollo, sin instalar)

La integración registrada con el shell llega en una entrega posterior, pero el
flujo ya funciona porque la ruta entra por `argv`:

- **Windows**: clic derecho sobre una imagen → «Abrir con» → «Elegir otra
  aplicación» → «Buscar otra aplicación en el equipo» → selecciona
  `target\debug\canvas-desktop.exe`. También puedes arrastrar un archivo o
  carpeta sobre la ventana abierta.
- **macOS / Linux**: `cargo run -p canvas-app -- /ruta/a/foto.png` o arrastrar
  el archivo sobre la ventana. (El registro por `Info.plist` / `.desktop`
  llega con el empaquetado.)

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
