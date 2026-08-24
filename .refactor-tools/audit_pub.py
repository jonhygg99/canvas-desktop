"""Simbolos publicos de cada crate biblioteca que nadie usa desde fuera.

En un `lib`, un `pub` sin consumidores no lo detecta ningun lint: `dead_code`
asume que alguien externo podria usarlo. En un workspace cerrado como este eso
es deuda invisible.
"""
import collections
import glob
import io
import os
import re

CRATES = ['canvas-core', 'canvas-io', 'canvas-render', 'canvas-shell']


def read(p):
    return io.open(p, encoding='utf-8', errors='replace').read()


def norm(p):
    return p.replace(os.sep, '/')


def exported_names(crate):
    """Lo que `lib.rs` pone a disposicion del resto del workspace."""
    s = read('crates/' + crate + '/src/lib.rs')
    names = set()
    for m in re.finditer(r'pub use ([\w:]+)::\{([^}]*)\}', s, re.S):
        for n in m.group(2).split(','):
            n = n.strip()
            if n and not n.startswith('//'):
                names.add(n.split(' as ')[-1].strip())
    for m in re.finditer(r'pub use ([\w:]+)::(\w+);', s):
        names.add(m.group(2))
    for m in re.finditer(r'^pub (?:fn|struct|enum|trait|type|const) (\w+)', s, re.M):
        names.add(m.group(1))
    return {n for n in names if n and n[0].isalpha()}


files = [norm(p) for p in glob.glob('crates/**/*.rs', recursive=True)]
files += [norm(p) for p in glob.glob('crates/*/examples/*.rs')]
files = sorted(set(files))

by_crate = collections.defaultdict(list)
for p in files:
    by_crate[p.split('/')[1]].append(p)

total_unused = 0
for crate in CRATES:
    names = exported_names(crate)
    prefix = 'crates/' + crate + '/'
    outside = ''.join(read(p) for p in files if not p.startswith(prefix))
    inside = ''.join(
        read(p) for p in by_crate[crate] if not p.endswith('src/lib.rs'))

    unused = []
    for n in sorted(names):
        if re.search(r'\b' + re.escape(n) + r'\b', outside):
            continue
        hits = len(re.findall(r'\b' + re.escape(n) + r'\b', inside))
        unused.append((n, hits))

    print('== ' + crate + ': ' + str(len(unused)) + '/' + str(len(names))
          + ' exportados sin uso fuera del crate')
    for n, hits in unused:
        print('   ' + n.ljust(26) + ' usos dentro del crate: ' + str(hits))
    total_unused += len(unused)

print()
print('total exportado y sin consumidores externos: ' + str(total_unused))
