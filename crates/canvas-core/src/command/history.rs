//! Composicion de pasos: `Composite` (varios comandos como uno solo para
//! deshacer) y la pila `History` que los conduce.

use crate::document::Document;
use crate::error::CoreError;

use super::Command;

/// Agrupa varios comandos en UN solo paso de deshacer: se aplican en orden y
/// se revierten en orden inverso.
#[derive(Debug)]
pub struct Composite {
    label: String,
    commands: Vec<Box<dyn Command>>,
}

impl Composite {
    pub fn new(label: impl Into<String>, commands: Vec<Box<dyn Command>>) -> Self {
        Self {
            label: label.into(),
            commands,
        }
    }
}

impl Command for Composite {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        for cmd in &mut self.commands {
            cmd.apply(doc)?;
        }
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        for cmd in self.commands.iter_mut().rev() {
            cmd.revert(doc)?;
        }
        Ok(())
    }
}

/// Historial de deshacer/rehacer basado en comandos, con marca de guardado
/// para derivar el estado sucio (dirty) sin un flag manual.
pub struct History {
    undo: Vec<Box<dyn Command>>,
    /// `pub(super)` solo para que los tests del modulo `command` puedan
    /// sembrar un comando que falla al rehacer; nada fuera de `command` la ve.
    pub(super) redo: Vec<Box<dyn Command>>,
    /// Longitud de la pila de undo en el último guardado. `None` si el estado
    /// guardado ya no es alcanzable deshaciendo/rehaciendo.
    saved_depth: Option<usize>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::with_limit(200)
    }
}

impl History {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            saved_depth: Some(0),
            limit: limit.max(1),
        }
    }

    /// Aplica el comando al documento y lo apila.
    pub fn apply(
        &mut self,
        doc: &mut Document,
        mut cmd: Box<dyn Command>,
    ) -> Result<(), CoreError> {
        cmd.apply(doc)?;
        self.push_applied(cmd);
        Ok(())
    }

    /// Apila un comando cuyo efecto YA está reflejado en el documento (final
    /// de un gesto continuo).
    pub fn push_applied(&mut self, cmd: Box<dyn Command>) {
        // Si el punto de guardado quedaba por delante (en la pila de redo que
        // vamos a vaciar), deja de ser alcanzable.
        if self.saved_depth.is_some_and(|d| d > self.undo.len()) {
            self.saved_depth = None;
        }
        self.redo.clear();
        self.undo.push(cmd);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
            self.saved_depth = match self.saved_depth {
                Some(0) | None => None,
                Some(d) => Some(d - 1),
            };
        }
    }

    /// Deshace el último comando. Devuelve `false` si no había nada que
    /// deshacer.
    ///
    /// El comando se saca de la pila ANTES de invocar `revert`: si falla, se
    /// descarta en vez de quedarse arriba del todo. La alternativa (sacarlo
    /// después) deja el historial atascado para siempre — cada Ctrl+Z
    /// posterior reintentaría el mismo revert fallido en silencio. El precio
    /// es que un `Composite` cuyo revert falle a medias puede perder el resto
    /// de sus pasos sin deshacer; preferible a un historial muerto.
    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, CoreError> {
        let Some(mut cmd) = self.undo.pop() else {
            return Ok(false);
        };
        let result = cmd.revert(doc);
        if result.is_ok() {
            self.redo.push(cmd);
        }
        result.map(|()| true)
    }

    /// Rehace el último comando deshecho. Devuelve `false` si no había nada.
    pub fn redo(&mut self, doc: &mut Document) -> Result<bool, CoreError> {
        let Some(mut cmd) = self.redo.pop() else {
            return Ok(false);
        };
        let result = cmd.apply(doc);
        if result.is_ok() {
            self.undo.push(cmd);
        }
        result.map(|()| true)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Marca el estado actual como guardado en disco.
    pub fn mark_saved(&mut self) {
        self.saved_depth = Some(self.undo.len());
    }

    /// ¿Hay cambios sin guardar respecto al último `mark_saved`?
    pub fn is_dirty(&self) -> bool {
        self.saved_depth != Some(self.undo.len())
    }
}
