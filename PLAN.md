# Plan de trabajo — Canvas Desktop

Plan derivado de `PROMPT.md` con las decisiones fijadas: **solo sidecar** (el
`.canvas` es el archivo `foto.png.canvas` junto a la imagen; no hay "Save as…
→ .canvas" autocontenido), **Windows primero** (macOS/Linux quedan como stubs
compilables en `canvas-shell`), y plan completo hasta empaquetado y CI.

Regla de oro heredada de la spec: cada fase se verifica **ejecutando la app**,
no solo compilando. `cargo test`, `cargo clippy -- -D warnings` y
`cargo fmt --check` deben quedar limpios en cada fase.

---

## PENDIENTE

### Verificación interactiva pendiente (requiere una persona delante)

Lo automatizable ya está verificado; esto necesita ojos y ratón:

1. `Ctrl+S` sobre un JPEG real → el modal muestra la calidad (92); "Don't ask
   again" sobrevive al reinicio; al confirmar, el visor de Windows muestra el
   cambio.
2. Foto con GPS/fecha/Adobe RGB → guardar → `exiftool` (o Propiedades del
   Explorador) conserva fecha, GPS y perfil, con `Orientation: normal`.
3. Settings → Register → clic derecho sobre un `.png` → "Open with" → Canvas
   Desktop; clic derecho sobre carpeta y sobre el fondo de una carpeta;
   Unregister lo limpia sin reiniciar el Explorador.
4. Con la app abierta, abrir otra imagen desde el Explorador reutiliza la
   MISMA ventana (y un diálogo pregunta si hay cambios sin guardar).
5. Editar el archivo abierto con Paint → banner "Reload / Keep mine"; guardar
   desde la app NO dispara el banner.
6. Menús nativos, Open Recent, Jump List de la barra de tareas, tema
   System/Light/Dark en vivo, geometría de ventana restaurada.
7. Rotar/voltear/recortar con deshacer; guías magnéticas al arrastrar (Alt
   las desactiva); Grid y Rulers desde el menú View.
8. Los 6 sliders de Adjustments a 60 fps sobre un JPEG de 24 MP; Reset
   devuelve la imagen exacta.
9. Insertar texto y formas, editarlos desde el panel, guardar con sidecar,
   reabrir → siguen siendo capas editables (criterio de producto 5 adaptado).
10. Repasar los 10 criterios de aceptación de la primera entrega de
    `PROMPT.md` de una sentada.
11. Arrastrar una fila en el panel de capas cambia el orden de apilado en el
    lienzo (Above/Below/Into); Ctrl+G/Ctrl+Shift+G agrupan/desagrupan desde el
    teclado; el ojo y el candado ocultan/bloquean con herencia visible sobre
    los hijos; renombrar con doble clic.
12. Win+Shift+S (recorte de pantalla) → Ctrl+V pega la captura como capa
    nueva; Ctrl+C/X/V/D sobre una selección múltiple (Ctrl+clic/Shift+clic)
    copian/cortan/pegan/duplican el conjunto completo.
13. File → Export… → PDF abre en Edge con texto nítido al ampliar (vectorial,
    no rasterizado); Export SVG abre en Edge/Inkscape con el texto como
    `<text>` real, no una imagen; Export PNG a 2x/3x.
14. Confirmar en un runner real de GitHub Actions (`windows-latest`) que el
    cross-compile a `aarch64-pc-windows-msvc` enlaza: en local falla
    (`link.exe` sin el componente "MSVC ARM64 build tools" de Visual
    Studio instalado) — `release.yml` lo marca `continue-on-error` y no
    bloquea el release de x64. Tampoco hay hardware Windows ARM64 para
    instalar y probar el `.exe` resultante aunque compile.
