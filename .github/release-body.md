## Novedades de v0.7.6

- La cabecera de la galería muestra el total, las imágenes y los vídeos con **iconos en vez de palabras**, y cada grupo es clicable para filtrar (igual que el menú Todos/Imágenes/Vídeos, siempre sincronizados).
- Si varios renders de la GPU fallan seguidos, el lienzo se congela en la última imagen buena y avisa en la barra de estado en vez de cerrar la app; cuando la GPU responde, se reanuda solo.
- La tarjeta de un download ya no enseña nunca la miniatura de un clip borrado que reutilizó su nombre.
- Primer guardado con nombre reservado atómicamente, etiquetas `{carpeta}-NN` compartidas por capas y lienzos, y fuentes de vídeo duraderas en `.download` que sobreviven al mover el proyecto.
- Las imágenes de internet y los archivos con dimensiones imposibles ya no pueden pedir gigabytes de memoria al abrirlos.

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
