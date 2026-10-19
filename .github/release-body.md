## Novedades de v0.7.10

- **Autoguardado silencioso**: el lienzo activo se guarda solo tras 1,5 s sin cambios, sin diálogos ni avisos de «operación en curso». Respeta todo lo que el usuario todavía está decidiendo (el aviso de «cambió en disco», el diálogo de cambios sin guardar, cualquier modal, «Guardar como…», un arrastre o un texto a medio escribir) y las dos confirmaciones destructivas del `Ctrl+S`; SVG/GIF y los vídeos se quedan como estaban. Si un guardado falla, reintenta solo a los 5 s, 15 s y 60 s y el estado dice «Error al guardar — reintentando…».
- **«Back to gallery» ya no se queda mudo**: navega o explica qué está esperando, con el motivo en pantalla. Al salir del editor se consolidan el texto inline y los campos del panel (cuentan como cambios, no se pierden) y se descartan los renombrados que solo eran UI.
- **Se acabó el banner ⚠ pegado**: un guardado propio limpia el aviso de «cambió en disco» que dejaba un evento anterior de la propia app; antes se quedaba pegado al lienzo toda la sesión.
- **Lienzo mucho más rápido con fotos grandes**: la copia reducida de cada capa se materializa UNA vez en lugar de re-copiarse y reducirse en cada frame y por capa (medido con una foto de 12 MP: 103 ms por capa y frame → 0 ms en reposo), una sola vista de textura por tamaño y los presupuestos de RAM del render medidos una vez por segundo. Sonda reproducible: `CANVAS_DEBUG_PERF=1` y `cargo run -p canvas-render --example fx_hotpath_probe`.
- **El primer lienzo de un bulk ya no sale en blanco**: si un escaneo intermedio marcó su miniatura como fallida, la app la vuelve a pedir una vez al terminar el reescaneo.
- **El panel de propiedades ya no ensucia una imagen intacta**: abrir una capa cuya escala no cae en un decimal (33,333 %) dejaba el documento como modificado y «Back» pedía guardar; ahora lo mostrado coincide con lo aplicado.

## Novedades de v0.7.9

- La pestaña **Web** y la de **Images** son una sola pestaña **Images**, con un selector de fuente (Google / Instagram-Facebook / Unsplash) encima de la barra de búsqueda: la barra y los resultados son los de la fuente elegida, y cambiar de fuente no pierde lo ya buscado —resultados, paginación, créditos y la búsqueda en vuelo sobreviven al ir y volver—; solo alternar entre Google e Instagram/Facebook reinicia, porque el corpus es otro.
- «Bring more» ya no tiene tope: cada búsqueda trae ~100 imágenes y las páginas se añaden hasta que Serper no tenga nada más (el selector de tokens desaparece; los ajustes guardados de antes siguen leyéndose).
- «Bring more» queda al final del mosaico de la ventana de selección múltiple, y su botón de entrada («Bulk add…») ocupa el ancho completo con el texto traducido; las cifras de créditos salen de los textos de la pestaña.
- Mientras la ventana de selección múltiple está abierta, el lienzo oculta la barra de navegación y la barra de selección (Crop/Replace…) para no tapar el trabajo.
- Los tooltips de las pestañas del panel (Page, Layers, Insert, Images y Download) se traducen todos: antes solo tres lo hacían y los otros dos se quedaban en inglés.

## Novedades de v0.7.8

- La pestaña **Download** lista TODOS los clips que siguen en la carpeta temporal (los de sesiones anteriores incluidos) y, al quedarse sin clips al borrar el último (o todos), la propia carpeta desaparece. El escaneo rueda una vez por sesión: no toca el disco en cada frame.
- El modo **Instagram/Facebook** de la pestaña Web vuelve a encontrar fotos de la red en cuentas **gratuitas** de Serper: el degradado sin `site:` (que la free rechaza — «Query pattern not allowed», medido en vivo) ahora nombra la red en la query; el perfil pelado traía 7/100 fuentes y con la red en la keyword llega a 45/100 por página.
- Transparencia de créditos revisada con la API real: 1 token ≈ 10 imágenes/1 crédito, 2 tokens ≈ 100/2, 3 tokens ≈ 200/4 (la segunda página de «Bring more» también cobra 2).
- El **Scale** del panel de propiedades quedó como un único slider logarítmico (mismo UI que Opacity, slider a lo ancho y etiqueta encima) y lo mostrado coincide con lo aplicado (1 decimal).
- Botones rápidos `+4`/`+5`/`+6`/`+7` bajo el campo End del recorte por segmentos del Download: `End = Start + N segundos`, con Start vacío contando como 0; etiquetas Start/End encima de sus cajas y placeholder «00:00».
- Textos de ayuda redundantes del sidebar fuera (el «Save to (temporary)», la nota del segmento y el bloque al pie del listado); no queda ningún aviso que repita lo que ya enseña el panel.

## Novedades de v0.7.6

- La cabecera de la galería muestra el total, las imágenes y los vídeos con **iconos en vez de palabras**, y cada grupo es clicable para filtrar (igual que el menú Todos/Imágenes/Vídeos, siempre sincronizados).
- Si varios renders de la GPU fallan seguidos, el lienzo se congela en la última imagen buena y avisa en la barra de estado en vez de cerrar la app; cuando la GPU responde, se reanuda solo.
- La tarjeta de un download ya no enseña nunca la miniatura de un clip borrado que reutilizó su nombre.
- Primer guardado con nombre reservado atómicamente, etiquetas `{carpeta}-NN` compartidas por capas y lienzos, y fuentes de vídeo duraderas en `.download` que sobreviven al mover el proyecto.
- Las imágenes de internet y los archivos con dimensiones imposibles ya no pueden pedir gigabytes de memoria al abrirlos.