15. Generar de verdad el bundle `.app`/`.dmg` en un Mac: `canvas-shell/src/
    macos.rs` sigue siendo el stub `NotImplemented`; falta instalar un
    `NSApplicationDelegate` propio con `objc2` que envuelva el de `winit` y
    encole `application:openURLs:` (puede llegar antes de que la ventana
    exista). `packaging/macos/Info.plist` está escrito y es XML válido, pero
    sin `plutil -lint` real ni instalación en un Mac.
16. Generar de verdad el AppImage/`.deb` en Linux: `canvas-shell/src/
    linux.rs` sigue siendo el stub `NotImplemented`. Ojo:
    `cargo packager --formats deb,appimage` genera su PROPIO `.desktop` a
    partir de `[package.metadata.packager]` (no lee
    `packaging/linux/canvas-desktop.desktop`), así que el `.desktop`
    real que sale del build de CI hoy NO incluye ni el MIME propio
    `.canvas` ni `inode/directory` — los archivos vendidos en
    `packaging/linux/` (`.desktop` + `canvas-desktop.xml` de
    shared-mime-info) son referencia para cuando se conecte de verdad, no
    algo que el pipeline ya use.
17. Confirmar en un tag de prueba real que `ci.yml` queda en verde en los
    tres sistemas operativos: esta sesión solo pudo compilar/testear en
    Windows localmente. Se corrigieron dos bloqueadores para que
    `canvas-app` compile fuera de Windows (`anyhow`/`tracing`/
    `tracing-subscriber` estaban mal ubicados bajo
    `[target.'cfg(windows)'.dependencies]`; `unused_mut` en
    `native_menus`), pero el fallback de menú egui de ~180 líneas en
    `menus.rs` (`#[cfg(not(windows))]`) nunca se ha compilado de verdad
    fuera de Windows — es la primera cosa a mirar si `ci.yml` falla en
    ubuntu/macos.

---

## HECHO (commits `eede998` … `b854c94`)

