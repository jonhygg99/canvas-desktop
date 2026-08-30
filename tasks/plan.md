# Plan: el bloqueo de guardado se dispara en falso en macOS

Supersede al plan anterior (Fases 1-3, cerrado en `8534713`). Estado del
trabajo nuevo en `tasks/todo.md`.

## Overview

Con solo la app y el chat abiertos, la barra de estado marca RAM
«crítica» y el guardado se bloquea al añadir algo al proyecto. El
diagnóstico (medido en la máquina del usuario) demuestra que **no es ni
un ordenador malo ni una app que consuma mucho**: es un error de medición
de la RAM libre en macOS.

| Señal | Valor medido | Interpretación |
|---|---|---|
| RAM total | 16 GiB | máquina normal |
| Presión oficial macOS (`kern.memorystatus_vm_pressure_level`) | 1 (NORMAL) | el OS no ve presión |
| `memory_pressure -Q` | 78 % libre | sistema holgado |
| Swap usado | 0 B (nunca ha paginado) | no hay hambre de memoria |
| RSS de canvas-desktop con carpeta abierta | 0.22 GiB | la app consume poco |
| RSS de Freebuff (chat) | ~1.1 GiB | normal |
| Métrica de la app (`free_ram_bytes`) | 0.39 GiB → «CRÍTICA» | **el bug** |

**Causa raíz:** `detect_free_ram_bytes` en macOS suma solo
`vm.page_free_count + vm.page_speculative_count + vm.page_purgeable_count`
y **excluye la caché de archivos reclamable** (páginas inactivas, ~6 GiB
en esta máquina según `vm_stat`: `Pages inactive: 1641824`). El OS
considera esa caché reclamable para decidir presión (por eso dice 78 %
libre), así que la app marca «crítico» mientras el sistema está normal.
Linux (`MemAvailable`) y Windows (`ullAvailPhys`) ya incluyen la caché
reclamable: el bug es solo de macOS.

## Architecture Decisions

- **Medir la RAM libre en macOS como el OS**: free + speculative +
  purgeable + **inactive**, vía `host_statistics64(HOST_VM_INFO64)` — la
  misma fuente que `vm_stat`. `vm.page_inactive_count` no existe como
  sysctl (verificado), pero sí como campo de `host_statistics64`, y `libc`
  ya expone `mach_host_self`/`host_statistics64` (sin dependencias nuevas).
- **Nivel oficial de presión como red de seguridad**: leer
  `kern.memorystatus_vm_pressure_level` (1 normal / 2 warning / 4
  critical). Si el OS dice warning o critical, la medición en bytes no
  puede contradecirlo: se reporta por debajo del umbral correspondiente
  (2 GiB / 512 MiB). Evita que contar inactive como libre enmascare
  presión real en picos.
- **Linux y Windows no se tocan**: ya miden lo reclamable.
- **Nunca bloquear por la propia memoria**: en presión crítica REAL, un
  Save/Export evicta primero las cachés propias (baraja + scopes FX) y
  reintenta una vez antes de mostrar el error — la app no debe bloquear al
  usuario por memoria que ella misma retiene.

## Task List

### Task 1: Medir en macOS la RAM libre reclamable (S)

**Description:** Reemplazar la suma de tres oids por `host_statistics64`
(free + speculative + purgeable + inactive) con red de seguridad del
nivel oficial de presión.

**Acceptance criteria:**
- [ ] En esta máquina (nivel 1, ~6 GiB inactivos) `free_ram_bytes()` reporta > 2 GiB.
- [ ] Con nivel oficial 4, la medición reporta < 512 MiB aunque haya páginas inactivas.
- [ ] Syscall fallida → `None` (sin regresión).

**Verification:**
- [ ] `cargo test -p canvas-app` verde.
- [ ] `cargo clippy -p canvas-app --all-targets -- -D warnings` limpio.
- [ ] Manual: abrir la app y comprobar la barra de estado en estado normal.

**Dependencies:** None

