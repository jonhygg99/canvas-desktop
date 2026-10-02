# Auditoría de Canvas Desktop

Fecha: 2026-10-02. Base Git: `75aea1280648f9f4f1cd20c27faa8347e646644f`, incluyendo los cambios locales presentes al comenzar.

## Resultado

Se identificaron **13 hallazgos prioritarios**. Los dos fallos comunicados tienen mecanismos reproducibles: una compensación de cámara que omite el zoom y una selección de Serper que reutiliza la identidad de un checkbox tras recolocar las tarjetas. Además, existen riesgos de pérdida de capas, cambios marcados incorrectamente como guardados, navegación que descarta documentos sucios e importaciones simultáneas que sobrescriben archivos.

Esta entrega es una auditoría: no se han aplicado correcciones al código de producción ni alterado los cambios locales previos.

### Alcance y nivel de evidencia

Revisión dirigida de los cinco crates, con mayor profundidad en `canvas-app`: Gallery → Editor, geometría de la baraja, cámara, Serper, operaciones asíncronas, guardado, navegación, ajustes y CI. Se inspeccionaron también historial y comandos del núcleo, escritura atómica, límites de sidecars, presupuesto GPU e integración de instancia única.

- **Reproducido:** ejecución de un caso controlado que muestra el fallo.
- **Comprobado en código:** el flujo contiene el defecto; falta ensayar el escenario completo en la aplicación.
- **Riesgo condicionado:** depende del contenido externo, de concurrencia o de presión de recursos; no se ha medido su frecuencia.

Las reproducciones fueron headless, con egui real y funciones originales importadas desde el repositorio. Para la cámara se usó una baraja mínima compatible; para Serper se reprodujo el patrón de widgets del selector junto al algoritmo original de reparto. **No equivalen a una prueba de la ventana nativa ni a una sesión real con Serper.** No se consumieron créditos de la API. No se ejecutaron pruebas GPU ni pruebas de macOS/Linux, ni un escaneo de vulnerabilidades de dependencias.

Las referencias `archivo:línea` corresponden al estado auditado y permiten localizar cada evidencia.

## Prioridades

P1 = corregir antes de una publicación o uso con documentos importantes. P2 = siguiente ciclo de estabilidad y calidad. No se ha acreditado ningún P0.

| ID | Prioridad | Hallazgo | Evidencia |
| --- | --- | --- | --- |
| A01 | P1 | El recálculo de la baraja desplaza la cámara al omitir el zoom | Reproducido |
| A02 | P1 | Un checkbox de Serper puede seleccionar otra foto tras un reflow | Reproducido |
| A03 | P1 | Un fallo al guardar el sidecar se comunica como éxito | Comprobado en código |
| A04 | P1 | Editar durante un guardado puede marcar cambios posteriores como guardados | Comprobado en código |
| A05 | P1 | Volver a Gallery puede descartar cambios en lienzos no activos | Comprobado en código |
| A06 | P1 | Dos importaciones bulk pueden sobrescribir el mismo archivo | Comprobado en código |
| A07 | P1 | Las inserciones asíncronas pueden terminar en otro documento | Comprobado en código |
| A08 | P2 | El rescate social puede sustituir la imagen elegida por otra | Riesgo condicionado |
| A09 | P2 | «Bring more» puede mezclar consultas diferentes | Comprobado en código |
| A10 | P2 | Los ajustes pueden persistirse en orden inverso | Comprobado en código |
| A11 | P2 | Miniaturas y lienzos bulk carecen de un presupuesto conjunto de recursos | Riesgo condicionado |
| A12 | P2 | El job GPU de CI no tiene un disparador que permita ejecutarlo | Comprobado en código |
| A13 | P2 | El Rust mínimo declarado es incompatible con las dependencias | Comprobado en manifests |

## Los dos fallos comunicados

### A01 — Gallery abre el documento correcto, pero la cámara termina lejos

**Evidencia:** `crates/canvas-app/src/editor/canvas/layout.rs:32–40`, `editor/canvas/mod.rs:108–113`, `editor/viewport.rs:24–28` y `deck/model.rs:118–134`.

La baraja usa dimensiones provisionales hasta recibir sondas o documentos cargados. Si cambian los tamaños de lienzos anteriores al activo, cambia también el origen del activo. `sync_deck_layout` intenta mantenerlo fijo en pantalla:

```rust
let delta = (after.0 - before.0, after.1 - before.1);
*pan -= egui::vec2(delta.0 as f32, delta.1 as f32);
```

Pero `delta` está en píxeles de documento y `pan` en puntos de pantalla. La conversión página → pantalla sí multiplica por `viewport.zoom`. La compensación debe usar ese mismo factor. La firma actual ni siquiera recibe el zoom.

**Reproducción ejecutada:** cinco ranuras verticales; la quinta activa mide 800×600; las cuatro anteriores empiezan estimadas en 1600×1600. Después llegan tamaños reales de 800×600, con `needs_fit=false` y zoom 0,25. Se ejecutaron `Deck::relayout` y `sync_deck_layout` originales:

```text
old_y=6592, new_y=2496, zoom=0.25
pan_delta=4096, screen_drift=3072
```

La compensación correcta es +1024, pero se aplican +4096. El lienzo activo se desplaza **3072 puntos de pantalla** hacia abajo. Con zoom 1 el defecto queda oculto; con carga rápida de sondas también puede no verse. Esto explica la intermitencia y el síntoma de acabar varios lienzos por debajo.

**Corrección propuesta:** pasar el zoom a la sincronización y compensar `delta * zoom`. Separar cambios de origen de cambios de tamaño; cuando esté armado el ajuste automático, revisar si corresponde repetir el fit del activo o de toda la baraja. Aplicar el mismo criterio a ambos ejes.

**Prueba de regresión necesaria:** posición en pantalla del activo invariante antes/después de sondas tardías, con zoom 0,25 / 0,5 / 1 / 2, pila vertical y horizontal, destino lejano y tamaños mixtos. Añadir navegación repetida Gallery → Editor con sondas que lleguen después del primer fit.

### A02 — «Select web images» puede cambiar la selección de otra tarjeta

**Evidencia:** `crates/canvas-app/src/serper/bulk.rs:109–132,153–191`, `serper/bulk_layout.rs:30–45,51–65` y `app/messages/serper.rs:95–100`.

Las alturas se vuelven a calcular con las miniaturas recibidas y las fotos se redistribuyen entre columnas **cada frame**. Además, el filtro posterior a la descarga puede retirar una foto y desplazar el resto. El clic de imagen tiene un ID estable `("bulk_img", photo.id)`, pero el checkbox se crea sin un `push_id` asociado a la foto: su identidad depende del lugar que ocupa en el árbol de widgets.

Si una tarjeta cambia de columna o de posición entre pulsar y soltar, un checkbox distinto puede heredar la identidad del que recibió la pulsación.

**Reproducción ejecutada:** egui 0.35 real, cuatro frames, cuatro fotos A/B/C/D y dos columnas. Primero alturas `[100,100,100,100]`, reparto `[[A,C],[B,D]]`. Se pulsa el checkbox de D. Antes de soltar llega una altura de 400 para A, cambiando el reparto a `[[A],[B,C,D]]`. La C ocupa el lugar anterior de D y recibe su ID automático:

```text
pressed image index 3 (D), toggled=[2]
selected=[false, false, true, false]
```

**Control de la propuesta:** repitiendo el caso con un ID ligado a cada foto, C deja de seleccionarse. En ese caso egui cancela el clic al desplazarse D; no se debe asumir que conservar IDs garantiza que una tarjeta móvil complete el clic.

**Corrección propuesta:** envolver la celda entera en `ui.push_id(("bulk_cell", photo.id), ...)`, incluyendo checkbox y Retry. Mantener estable el reparto mientras se interactúa: fijar columnas al abrir, reservar la geometría con un aspecto coherente o aplazar recolocaciones y retiradas hasta finalizar el gesto. Asociar cada gesto a la foto que recibió la pulsación.

**Prueba de regresión necesaria:** pulsar/soltar checkbox e imagen mientras llegan miniaturas, se elimina un banner o cambia el ancho de ventana. Verificar que nunca cambia una foto distinta de la pulsada; comprobar también teclado y Retry. Las pruebas existentes de reparto y selección inicial no cubren esta secuencia temporal.

## Otros bugs y riesgos

### A03 — Se informa de guardado correcto aunque fallen las capas editables

**Evidencia:** `crates/canvas-app/src/loader/save_ops.rs:118–133` y `app/messages/save.rs:70–79,110–119`.