## Novedades de v0.7.7

- Seguridad: las rutas a ejecutables (`YTDLP_PATH`/`CANVAS_YTDLP_DIR`/`CANVAS_FFMPEG_DIR`) se congelan antes de cargar el `.env` — abrir una carpeta con un `.env` ajeno ya no puede redirigir qué binario ejecuta la pestaña Download.
- La resolución de yt-dlp ya no depende de una carpeta fija de desarrollo: variables de entorno o `PATH` (el aviso en el panel lo explica).
- Las cachés de disco ya no crecen sin cota: las miniaturas huérfanas se podan al arrancar (tope de 1000 archivos / 128 MiB) y los JSONs de la pestaña Web más viejos que su TTL también.
- La reserva de nombres de clip (`clip-Nombre (2).mp4`…) lista los parciales de reanudación una sola vez en vez de re-leer la carpeta por candidato.

## Novedades de v0.7.5

- Los lienzos de vídeo se guardan como **mp4 renderizado** (página + fondo con blur + clip colocado y recortado): el primer `Ctrl+S` adopta el render como origen y «Guardar como…» ofrece el tramo recortado (`<origen>-trim.mp4`).
- Tras guardar, el clip ya no se ve distorsionado en el lienzo: la capa se re-ancla al archivo adoptado (tamaño de página y pantalla completa) en vez de decodificarlo al tamaño del clip original.
- La pestaña **Download** rueda entera, formulario incluido: con muchos clips ya no quedan fuera de alcance.
- Tanda de robustez de guardado y papelera: destinos reservados atómicamente, límites de lectura, y deshacer/recuperar de la papelera preservados al navegar.

## Novedades de v0.7.4

- El release incluye los dos `.dmg` de macOS: **Intel** (i5/i7) y **Apple Silicon**. En v0.7.3 solo llegó a publicarse el de Intel por un fallo del pipeline (las dos arquitecturas subían el artefacto con el mismo nombre y una pisaba a la otra).
- En v0.7.2 solo se publicó el de Apple Silicon y en un Intel no había nada que descargar.
- Los lienzos creados desde un clip del panel Download se muestran como vídeo en la baraja, con el nombre del archivo del clip, en vez de como una imagen `N.png`.
- El formato de esos lienzos nuevos respeta el ajuste de Ajustes (PNG, JPEG, WebP o diseño `.canvas`): antes quedaba fijado en PNG.
- «Create canvas» del editor de vídeo añade el lienzo al proyecto en curso en vez de abrir otro proyecto y descartar el actual.
- Recorta varias secciones del mismo vídeo y crea un canvas independiente por recorte.
- Cada recorte conserva sus tiempos y su propio Undo/Redo; navega entre los canvases desde la tira.
- Controles de reproducción centrados y selector de tamaño del canvas alineado a la derecha.
- Instalador Windows probado antes de publicar; hashes SHA-256 y procedencia verificable.
- Edición de vídeo con controles de zoom, posición, desenfoque, tamaño del canvas y recorte preciso.
- Mejoras en reproducción, previsualización vertical y descarga; el audio se excluye por defecto.
- Ajustes y reinicio más claros, y panel de edición desplazable junto a la previsualización.

Apps de escritorio **nativas** para Windows, macOS y Linux. Elige tu sistema,
descarga el archivo y ya está: no hace falta tener Rust instalado ni compilar
nada.

## Windows 10/11 (x64)

Descarga el archivo que termina en **`x64-setup.exe`** y haz doble clic. El
asistente instala la app, registra las asociaciones «Abrir con» del Explorador
de Windows y crea accesos directos en el menú Inicio y el escritorio. Se
desinstala desde *Agregar o quitar programas*, sin dejar rastro.

Si Windows SmartScreen avisa de un editor desconocido, pulsa *Más información →
Ejecutar de todas formas* (el instalador no está firmado con un certificado de
code signing).

## macOS (Apple Silicon e Intel)

Descarga el **`.dmg`** de tu arquitectura (`aarch64` para Apple Silicon M1 y
posteriores, `x86_64` para Intel i5/i7), ábrelo y arrastra *Canvas Desktop* a
la carpeta *Aplicaciones*.

Si el build está firmado por Apple, se abre directamente. Si macOS dice que no
puede verificar al desarrollador (build sin firmar: requiere cuenta de Apple
Developer de pago), ábrela igualmente con **clic derecho (o Control + clic)
sobre la app → Abrir → Abrir**. Solo hace falta hacerlo una vez.

## Linux (x64)

- **AppImage** (funciona en cualquier distro): descarga el `.AppImage`, dale
  permisos de ejecución y ejecútalo:

  ```sh
  chmod +x canvas-desktop_*.AppImage
  ./canvas-desktop_*.AppImage
  ```

- **Debian / Ubuntu**: descarga el `.deb` e instálalo con `apt`, que resuelve
  las dependencias solo (`dpkg -i` no lo hace):

  ```sh
  sudo apt install ./canvas-desktop_*.deb
  ```

## Requisitos

Una GPU con drivers Vulkan (Linux), DX12 (Windows) o Metal (macOS) y su stack
gráfico actualizado: el render usa `wgpu` + `vello`, así que sin aceleración por
hardware la app no arranca.

---

Los paquetes de **macOS y Linux** se compilan de forma *best-effort* y todavía
no están verificados en hardware real; el instalador de **Windows x64** es el
único soportado de verdad hoy. Si algo no arranca, abre un issue indicando el
archivo descargado y el error.
