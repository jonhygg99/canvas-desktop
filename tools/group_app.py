"""Agrupa los campos de `App` en cuatro sub-estados por dominio.

El movimiento es puramente mecanico: los nombres de campo NO cambian, solo se
les antepone el del sub-estado (`self.save_requested` -> `self.save.save_requested`).
Renombrarlos ademas habria hecho el diff mucho mas dificil de revisar.
"""
import io, re, glob, os, sys

GROUPS = {
 'save': ['pending_save_as','save_requested','close_after_save','allow_close','after_save',
          'overwrite_confirmed','overwrite_prompt','overwrite_dont_ask','readonly_prompt',
          'save_all_queue','save_all_attempted'],
 'export': ['export_dialog','pending_export_settings','pending_export'],
 'deck_ops': ['pending_deck','materializing','materialize_blocked','undoable_deletes'],
 'menu_mirror': ['menus_editor_open','menus_can_undo','menus_can_redo'],
}
FIELD_OF = {f: g for g, fs in GROUPS.items() for f in fs}

MOD='crates/canvas-app/src/app/mod.rs'
src=io.open(MOD,encoding='utf-8',newline='').read().replace('\r\n','\n')
L=src.split('\n')

# --- 1. extrae la declaracion de cada campo (con sus doc-comments) del struct
a = next(i for i,l in enumerate(L) if l.startswith('pub(crate) struct App {'))
b = next(i for i in range(a,len(L)) if L[i]=='}')
decls={}; keep=[]; pend=[]
for i in range(a+1,b):
    l=L[i]
    m=re.match(r'    (\w+): ', l)
    if m and m.group(1) in FIELD_OF:
        decls[m.group(1)]='\n'.join(pend+[l]); pend=[]
    elif m:
        keep.extend(pend+[l]); pend=[]
    else:
        pend.append(l)
keep.extend(pend)

def block(g, doc):
    body='\n'.join(decls[f] for f in GROUPS[g])
    return doc+'\nstruct '+''.join(w.capitalize() for w in g.split('_'))+' {\n'+body+'\n}\n'

subs = (
 block('save', '/// Todo lo que hace falta para llevar un guardado a termino: lo pedido, lo\n'
               '/// diferido, los modales de aviso y el lote de «Save all».') + '\n' +
 block('export', '/// El dialogo de exportacion y lo que queda pendiente de el.') + '\n' +
 block('deck_ops', '/// Contabilidad de la baraja que no cabe en `Deck` porque cruza con el disco:\n'
                   '/// la semilla pendiente de la galeria, las reservas de nombre en vuelo y los\n'
                   '/// borrados que todavia se pueden deshacer.') + '\n' +
 block('menu_mirror', '/// Ultimo estado comunicado a los menus nativos, para no reenviarlo cada frame.')
)

new_struct = ('\n'.join(L[:a+1]) + '\n' + '\n'.join(keep) +
  '\n    /// Estado del camino de guardado.\n    save: SaveFlow,'
  '\n    /// Estado del camino de exportacion.\n    export: ExportFlow,'
  '\n    /// Contabilidad de la baraja que cruza con el disco.\n    deck_ops: DeckOps,'
  '\n    /// Espejo del estado de los menus nativos.\n    menu_mirror: MenuMirror,\n}\n\n' +
  subs + '\n' + '\n'.join(L[b+1:]))
io.open(MOD,'w',encoding='utf-8',newline='\n').write(new_struct)

# --- 2. reescribe `self.<campo>` -> `self.<grupo>.<campo>` en todo el crate
FILES=[p for p in glob.glob('crates/canvas-app/src/**/*.rs', recursive=True)]
n=0
for p in FILES:
    s=io.open(p,encoding='utf-8',newline='').read().replace('\r\n','\n')
    o=s
    for f,g in FIELD_OF.items():
        s=re.sub(r'\bself\.'+f+r'\b', 'self.'+g+'.'+f, s)
    if s!=o:
        io.open(p,'w',encoding='utf-8',newline='\n').write(s); n+=1
print('reescritos', n, 'archivos')
