"""Utilidades de troceado mecanico para el refactor.

`items(path)` devuelve los items de nivel superior de un archivo .rs con el
rango de lineas que les corresponde, incluyendo los comentarios de doc y los
atributos que los preceden. `emit` escribe un archivo nuevo con una cabecera
propia y los bloques pedidos, en el mismo orden en que estaban.
"""
import re, sys, io, os

DOCLIKE = re.compile(r'^\s*(///|//!|#\[|#!\[|//)')

def read(path):
    with io.open(path, encoding='utf-8', newline='') as f:
        return f.read().replace('\r\n', '\n').split('\n')

def items(path):
    """[(name, start, end)] con lineas 0-indexadas, end exclusivo."""
    lines = read(path)
    heads = []
    for i, l in enumerate(lines):
        m = re.match(r'^(pub(\([^)]*\))? )?(unsafe )?(struct|enum|trait|impl|fn|const|static|type|mod|macro_rules!)\b(.*)', l)
        if not m:
            continue
        kind = m.group(4)
        rest = m.group(5)
        name = re.sub(r'[<({=;].*$', '', rest).strip()
        heads.append((i, f'{kind} {name}'.strip()))
    starts = []
    for i, name in heads:
        # sube por los doc-comments/atributos pegados justo encima
        s = i
        while s > 0 and DOCLIKE.match(lines[s-1]) and lines[s-1].strip():
            s -= 1
        starts.append(s)
    out = []
    for n, (i, name) in enumerate(heads):
        end = starts[n+1] if n + 1 < len(heads) else len(lines)
        while end > i and not lines[end-1].strip():
            end -= 1
        out.append((name, starts[n], end))
    return out, lines

def emit(dest, header, lines, ranges):
    body = []
    for (a, b) in ranges:
        body.append('\n'.join(lines[a:b]))
    text = header.rstrip('\n') + '\n\n' + '\n\n'.join(body).rstrip('\n') + '\n'
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with io.open(dest, 'w', encoding='utf-8', newline='\n') as f:
        f.write(text)

if __name__ == '__main__':
    its, lines = items(sys.argv[1])
    for name, s, e in its:
        print(f'{s+1:>5}-{e:<5} {name}')


METHOD = re.compile(r'^(?P<ind>[ ]+)(pub(\([^)]*\))? )?(async )?(unsafe )?(const )?fn (?P<name>\w+)')

def methods(path, impl_start, impl_end):
    """Trocea el cuerpo de un impl (rango 0-indexado, end exclusivo) en
    (nombre, inicio, fin) por metodo, quedandose los doc-comments de encima.
    Devuelve tambien la linea de apertura del impl y la de cierre."""
    lines = read(path)
    heads = []
    for i in range(impl_start, impl_end):
        m = METHOD.match(lines[i])
        if m and len(m.group('ind')) == 4:
            heads.append((i, m.group('name')))
    starts = []
    for i, _ in heads:
        s = i
        while s > impl_start + 1 and DOCLIKE.match(lines[s-1]) and lines[s-1].strip():
            s -= 1
        starts.append(s)
    out = []
    for n, (i, name) in enumerate(heads):
        end = starts[n+1] if n + 1 < len(heads) else impl_end - 1
        while end > i and not lines[end-1].strip():
            end -= 1
        out.append((name, starts[n], end))
    return out, lines