Tras escribir la imagen, `write_sidecar` puede fallar. El error solo se registra como warning y la operación devuelve `Ok(())`. La UI marca el historial como guardado y puede cerrar o navegar. Al reabrir, las capas no están actualizadas o solo queda la imagen plana.

**Escenario:** bloquear el destino `.canvas`, hacer que sea una ruta no escribible o provocar falta de espacio para el segundo archivo. Guardar con sidecar habilitado. La imagen puede quedar actualizada mientras el proyecto editable falla silenciosamente.

**Propuesta:** devolver un resultado explícito de guardado parcial, mantener el documento pendiente y presentar el fallo al usuario. No cerrar ni navegar automáticamente cuando falte la parte editable. Diseñar recuperación de la pareja imagen + sidecar: la atomicidad individual de cada archivo no convierte la pareja en una transacción.

**Validación:** inyectar fallo exclusivamente en el sidecar, verificar banner, dirty, ausencia de cierre automático y reintento sin pérdida del documento en memoria.

### A04 — Cambios hechos después de iniciar el guardado quedan falsamente «guardados»

**Evidencia:** `crates/canvas-app/src/app/persistence.rs:282–299`, `app/views/editor/panels.rs:149–172`, `editor/canvas/mod.rs:168–170`, `app/messages/save.rs:75` y `crates/canvas-core/src/command/history.rs:145–152`.

El worker recibe una captura del documento. Mientras escribe, los paneles y el lienzo siguen habilitados: su guardia es `locked`, no `saving`. Cuando llega `Saved`, `history.mark_saved()` marca la profundidad **actual**, que puede incluir ediciones posteriores a la captura. Esas ediciones no están en disco, pero dejan de aparecer como pendientes. El bloqueo de `apply_jump` durante un guardado protege el cambio de ranura, no estas ediciones.

**Propuesta:** identificar la revisión capturada y marcar como guardada esa revisión concreta. No basta comparar longitudes de undo si puede haber undo y una rama nueva con igual profundidad. Usar una identidad de revisión; si el documento cambió, conservar dirty y no ejecutar un cierre diferido sin resolver esos cambios.

**Validación:** retrasar la escritura usando el gancho existente `CANVAS_IO_TEST_SLEEP_BEFORE_REPLACE_MS`, editar durante la espera, recibir `Saved` y comprobar que la edición nueva sigue pendiente y que reabrir muestra solo la captura escrita.

### A05 — Volver a Gallery no considera los lienzos sucios de fondo

**Evidencia:** `crates/canvas-app/src/app/views/editor/mod.rs:65–105`, `app/ws_frame.rs:184–185`, `app/navigation.rs:264–301` y `app/views/gallery.rs:29–34`.

El botón de retorno consulta únicamente `state.is_dirty()` del activo. El flujo general `request_nav` sí consulta `ws.dirty_canvas_names()`, pero el retorno no pasa por él. Si se editó A, se pasó a B limpio y se vuelve a Gallery, se navega sin aviso. Al abrir otro archivo desde esa galería, una semilla nueva sustituye la baraja y desaparece el documento sucio de A.

**Propuesta:** centralizar todas las salidas del editor en la política de navegación del workspace; incluir los documentos de fondo y ofrecer guardar todos, descartar o cancelar. Evitar un diálogo de guardado diferente para el botón de retorno.

**Validación:** A sucio → B limpio → Gallery → abrir C. Debe requerir resolver los cambios de A y conservarlos al cancelar. Cubrir también varios documentos sucios y fallo de uno al guardar.

### A06 — Los nombres del bulk no se reservan atómicamente

**Evidencia:** `crates/canvas-app/src/loader/serper_ops.rs:251–268,308–315` y `crates/canvas-io/src/load.rs:92–124`.

`free_bulk_path` comprueba `exists()` y devuelve un nombre sin reservarlo. Dos ventanas que importan la misma keyword a la misma carpeta pueden obtener el mismo nombre. Después `save_rgba` sustituye el destino y una tanda sobrescribe la otra; el sidecar puede pertenecer a una captura distinta. El repositorio ya tiene `reserve_unique_path` con `create_new` para evitar esta carrera.

**Propuesta:** reservar el nombre con el helper existente antes de escribir, mantener la reserva durante la operación y limpiar solo las reservas propias si falla. Considerar también un sidecar previo sin imagen hermana.

**Validación:** sincronizar dos workers tras elegir/reservar sus destinos y verificar rutas distintas, imágenes intactas y sidecars correspondientes.