| Fase | Contenido | Verificación |
|---|---|---|
| 1 | Ajustes persistidos (`settings.json`), aviso de sobrescritura destructiva con "Don't ask again", calidad JPEG (92), guardado no-op, UI en inglés | clippy/fmt/tests + arranque |
| 2 | ICC y EXIF preservados al guardar (`img-parts`), `Orientation`→1 parcheado in situ | tests de roundtrip (JPEG APP1/APP2, PNG iCCP) |
| 3 | SVG abre rasterizado (`resvg`), GIF primer fotograma, `Ctrl+S` sobre ellos redirige a "Save as…" | tests + app con SVG real |
| 4 | Test de kill a mitad de guardado (criterio 9), galería sin ocultos y orden Nombre/Fecha | `cargo test` (proceso hijo real) |
| 5 | Instancia única (`interprocess`), watcher `notify` con banner, trait `ShellIntegration`, `foto.png.canvas` abre su imagen | 2ª instancia sale con 0, primaria recibe la ruta |
| 6 | "Abrir con" en Windows: ProgID + `OpenWithProgids` + menú contextual de carpetas + `SHChangeNotify`, botones Register/Unregister | claves comprobadas con `reg query` y limpiadas |
| 7 | Menús nativos `muda` (atajos siguen en egui), recientes + Jump List (COM STA), tema, geometría persistida, navegación unificada con diálogo de cambios | menú instalado, Jump List OK headless, recents en settings.json |
| 8 | Rotación (manejador, Shift=15°), volteo, recorte no destructivo (`CropRect` + trim/uncrop), guías magnéticas (`snap.rs`), cuadrícula y reglas | 12 tests nuevos de geometría pura |
| 9 | Filtros de color GPU (brillo/contraste/saturación/temperatura/grises/sepia) encadenados al blur, `SetEffects` consolidado | example `bake_filters`: neutro byte-idéntico, grises R≈G≈B |
| 10 | Gate parley 0.11 + vello 0.9 (un solo `peniko`), capas de **texto** y **formas** editables, `SvgContent`, sidecar v2 | example `text_probe` (PNG verificado) + clippy/tests |
| 11 | Grupos (`parent_id` + invariante de preorden, comandos `Reorder`/`Group`/`Ungroup`/`Rename`/`SetVisible`/`SetLocked`/`SetOpacity`), panel de capas (arrastrar, renombrar, ojo, candado), `Selection` multi-capa con primaria, portapapeles interno + pegado de imágenes del sistema (`arboard`), exportación PNG/JPEG/SVG/PDF con escala (`svg2pdf`), sidecar v3 | example `export_probe`: escala 2x exacta, opacidad de grupo horneada (alpha 127≈128), SVG reparseado con usvg + 107 tests |
| 12 | Empaquetado Windows real: `build.rs` + `winresource` incrusta `assets/windows/icon.ico` en el `.exe`; flags headless `--register-shell`/`--unregister-shell` en `canvas-desktop` delegan en `canvas_shell::windows` (sigue siendo la única lista canónica); `packaging/windows/installer.nsi` bifurcado de cargo-packager (fork documentado y pineado al tag `@crabnebula/packager-v0.11.8`) invoca esos flags por `nsExec` en Install/Uninstall y corrige el AppUserModelID de los accesos directos (`${AUMID}`, no `${IDENTIFIER}`); `[package.metadata.packager]` en `canvas-app/Cargo.toml` (`installer-mode = "currentUser"`, sin usar el `file-associations` propio del empaquetador); iconos `.ico`/`.icns`/hicolor generados sin herramientas externas (`cargo run -p canvas-render --example gen_icons`, solo `resvg`+`tiny-skia`); `packaging/macos/Info.plist` y `packaging/linux/{.desktop,canvas-desktop.xml}` preparados, sin verificar; `.github/workflows/ci.yml` (fmt/clippy `-D warnings`/test en windows+ubuntu+macos) y `release.yml` (NSIS x64 garantizado; arm64/dmg/AppImage/deb best-effort, `continue-on-error`) | ciclo real `cargo packager --formats nsis` → instalar → `reg query` confirma las 8 claves canónicas + entrada de desinstalación → desinstalar → registro y carpeta completamente limpios (probado dos veces); icono visible en la barra de título (captura); 110 tests + clippy/fmt limpios tras mover `anyhow`/`tracing`/`tracing-subscriber` fuera de `[target.'cfg(windows)']` |

Estado global: **110 tests**, `clippy -D warnings` y `fmt --check` limpios (Windows; ver ítem 17 de verificación pendiente para ubuntu/macos).

## Decisiones tomadas (no reabrir sin motivo)

- **ICC/EXIF con `img-parts`**, no `lcms2` (lcms2 convierte color, no
  preserva bloques). El parche de `Orientation` es un parser TIFF propio de
  ~40 líneas con fallo suave.
- **muda sin aceleradores nativos**: sin acceso al event loop de eframe no hay
  `TranslateAcceleratorW`; los atajos los gestiona egui y el menú los muestra
  como texto. En Linux el fallback es una barra de menús egui.
- **El checkbox del sidecar en el editor ES el ajuste persistido**
  (`sidecar_default`).
- **Snap solo entre capas sin rotar** (con rotación los bordes AABB no
  significan nada); umbral 6 px de pantalla, Alt lo desactiva.
- **Recorte = "trim de bordes"**: el contenido queda clavado en la página y la
  ventana visible se mueve sobre él; `uncrop` lo restaura en el sitio.
- **parley fijado en 0.11** (comparte `peniko 0.6` con vello 0.9); si se
  actualiza vello hay que revalidar con `cargo tree -i peniko` y el example
  `text_probe`.
- **Grupos: `parent_id` + invariante de preorden**, no anidar `Vec<Layer>`: los
  descendientes de un grupo ocupan el tramo contiguo justo por encima de su
  cabecera en `Page::layers`; el renderer y el exportador SVG recorren esa
  lista con un índice + pila de fin de subárbol. Toda mutación del árbol pasa
  por `Page::move_subtree`/`insert_child` para no romper la invariante.
