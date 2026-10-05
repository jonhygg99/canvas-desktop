# Releases rápidos y verificables

## Uso habitual

1. Incrementa `workspace.package.version` en `Cargo.toml`, actualiza `Cargo.lock`
   con `cargo check --workspace` y escribe las novedades en `.github/release-body.md`.
2. Haz commit, pasa CI y mergea a `main`. Sincroniza tu checkout de `main`.
3. Ejecuta desde PowerShell:

   ```powershell
   ./scripts/release.ps1 -CheckOnly
   ./scripts/release.ps1
   ```

El script comprueba `main`, cambios pendientes, sincronización con el remoto y
que la versión no tenga ya un tag. Crea un tag anotado `vX.Y.Z` y lo envía;
el workflow **Release** se encarga del resto. Requiere Git, Cargo y GitHub CLI
autenticado (`gh auth login`). No incrementa la versión ni hace merge por ti.

También puedes crear y enviar manualmente el tag anotado. Los controles de
seguridad se ejecutan igualmente en Actions.

## Qué sucede automáticamente

- **Prepare release** compila Windows x64 en cada push a `main`, prueba el
  instalador y conserva el artefacto 30 días. No publica releases.
- **Release** comprueba que el tag coincida con Cargo, que el commit pertenezca
  a `main` y que la CI de ese SHA en `main` haya terminado correctamente.
  Espera hasta 30 minutos si la CI sigue ejecutándose; un fallo bloquea la publicación.
- Si hay un instalador preparado del mismo SHA, lo reutiliza. Si no existe,
  está caducado o todavía se está preparando, compila y prueba uno nuevo.
- La prueba Windows instala en un directorio temporal del runner, verifica
  ejecutable, versión y asociación «Abrir con», y comprueba la desinstalación.
  Solo se permite en GitHub Actions; **no la ejecutes en tu equipo**.
- Antes de publicar se verifica la attestation: repositorio, workflow firmante
  y SHA del código deben coincidir. Se generan hashes SHA-256 por plataforma.
- Windows se publica primero. ARM64, macOS y Linux compilan en paralelo y se
  adjuntan después si tienen éxito. Un fallo opcional no retira Windows, pero
  el workflow queda en rojo para que el fallo sea visible.
- El release se crea como draft y se promueve después de subir los assets.
  Un release publicado no se sobrescribe; publica una versión nueva.

La mejora mayor se obtiene cuando el build de `main` ya terminó antes de crear
el tag: no se vuelve a compilar Windows. La primera ejecución o un cambio de
dependencias/toolchain sigue requiriendo una compilación completa.

## Recuperación

- **CI fallida:** corrige el problema o reejecuta CI si fue un fallo externo;
  después reejecuta el workflow de release. No muevas un tag ya publicado.
- **Preparación fallida/caducada:** Release usa el build de respaldo. También
  puedes ejecutar manualmente **Prepare release** sobre `main`.
- **Fallo al publicar un draft:** reejecuta los jobs fallidos. El publicador
  completa el draft antes de promoverlo.
- **Fallo en una plataforma opcional:** reejecuta los jobs fallidos. Los assets
  idénticos se conservan; si cambian los bytes de un asset publicado, se rechaza
  el reemplazo. Usa una versión nueva para publicar bytes distintos.
- **Reintento manual completo:** selecciona el tag como ref del workflow, no
  `main`, y pasa el mismo tag como entrada:

  ```powershell
  gh workflow run release.yml --ref vX.Y.Z -f tag=vX.Y.Z
  ```

## Integridad y mantenimiento

Cada plataforma incluye `SHA256SUMS-<plataforma>.txt`. Para comprobar un paquete:

```powershell
Get-FileHash ./canvas-desktop_X.Y.Z_x64-setup.exe -Algorithm SHA256
gh attestation verify ./canvas-desktop_X.Y.Z_x64-setup.exe --repo jonhygg99/canvas-desktop
```

Las Actions de CI y release están fijadas por SHA; la toolchain de CI y
empaquetado por versión (`1.97.1`), conservando la comprobación MSRV con `1.92`.
Al actualizar una Action o Rust, revisa el cambio y valida
los workflows con actionlint. CI ejecuta las pruebas de los scripts y actionlint.
La caché de `cargo-packager` está separada por versión, SO y arquitectura.

Las attestations acreditan la procedencia; **no sustituyen la firma Authenticode
de Windows ni la firma/notarización de macOS**. Estos paquetes siguen sin firma
de editor. Configurar esos certificados requiere disponer de las credenciales.

En GitHub, configura reglas de `main` con CI obligatoria y reglas para `v*` que
impidan mover/borrar tags publicados. Estas reglas son ajustes del repositorio;
no quedan activadas por añadir los archivos del workflow.

Validación local: `python -m unittest discover -s scripts -p '*_tests.py'`,
`./scripts/release_tests.ps1` (comandos simulados) y actionlint.
La instalación real, las attestations y los tiempos de caché se
validan en la primera ejecución de **Prepare release** en GitHub Actions.