### A07 — Una descarga puede insertar la imagen en otro documento

**Evidencia:** `crates/canvas-app/src/loader/serper_ops.rs:108–119`, `app/messages/serper.rs:109–135`, `editor/state/mod.rs:239–251,254–301` y `app/messages/load.rs:105–145`.

El mensaje de Serper identifica la foto, pero no el documento ni la ranura destino. Su handler inserta en cualquier `View::Editor` activo y limpia `inserting` sin comprobar siquiera que la respuesta coincida con la petición pendiente. `is_idle()` no bloquea el salto por una inserción Serper en vuelo; el panel permanece compartido al intercambiar ranuras.

**Escenario:** empezar a insertar en A, cambiar a B antes de terminar la descarga y recibir la respuesta. La imagen acaba en B. Si se abrió otro proyecto en el mismo workspace, también puede insertarse allí. Las cargas locales de capas y reemplazos tienen un problema de identidad relacionado.

**Propuesta:** transportar identidad de documento, generación, ranura y petición. Aplicar el resultado al destino original o descartarlo explícitamente si ya no existe. No usar solo la URL como identidad de petición. Validar antes de limpiar flags o consumir `pending_drop`.

**Validación:** respuesta tardía tras salto, cambio de proyecto, cierre/reapertura y dos solicitudes con la misma URL.

### A08 — La miniatura y el rescate social pueden representar fotos diferentes

**Evidencia:** `crates/canvas-app/src/serper/types.rs:158–165`, `serper/api.rs:452–485` y `serper/state.rs:314–324`.

La miniatura puede venir del proxy de Google, mientras la inserción descarga `image_url`. Si esa URL social devuelve HTML, `fetch_image` puede recuperar el `og:image` de la página o de `source_url` y devolverlo como si fuera la foto seleccionada. En una página de perfil o con varias fotos, esa imagen puede ser otra. No hay verificación visual ni confirmación de sustitución.

Este mecanismo es distinto del fallo del checkbox A02: afecta a **qué imagen se descarga**, aunque el ID seleccionado sea correcto. No se verificó con una respuesta real de Serper durante la auditoría.

**Propuesta:** devolver la URL realmente resuelta y el método de resolución. Ante una sustitución, mostrar la nueva preview o requerir una elección explícita; conservar la miniatura seleccionada como alternativa identificada cuando corresponda. La documentación promete un fallback a `thumb_url` que la función actual no recibe ni ejecuta. Además, un error de la descarga inicial sale por `?`, sin recorrer el rescate.

**Validación:** fixture con miniatura A y página social cuyo `og:image` es B; la inserción no debe aceptar B silenciosamente. Añadir respuestas 403/404 y HTML válido sin imagen.

### A09 — La paginación usa el texto editable, no la consulta de los resultados

**Evidencia:** `crates/canvas-app/src/serper/panel.rs:95–104,199–228` y `serper/state.rs:274–293`.

Después de buscar A, el usuario puede escribir B sin pulsar Search. «Bring more» usa `panel.query` actual, pide la página siguiente de B y la añade a los resultados de A. Cambiar bloqueados o presupuesto entre páginas puede producir una mezcla equivalente.

**Propuesta:** guardar una especificación inmutable de la búsqueda activa —keyword, modo, filtros y tamaño de página— y paginar sobre ella. Mantener separado el borrador de texto. Reiniciar la búsqueda al aplicar cambios que alteren esa especificación.

**Validación:** buscar A, editar el input a B, pedir más y comprobar que se pagina A o se inicia B desde página 1; nunca mezclar ambos.

### A10 — Persistencia de ajustes sin orden entre snapshots

**Evidencia:** `crates/canvas-app/src/settings/mod.rs:172–192`.

Cada cambio lanza un hilo con un snapshot completo. Si la escritura del snapshot antiguo termina después de la del nuevo, sustituye los ajustes más recientes. `write_atomic` evita archivos parciales, pero no evita escrituras fuera de orden. Puede afectar al tema, workspaces restaurados, preferencias y contador de créditos.

**Propuesta:** un escritor serial con revisiones monotónicas y agrupación de cambios rápidos. Al salir, esperar la persistencia de la última revisión. Un mutex de escritura sin ordenar las revisiones no resuelve por sí solo cuál snapshot gana.

