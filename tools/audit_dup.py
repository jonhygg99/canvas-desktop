"""Bloques de codigo repetidos entre archivos distintos.

Normaliza cada archivo (fuera comentarios, lineas vacias y sangria), desliza
una ventana de N lineas significativas y agrupa las que aparecen identicas en
mas de un archivo. Ignora los `use` y las llaves sueltas, que se repiten en
todas partes sin significar nada.
"""
import collections
import glob
import hashlib
import io
import os
import re
import sys

WINDOW = int(sys.argv[1]) if len(sys.argv) > 1 else 6
NOISE = re.compile(r'^(\}|\{|\)|\);|\},|use |mod |pub mod |#\[|//)')


def read(p):
    return io.open(p, encoding='utf-8', errors='replace').read()


def significant(path):
    """[(texto_normalizado, numero_de_linea)] de las lineas que cuentan."""
    out = []
    for i, raw in enumerate(read(path).split('\n'), start=1):
        line = raw.strip()
        if not line or NOISE.match(line):
            continue
        line = re.sub(r'\s+', ' ', line)
        out.append((line, i))
    return out


files = sorted(
    p.replace(os.sep, '/') for p in glob.glob('crates/**/*.rs', recursive=True))
files = [p for p in files if '/tests' not in p and not p.endswith('tests.rs')]

buckets = collections.defaultdict(list)
for p in files:
    lines = significant(p)
    for i in range(len(lines) - WINDOW + 1):
        chunk = [l for l, _ in lines[i:i + WINDOW]]
        key = hashlib.sha1('\n'.join(chunk).encode()).hexdigest()
        buckets[key].append((p, lines[i][1], chunk))

seen = set()
found = []
for key, hits in buckets.items():
    distinct = {p for p, _, _ in hits}
    if len(distinct) < 2:
        continue
    sig = tuple(sorted(distinct))
    if sig in seen:
        continue
    seen.add(sig)
    found.append((len(hits), hits))

found.sort(key=lambda x: -x[0])
for _, hits in found[:15]:
    print('--- repetido en:')
    for p, line, _ in hits:
        print('    ' + p + ':' + str(line))
    for l in hits[0][2][:4]:
        print('      | ' + l[:88])
    print()

print('grupos duplicados (ventana de ' + str(WINDOW) + ' lineas): ' + str(len(found)))