**Files likely touched:**
- `crates/canvas-app/src/deck/system.rs`

**Estimated scope:** Small (1-2 files)

### Task 2: Tests de tabla del contrato (S)

**Description:** Extraer el cálculo a una función pura (sin hardware) y
fijar con tabla: mucha caché inactiva no es crítico; nivel oficial 4 sí
lo es; el umbral de 2 GiB se respeta; `None` no es crítico.

**Acceptance criteria:**
- [ ] Tabla que cubre: libre alto/inactiva alta → normal; libre baja +
  inactiva alta → normal (el caso del usuario); nivel 2 → reducido;
  nivel 4 → crítico; `None` → no crítico.
- [ ] Los tests no dependen del hardware (funciones puras).

**Verification:**
- [ ] `cargo test -p canvas-app deck::` verde.

**Dependencies:** Task 1

**Files likely touched:**
- `crates/canvas-app/src/deck/system.rs` (tests)

**Estimated scope:** Small

### Task 3: Evictar cachés propias antes de bloquear un Save/Export (M)

**Description:** Cuando el gate de RAM crítica se dispare en un
Save/Export, evictar primero la caché de la baraja (`Deck::evict`) y los
scopes FX GPU (`evict_fx_to_budget` con presupuesto mínimo), re-medir, y
solo bloquear si sigue crítico. La app nunca se bloquea a sí misma.

**Acceptance criteria:**
- [ ] Con RAM crítica real, un save dispara la evicción y solo muestra el
      banner si tras liberar sigue crítico.
- [ ] El archivo nunca se toca si el bloqueo final se mantiene.
- [ ] Sin presión, el flujo de guardado no cambia (sin evicción).

**Verification:**
- [ ] `cargo test -p canvas-app` verde; clippy/fmt limpios.
- [ ] Manual con memhog real: save bajo presión crítica evicta y, si
      libera suficiente, guarda.

**Dependencies:** Tasks 1-2

**Files likely touched:**
- `crates/canvas-app/src/app/persistence.rs`
- `crates/canvas-app/src/deck/mod.rs` (si falta exponer la evicción)

**Estimated scope:** Medium (3-5 files)

### Task 4: Verificación UI real en la carpeta de Julián Gil (XS)

**Description:** Abrir la carpeta real del usuario
(`…/Material Youtube/1. Chismes MX/21. Alejandra Jaramillo/1. IMG/11. Julián Gil`),
añadir una imagen al proyecto y guardar/exportar.

**Acceptance criteria:**
- [ ] La barra de estado muestra RAM normal (no crítica) con la carpeta abierta.
- [ ] Añadir una imagen y Cmd+S guarda sin banner de bloqueo.
- [ ] Exportar produce un PNG completo (no blanco).

**Verification:**
- [ ] Manual, con la carpeta real.

**Dependencies:** Tasks 1-3

**Files likely touched:** none (verificación)

**Estimated scope:** XS

### Checkpoint: Final
- [ ] `cargo test --workspace` verde (≥463), clippy `-D warnings`, fmt OK.
- [ ] Verificación en la carpeta real del usuario: guardado permitido.
- [ ] Documentar en CLAUDE.md el matiz de la métrica macOS (red de
      seguridad por nivel oficial).

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Contar inactive como libre enmascara presión real | Med | El nivel oficial del OS (warning/critical) actúa de techo duro |
| `kern.memorystatus_vm_pressure_level` ausente en macOS antiguo | Bajo | Syscall fallida → caer a la medición de páginas sola (ya correcta con inactive) |
| `host_statistics64` falla | Bajo | Devolver `None` como hoy (los llamadores ya caen al histórico) |
| La evicción previa al bloqueo libera poco | Med | El guard solo se bloquea si SIGUE crítico tras evictar; el mensaje explica qué hacer |

## Open Questions

- ¿Debe la barra de estado mostrar también el nivel oficial de presión
  (p.ej. «presión del sistema: normal»)? Nicety, no bloqueante.