**Validación:** forzar que la revisión antigua se retrase; persistir dos cambios y comprobar que al reiniciar se conserva la revisión nueva completa.

### A11 — El bulk puede disparar demasiadas descargas y asignaciones grandes

**Evidencia:** `crates/canvas-app/src/serper/bulk.rs:107–144`, `serper/state.rs:235–247`, `loader/serper_ops.rs:85–102,224–239`, `serper/api.rs:517–525` y `serper/bake.rs:41–55`.

El bulk reclama hasta 12 miniaturas **por frame**, recorriendo toda la lista. Ese límite no controla el número simultáneo de descargas: puede lanzar los resultados enteros en pocos frames aunque estén fuera de pantalla. Cada descarga decodifica RGBA y conserva una textura. No hay presupuesto conjunto de CPU/GPU para estas miniaturas.

Además, en `BatchMax` se confían las dimensiones de la API para asignar el lienzo: se toma el máximo ancho y alto, sin techo de píxeles o bytes. Un lienzo 20000×20000 requiere aproximadamente **1,49 GiB solo para un buffer RGBA**, antes de fondos, copias y codificación. `catch_unwind` no garantiza recuperación frente a un aborto por falta de memoria.

**Propuesta:** cola de descarga con límite de trabajos en vuelo, visibilidad y cancelación; miniaturas reducidas antes de subirlas a GPU; límites explícitos de dimensiones, píxeles y bytes para importaciones y bake. Calcular el presupuesto antes de asignar buffers y comprobar multiplicaciones. Reutilizar la disciplina de límites aplicada a PNGs de sidecars, sin asumir que los límites implícitos del decoder cubren todo el flujo.

**Validación:** 200 resultados con respuestas lentas, scroll rápido y cierre del selector; medir pico de hilos y memoria. Rechazar dimensiones enormes en un test de validación, sin intentar materializar la imagen.

### A12 — El job de pruebas GPU no puede activarse con el workflow actual

**Evidencia:** `.github/workflows/ci.yml:3–6,94–101`.

El job `gpu-bake` requiere `github.event_name == 'workflow_dispatch'`, pero `on` solo declara `push` y `pull_request`. El evento necesario no está habilitado. El comentario que indica «Actions → Run workflow» no coincide con la configuración.

**Propuesta:** añadir `workflow_dispatch` y una entrada explícita para solicitar GPU, comprobar disponibilidad del runner y mantener visible la diferencia entre pruebas compiladas, omitidas y ejecutadas. El guard de número de tests no mide cobertura de comportamientos ni ejecuta los casos GPU ignorados.

**Validación:** validar el YAML y lanzar manualmente el job en un runner GPU conectado; comprobar el número real de pruebas ejecutadas y que no se omiten.

### A13 — El MSRV declarado no permite compilar la aplicación

**Evidencia:** `Cargo.toml:14` declara `rust-version = "1.85"`. Los manifests de los paquetes resueltos, inspeccionados directamente en el registro local de Cargo, declaran `rust-version = "1.92"` para `egui 0.35.0` y `"1.87.0"` para `wgpu 29.0.4`.

La aplicación no puede construirse con el mínimo anunciado por el workspace. La CI usa `stable`, por lo que puede mantenerse verde sin detectar esa incompatibilidad. El mínimo efectivo es **al menos 1.92**; falta verificar si alguna otra dependencia lo eleva.

**Propuesta:** determinar y declarar el MSRV real del conjunto resuelto, y comprobarlo en CI con `--locked`. Alinear las instrucciones de desarrollo y publicación. No bajar versiones de GPU/UI de forma independiente para cumplir el número antiguo.

**Validación:** compilar en CI con la versión mínima elegida y mantener un segundo check con stable. No se ejecutó una build con Rust 1.85; la incompatibilidad se acredita con los requisitos de las dependencias.

## Áreas de mejora

