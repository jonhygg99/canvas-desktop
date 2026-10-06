## Novedades de v0.7.0

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

## macOS (Apple Silicon)

Descarga el **`.dmg`**, ábrelo y arrastra *Canvas Desktop* a la carpeta
*Aplicaciones*.

Este build **no está firmado ni notarizado** (eso requiere una cuenta de Apple
Developer de pago), así que la primera vez macOS dirá que no puede verificar al
desarrollador. Para abrirla igualmente: **clic derecho (o Control + clic) sobre
la app → Abrir → Abrir**. Solo hace falta hacerlo una vez.

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
