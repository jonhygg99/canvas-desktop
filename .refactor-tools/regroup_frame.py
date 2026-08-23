"""`EditorFrame` pasa de 25 prestamos sueltos a 11: los campos que ya viven
agrupados en `App` viajan agrupados tambien aqui."""
import io, re, glob

GROUPS = {
 'save': ['pending_save_as','save_requested','close_after_save','allow_close','after_save',
          'overwrite_confirmed','overwrite_prompt','overwrite_dont_ask','readonly_prompt',
          'save_all_queue','save_all_attempted'],
 'export': ['export_dialog','pending_export_settings','pending_export'],
 'deck_ops': ['pending_deck','materializing','materialize_blocked','undoable_deletes'],
}
FIELD_OF = {f: g for g, fs in GROUPS.items() for f in fs}

p='crates/canvas-app/src/app/frame.rs'
s=io.open(p,encoding='utf-8',newline='').read().replace('\r\n','\n')
a=s.index('pub(super) struct EditorFrame')
head=s[:a]
new = """pub(super) struct EditorFrame<'a> {
    pub(super) deck: &'a mut deck::Deck,
    pub(super) renderer: &'a mut CanvasRenderer,
    pub(super) surface: &'a mut Option<CanvasSurface>,
    pub(super) tx: &'a Sender<AppMsg>,
    pub(super) settings: &'a mut settings::AppSettings,
    pub(super) show_settings: &'a mut bool,
    pub(super) watcher: &'a mut Option<watcher::DocWatcher>,
    pub(super) ignore_fs_events_until: &'a mut Option<Instant>,
    /// Estado del camino de guardado.
    pub(super) save: &'a mut SaveFlow,
    /// Estado del camino de exportacion.
    pub(super) export: &'a mut ExportFlow,
    /// Contabilidad de la baraja que cruza con el disco.
    pub(super) deck_ops: &'a mut DeckOps,
}
"""
head=head.replace('use crate::{deck, export, settings, watcher};','use crate::{deck, settings, watcher};')
head=head.replace('use super::Nav;','use super::{DeckOps, ExportFlow, SaveFlow};')
io.open(p,'w',encoding='utf-8',newline='\n').write(head+new)

# call site
p='crates/canvas-app/src/app/mod.rs'
s=io.open(p,encoding='utf-8',newline='').read().replace('\r\n','\n')
a=s.index('                let mut frame = frame::EditorFrame {')
b=s.index('                };', a)+len('                };\n')
s=s[:a]+"""                let mut frame = frame::EditorFrame {
                    deck: &mut self.deck,
                    renderer: &mut self.renderer,
                    surface: &mut self.surface,
                    tx: &self.tx,
                    settings: &mut self.settings,
                    show_settings: &mut self.show_settings,
                    watcher: &mut self.watcher,
                    ignore_fs_events_until: &mut self.ignore_fs_events_until,
                    save: &mut self.save,
                    export: &mut self.export,
                    deck_ops: &mut self.deck_ops,
                };
"""+s[b:]
s=s.replace('struct SaveFlow {','pub(super) struct SaveFlow {').replace('struct ExportFlow {','pub(super) struct ExportFlow {').replace('struct DeckOps {','pub(super) struct DeckOps {')
io.open(p,'w',encoding='utf-8',newline='\n').write(s)

# f.<campo> -> f.<grupo>.<campo> en las vistas del editor
for q in glob.glob('crates/canvas-app/src/app/views/**/*.rs', recursive=True):
    s=io.open(q,encoding='utf-8',newline='').read().replace('\r\n','\n')
    o=s
    for fld,g in FIELD_OF.items():
        s=re.sub(r'\bf\.'+fld+r'\b', 'f.'+g+'.'+fld, s)
    if s!=o:
        io.open(q,'w',encoding='utf-8',newline='\n').write(s)
print('ok')