1. **Identidad asíncrona común.** Usar un contrato de mensajes con documento/generación/ranura/petición/revisión. Las respuestas de carga de baraja ya validan generación y carpeta; extender ese patrón a inserciones, diálogos, guardados y bulk.
2. **Política única de navegación y persistencia.** Centralizar el tratamiento de documentos sucios y de guardados parciales. El botón Gallery mantiene lógica propia y un diálogo síncrono (`app/views/editor/mod.rs:72–84`); revisar ese diálogo y el de restauración (`app/messages/load.rs:48–56`) para no bloquear el event loop multi-ventana mientras el usuario responde.
3. **Pruebas de interacción con tiempo.** Añadir frames de pulsación/espera/release, respuestas tardías y workers con barreras controladas. Los tests matemáticos y de un solo frame no detectan reutilización de IDs ni contaminación entre documentos.
4. **Geometría con unidades explícitas.** Distinguir coordenadas de documento y pantalla mediante helpers o tipos. La corrección A01 debe preservar un punto o rect en pantalla, no restar cantidades que parecen similares.
5. **Presupuesto de recursos de principio a fin.** Incluir descargas, RGBA decodificado, thumbnails, texturas, captura de guardado y buffers temporales. El presupuesto actual de baraja/FX no cubre todos esos componentes.
6. **Observabilidad útil para estos bugs.** Registrar petición, documento, ranura, revisión, origen anterior/nuevo, zoom y método de resolución de imagen. No registrar API keys. Reducir URLs firmadas en logs a información diagnóstica suficiente.
7. **CI reproducible.** Mantener `--locked`, fijar o verificar la toolchain de publicación y habilitar el job GPU. Separar checks de formato, compilación, tests CPU, interacción y GPU; evitar presentar un suelo de cantidad de tests como cobertura.
8. **Documentación alineada con el estado real.** Corregir la descripción del rescate de imágenes, del disparador GPU y del Rust mínimo (A13). Verificar el mínimo del conjunto completo de dependencias.

## Validación ejecutada

| Comprobación | Resultado |
| --- | --- |
| `cargo fmt --all -- --check` | Correcto, salida 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Primer intento interrumpido por `STATUS_ACCESS_VIOLATION` al compilar `windows` |
| `cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings` | Correcto, salida 0, sin warnings |
| `cargo test --workspace --locked` | No completado: linker MSVC devuelve `LNK1243`, sección COMDAT inválida/corrupta en un objeto de `libvello` |
| `cargo test -p canvas-core -p canvas-io -p canvas-shell --locked` | Primer intento interrumpido por `STATUS_ACCESS_VIOLATION` al compilar `windows` |
| `cargo test -p canvas-core --locked` | Correcto: 87 pruebas superadas, ninguna fallida |
| `cargo test -p canvas-core -p canvas-io -p canvas-shell --locked -j 2` | Correcto: 196 pruebas superadas, ninguna fallida (87 core, 85 I/O y 24 shell) |
| Reproducción headless de cámara | Fallo demostrado: desplazamiento de 3072 puntos |
| Reproducción headless del selector | Fallo demostrado: se pulsa D y se selecciona C |
| Control headless con IDs estables | C deja de seleccionarse; el gesto se cancela al desplazarse D |

Entorno: Windows, Rust 1.97.1, host `x86_64-pc-windows-msvc`. La toolchain 1.93 está instalada, pero no se ha usado para atribuir ni resolver los fallos del compilador/linker. El segundo clippy correcto sugiere que el primer bloqueo no es un diagnóstico del código de aplicación; no permite determinar por sí solo la causa del crash. Tampoco se puede atribuir `LNK1243` al código del proyecto sin investigar artefactos y toolchain.

Las reproducciones están en `%TEMP%/canvas-audit-20261002`, con `Cargo.toml`, `src/main.rs` y `probes-verified.log`. Se ejecutan con:

```powershell
cargo run --offline --manifest-path "$env:TEMP\canvas-audit-20261002\Cargo.toml"
```

El harness importa los módulos originales de layout y masonry por ruta absoluta; esa carpeta es evidencia local temporal, no una nueva suite mantenida por el proyecto. Para convertirla en regresiones permanentes, integrar los escenarios en los tests vecinos y afirmar el comportamiento corregido.

## Orden recomendado de trabajo

1. Corregir A01 y A02 con sus regresiones temporales y validar los gestos en la ventana nativa.
2. Resolver A03–A07 antes de confiar en guardados y navegación con varios documentos o ventanas.
3. Corregir identidad de imágenes y búsqueda activa (A08–A09), persistencia ordenada (A10) y límites de recursos (A11).
4. Habilitar las pruebas GPU (A12), alinear el MSRV (A13), resolver la validación completa de la toolchain y ensayar Gallery/Serper con datos reales.

La auditoría identifica problemas concretos, pero no certifica ausencia de otros bugs: queda pendiente ejecutar la suite completa, la interacción nativa y las pruebas GPU.
