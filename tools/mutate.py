"""Comprueba que los tests nuevos FALLAN si se rompe el codigo que cubren.

Aplica una mutacion, deja el arbol como estaba y reporta. Uso:
    python .refactor-tools/mutate.py apply <nombre>
    python .refactor-tools/mutate.py restore
"""
import io
import os
import shutil
import sys

CAM = 'crates/canvas-app/src/editor/canvas/camera.rs'
VP = 'crates/canvas-app/src/editor/viewport.rs'
BACKUP = '.refactor-tools/.mutation-backup'

MUTATIONS = {
    # El atajo mas especifico deja de ir primero: Ctrl+0 se come Ctrl+Alt+0.
    'shortcut-order': (CAM, '    let fit_all = ui.ctx().input_mut(|i| {',
                       '    let fit_all_MOVED = ui.ctx().input_mut(|i| {'),
    # La rueda al reves que el resto de la app.
    'wheel-sign': (CAM, '            state.viewport.pan += delta;',
                   '            state.viewport.pan -= delta;'),
    # El zoom deja de compensar el pan: el punto bajo el cursor se mueve.
    'zoom-anchor': (VP, '        self.pan = anchor - (anchor - self.pan) * applied as f32;',
                    '        let _ = (anchor, applied);'),
    # Se pierde el umbral sub-pixel de note_size.
    'resize-threshold': (VP, '            (self.last_avail.x - avail.x).abs() > 0.5 || (self.last_avail.y - avail.y).abs() > 0.5;',
                         '            self.last_avail != avail;'),
}


def files():
    return sorted({m[0] for m in MUTATIONS.values()})


def restore():
    for f in files():
        b = os.path.join(BACKUP, os.path.basename(f))
        if os.path.exists(b):
            shutil.copyfile(b, f)


def apply(name):
    os.makedirs(BACKUP, exist_ok=True)
    for f in files():
        b = os.path.join(BACKUP, os.path.basename(f))
        if not os.path.exists(b):
            shutil.copyfile(f, b)
    restore()
    path, old, new = MUTATIONS[name]
    s = io.open(path, encoding='utf-8', newline='').read().replace('\r\n', '\n')
    assert old in s, (path, old)
    io.open(path, 'w', encoding='utf-8', newline='\n').write(s.replace(old, new, 1))


if __name__ == '__main__':
    if sys.argv[1] == 'restore':
        restore()
    else:
        apply(sys.argv[2])
    print('ok')