- **Selección múltiple con primaria** (`canvas_core::Selection`): la primera
  capa manda en el panel de propiedades y los gestos del lienzo; el resto de
  la selección solo participa en operaciones en bloque (`roots`/
  `in_stack_order`) como agrupar, borrar o copiar.
- **Texto en SVG con un `<tspan>` por línea**, usando las métricas reales de
  parley (`canvas_render::text_lines`) en vez de dejar que el visor SVG rompa
  líneas por su cuenta; canvas-io no lleva motor de texto propio.
- **PDF vía `svg2pdf` sobre `resvg::usvg`** (no `svg2pdf::usvg`): fuerza a que
  ambos compartan la misma versión de usvg 0.45 — si se actualiza `resvg` hay
  que revalidar con `cargo tree -i usvg` (una sola entrada).
- **cargo-packager NO tiene `installer_hooks`** (eso es de Tauri v2): solo
  expone `preinstall_section` (código antes de instalar, no toca el
  desinstalador) y `template` (sustituir el `.nsi` entero). Por eso el
  registro «Abrir con» se delega en el propio binario
  (`--register-shell`/`--unregister-shell`, headless, sin ventana, sin tocar
  la instancia única — se interceptan al principio mismo de `main()`,
  antes de `acquire_instance`) invocado por `nsExec` desde
  `packaging/windows/installer.nsi`, una plantilla bifurcada y pineada al
  tag `@crabnebula/packager-v0.11.8` con solo 4 cambios documentados en su
  propia cabecera. `windows.rs` sigue siendo la única lista canónica; no
  hay una segunda copia de las claves de registro que pueda desincronizarse.
- **NO se usa el `file-associations` propio de cargo-packager**: su macro
  `APP_ASSOCIATE` fija un ProgID **por defecto** por extensión, y esta app
  solo quiere aparecer como opción en «Abrir con» (`OpenWithProgids`), nunca
  robar el asociado por defecto.
- **El AppUserModelID de los accesos directos NSIS usa `${AUMID}`
  ("CanvasDesktop.App"), no `${IDENTIFIER}`** (el identificador de
  empaquetado, `com.canvas-desktop.CanvasDesktop`, una cadena distinta): si
  no coincide exactamente con lo que `set_app_user_model_id()` pasa a
  `SetCurrentProcessExplicitAppUserModelID`, la Jump List de la barra de
  tareas se parte en dos entradas.
- **Iconos generados con el propio árbol de dependencias**
  (`cargo run -p canvas-render --example gen_icons`), no con Inkscape ni
  ImageMagick: rasteriza `assets/icon.svg` con `resvg`/`tiny-skia` (ya en el
  árbol para exportar SVG) y escribe `.ico`/`.icns` a mano (formatos simples,
  contenedores con PNG embebido) — repetible sin instalar nada al cambiar el
  arte definitivo.
- **`aarch64-pc-windows-msvc` no compila en todas las máquinas de
  desarrollo**: enlaza con `link.exe` solo si Visual Studio tiene el
  componente "MSVC ARM64 build tools"; confirmado que falla sin él. El
  workflow de release lo trata como best-effort (`continue-on-error`), nunca
  bloquea el release x64.
- **Subsistema GUI de Windows solo en release**
  (`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` en
  `crates/canvas-app/src/main.rs`): sin esto el binario se enlaza con el
  subsistema console por defecto de Rust y Windows le abre una consola negra
  detrás de la ventana al lanzarlo desde el Explorador (acceso directo del
  instalador). Condicionado a `not(debug_assertions)` para no perder la
  consola con `cargo run` en desarrollo. No rompe
  `packaging/windows/installer.nsi`: `nsExec::ExecToLog` lanza
  `--register-shell`/`--unregister-shell` con sus propias tuberías de
  stdout/stderr, que se heredan igual sea cual sea el subsistema del `.exe`.
