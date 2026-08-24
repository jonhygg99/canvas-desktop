"""Prueba, uno a uno, si un simbolo puede dejar de exportarse.

Lo quita del `pub use` de `lib.rs`, compila el workspace entero (incluidos
ejemplos y tests) y restaura. Si compila, el export sobra.
"""
import io
import re
import subprocess
import sys

CANDIDATES = [
    ('canvas-core', 'GroupContent'),
    ('canvas-core', 'SetOpacity'),
    ('canvas-core', 'SnapResult'),
    ('canvas-core', 'resize_from_corner'),
    ('canvas-io', 'IoError'),
    ('canvas-io', 'PREVIEW_MAX_DIM'),
    ('canvas-io', 'TextLineBreaker'),
    ('canvas-io', 'blank_design'),
    ('canvas-io', 'extract_metadata'),
    ('canvas-io', 'load_svg'),
    ('canvas-io', 'patch_orientation_to_1'),
    ('canvas-io', 'peek_unique_path'),
    ('canvas-io', 'read_preview'),
    ('canvas-io', 'reinject_metadata'),
    ('canvas-io', 'save_format_from_path'),
    ('canvas-io', 'trash_dir'),
    ('canvas-io', 'write_blank_canvas'),
    ('canvas-render', 'ColorParams'),
    ('canvas-shell', 'ShellError'),
    ('canvas-shell', 'ShellEvent'),
]


def lib(crate):
    return 'crates/' + crate + '/src/lib.rs'


def read(p):
    return io.open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')


def write(p, s):
    io.open(p, 'w', encoding='utf-8', newline='\n').write(s)


def drop_export(s, name):
    """Quita `name` de los `pub use` de lib.rs. None si no se pudo."""
    pat_list = re.compile(r'(pub use [\w:]+::\{)([^}]*)(\})', re.S)

    def repl(m):
        items = [x.strip() for x in m.group(2).split(',')]
        kept = [x for x in items if x and x.split(' as ')[0].strip() != name]
        if len(kept) == len(items):
            return m.group(0)
        return m.group(1) + ', '.join(kept) + m.group(3)

    out = pat_list.sub(repl, s)
    if out != s:
        return out
    out = re.sub(r'pub use [\w:]+::' + re.escape(name) + r';\n', '', s)
    if out != s:
        return out
    # declarado directo en lib.rs
    out = re.sub(r'^pub (fn|struct|enum|type|const|static) ' + re.escape(name),
                 r'pub(crate) \1 ' + name, s, flags=re.M)
    return out if out != s else None


def check():
    r = subprocess.run(
        ['cargo', 'check', '--workspace', '--all-targets', '--locked',
         '--message-format=short'],
        capture_output=True, text=True)
    return r.returncode == 0


results = []
for crate, name in CANDIDATES:
    p = lib(crate)
    original = read(p)
    changed = drop_export(original, name)
    if changed is None:
        results.append((crate, name, 'NO ENCONTRADO en lib.rs'))
        continue
    write(p, changed)
    ok = check()
    write(p, original)
    results.append((crate, name, 'SOBRA' if ok else 'lo usa alguien'))
    sys.stderr.write('.')
    sys.stderr.flush()

sys.stderr.write('\n')
for crate, name, verdict in results:
    print(crate.ljust(14) + name.ljust(24) + verdict)
