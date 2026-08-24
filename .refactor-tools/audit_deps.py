"""Dependencias declaradas que el crate no nombra en su codigo."""
import glob
import io
import os
import re


def read(p):
    return io.open(p, encoding='utf-8', errors='replace').read()


def norm(p):
    return p.replace(os.sep, '/')


for manifest in sorted(norm(p) for p in glob.glob('crates/*/Cargo.toml')):
    crate_dir = manifest.rsplit('/', 1)[0]
    crate = crate_dir.split('/')[-1]
    s = read(manifest)

    sources = ''
    for p in glob.glob(crate_dir + '/src/**/*.rs', recursive=True):
        sources += read(p)
    for p in glob.glob(crate_dir + '/examples/*.rs'):
        sources += read(p)
    for p in glob.glob(crate_dir + '/tests/**/*.rs', recursive=True):
        sources += read(p)
    for p in glob.glob(crate_dir + '/build.rs'):
        sources += read(p)

    missing = []
    section = None
    for line in s.split('\n'):
        line = line.strip()
        if line.startswith('['):
            section = line
            continue
        if section not in ('[dependencies]', '[dev-dependencies]',
                           '[build-dependencies]'):
            continue
        m = re.match(r'^([A-Za-z0-9_-]+)\s*[=.]', line)
        if not m:
            continue
        dep = m.group(1)
        ident = dep.replace('-', '_')
        if re.search(r'\b' + re.escape(ident) + r'\b', sources):
            continue
        missing.append((dep, section))

    if missing:
        print('== ' + crate)
        for dep, section in missing:
            print('   ' + dep.ljust(22) + section)
