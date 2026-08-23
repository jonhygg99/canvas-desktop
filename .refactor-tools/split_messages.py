import io, re, os, sys
sys.path.insert(0, '.refactor-tools')
from msg_spec import SPEC, GROUP, DOC, ORDER

src='crates/canvas-app/src/app/messages.rs'
L=io.open(src,encoding='utf-8',newline='').read().replace('\r\n','\n').split('\n')

starts=[i for i,l in enumerate(L) if l.startswith('                AppMsg::')]
end_match=next(i for i in range(starts[-1],len(L)) if L[i]=='            }')
arms=[]
for n,s in enumerate(starts):
    e = starts[n+1] if n+1<len(starts) else end_match
    arms.append((re.match(r'\s*AppMsg::(\w+)', L[s]).group(1), s, e))

bodies={}; patterns={}
for name,s,e in arms:
    txt='\n'.join(L[s:e]).rstrip('\n')
    i=txt.index('=> ')
    patterns[name]=txt[:i].rstrip()
    if 'FilePicked' in patterns[name]:
        continue
    body=txt[i+3:]
    if body.startswith('{'):
        body=body[1:].rstrip()
        assert body.endswith('}'), name
        body=body[:-1]
    else:
        body='\n'+body.rstrip().rstrip(',')+'\n'
    bodies[name]='\n'.join(l[20:] if l.startswith(' '*20) else l for l in body.split('\n')).strip('\n')

D='crates/canvas-app/src/app/messages/'
os.makedirs(D, exist_ok=True)
HDR = ("use std::path::PathBuf;\n\nuse eframe::egui;\n\n"
       "use crate::{deck, editor, gallery, loader, settings};\n\n"
       "use super::super::{App, Nav, View};\nuse loader::AppMsg;\n")

calls={}
for f,names in GROUP.items():
    out=[]
    for name in names:
        params,fname = SPEC[name]
        body=bodies[name]
        sig=[n+': '+t for n,t in params]; args=[n for n,_ in params]
        if re.search(r'(?<![\w.])ctx\b', body):
            sig.append('ctx: &egui::Context'); args.append('ctx')
        if re.search(r'(?<![\w.])open_after\b', body):
            sig.append('open_after: &mut Option<Nav>'); args.append('open_after')
        body=re.sub(r'(?<![\w.*])open_after = Some\(', '*open_after = Some(', body)
        ind='\n'.join(('    '+l if l.strip() else l) for l in body.split('\n'))
        out.append('    pub(super) fn '+fname+'(&mut self'+(', ' if sig else '')+', '.join(sig)+') {\n'+ind+'\n    }')
        calls[name]='                '+patterns[name].lstrip()+' => self.'+fname+'('+', '.join(args)+'),'
    io.open(D+f,'w',encoding='utf-8',newline='\n').write(
        DOC[f]+'\n\n'+HDR+'\nimpl App {\n'+'\n\n'.join(out)+'\n}\n')

dispatch=[]
for name in ORDER:
    if name=='FilePicked':
        dispatch.append('                AppMsg::FilePicked(Some(path)) | AppMsg::FolderPicked(Some(path)) => {\n'
                        '                    open_after = Some(Nav::Open(path));\n'
                        '                }\n'
                        '                AppMsg::FilePicked(None) | AppMsg::FolderPicked(None) => {}')
    else:
        dispatch.append(calls[name])

HEAD = ('//! Bucle de mensajes: reacciona a todo lo que vuelve de los hilos de fondo\n'
        '//! (`AppMsg` - cargas, guardados, exportaciones, miniaturas, sondeos de la\n'
        '//! baraja, escaneos de la galeria...). Este archivo es solo el bucle y el\n'
        '//! despacho: el cuerpo de cada respuesta vive en el submodulo de su dominio.\n\n'
        'use eframe::egui;\n\nuse crate::loader;\n\n'
        'use super::{App, Nav, View};\nuse loader::AppMsg;\n\n'
        'mod document;\nmod export;\nmod gallery;\nmod load;\nmod save;\nmod shell;\n\n'
        'impl App {\n')
TAILSIG = ('\n\n    pub(super) fn handle_messages(&mut self, ctx: &egui::Context) {\n'
           '        // Aperturas diferidas para no pelear con el prestamo de self.view.\n'
           '        let mut open_after: Option<Nav> = None;\n'
           '        while let Ok(msg) = self.rx.try_recv() {\n'
           '            match msg {\n')
TAIL = ('\n            }\n        }\n        if let Some(nav) = open_after {\n'
        '            self.navigate(nav, ctx);\n        }\n    }\n}\n')
io.open(D+'mod.rs','w',encoding='utf-8',newline='\n').write(
    HEAD + '\n'.join(L[15:28]) + TAILSIG + '\n'.join(dispatch) + TAIL)
os.remove(src)
print('ok')
