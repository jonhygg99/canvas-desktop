# Todo — El bloqueo de guardado se dispara en falso en macOS

Plan completo en `tasks/plan.md` (supersede al plan de memoria anterior,
cerrado). Diagnóstico ya medido en la máquina del usuario: 16 GiB RAM,
presión oficial macOS NORMAL (nivel 1), 78 % libre, swap 0, app a
0.22 GiB RSS — la métrica de RAM libre en macOS excluye la caché de
archivos reclamable (~6 GiB) y marca «crítico» en falso.

## Task 1: Medir en macOS la RAM libre reclamable
- [ ] `host_statistics64(HOST_VM_INFO64)`: free + speculative + purgeable + inactive.
- [ ] Red de seguridad: nivel oficial `kern.memorystatus_vm_pressure_level`
      (2 → < 2 GiB, 4 → < 512 MiB) como techo duro.
- [ ] En esta máquina `free_ram_bytes()` > 2 GiB; syscall fallida → `None`.

## Task 2: Tests de tabla del contrato
- [ ] Función pura (sin hardware) con tabla: caché inactiva alta → normal
      (el caso del usuario); nivel 4 → crítico; nivel 2 → reducido;
      `None` → no crítico; umbral 2 GiB intacto.

## Checkpoint: Tasks 1-2
- [ ] `cargo test -p canvas-app deck::` verde; clippy/fmt limpios.

## Task 3: Evictar cachés propias antes de bloquear Save/Export
- [ ] En presión crítica real: evictar baraja + scopes FX, re-medir,
      reintentar una vez; solo bloquear si sigue crítico.
- [ ] Sin presión, el flujo de guardado no cambia.

## Checkpoint: Task 3
- [ ] `cargo test -p canvas-app` verde; clippy/fmt limpios.
- [ ] Manual con memhog: save bajo presión crítica evicta y guarda si libera.

## Task 4: Verificación UI real en la carpeta de Julián Gil
- [ ] Barra de estado en normal con la carpeta abierta.
- [ ] Añadir imagen + Cmd+S guarda sin banner; export produce PNG completo.

## Checkpoint: Final
- [ ] `cargo test --workspace` verde, clippy `-D warnings`, fmt OK.
- [ ] Documentar en CLAUDE.md el matiz de la métrica macOS.
