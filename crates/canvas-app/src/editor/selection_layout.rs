//! Alinear y distribuir raíces seleccionadas, expandiendo sus grupos.
use super::{layer_ops::selection_transforms, selection_geometry::bounds, EditorState};
use canvas_core::{Command, Composite, LayerId, SetTransform, Transform};
use eframe::egui;

struct Item {
    leaves: Vec<(LayerId, Transform)>,
    bounds: Transform,
}
#[derive(Clone, Copy)]
pub(super) enum Layout {
    Align { vertical: bool, fraction: f64 },
    Distribute { vertical: bool },
}
fn items(state: &EditorState) -> Vec<Item> {
    let Ok(page) = state.doc.page() else {
        return vec![];
    };
    let leaves = selection_transforms(state);
    state
        .selection
        .roots(page)
        .into_iter()
        .filter_map(|root| {
            let leaves: Vec<_> = leaves
                .iter()
                .copied()
                .filter(|(id, _)| *id == root || page.is_ancestor(root, *id))
                .collect();
            Some(Item {
                bounds: bounds(&leaves)?,
                leaves,
            })
        })
        .collect()
}
pub(super) fn apply(state: &mut EditorState, layout: Layout) {
    let mut items = items(state);
    if items.is_empty() {
        return;
    }
    let vertical = match layout {
        Layout::Align { vertical, .. } | Layout::Distribute { vertical } => vertical,
    };
    let coord = |t: &Transform| if vertical { t.y } else { t.x };
    let size = |t: &Transform| if vertical { t.height } else { t.width };
    items.sort_by(|a, b| coord(&a.bounds).total_cmp(&coord(&b.bounds)));
    let left = items
        .iter()
        .map(|i| coord(&i.bounds))
        .fold(f64::INFINITY, f64::min);
    let right = items
        .iter()
        .map(|i| coord(&i.bounds) + size(&i.bounds))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut targets = Vec::new();
    match layout {
        Layout::Align { fraction, .. } => {
            let (start, end) = if items.len() == 1 {
                let Ok(page) = state.doc.page() else {
                    return;
                };
                (0.0, if vertical { page.height } else { page.width })
            } else {
                (left, right)
            };
            for item in &items {
                targets.push(start + (end - start - size(&item.bounds)) * fraction);
            }
        }
        Layout::Distribute { .. } => {
            if items.len() < 3 {
                return;
            }
            let gap = (right - left - items.iter().map(|i| size(&i.bounds)).sum::<f64>())
                / (items.len() - 1) as f64;
            let mut cursor = left;
            for item in &items {
                targets.push(cursor);
                cursor += size(&item.bounds) + gap;
            }
        }
    }
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    for (item, target) in items.into_iter().zip(targets) {
        let delta = target - coord(&item.bounds);
        if delta.abs() < 1e-9 {
            continue;
        }
        for (layer, before) in item.leaves {
            let after = Transform {
                x: before.x + if vertical { 0.0 } else { delta },
                y: before.y + if vertical { delta } else { 0.0 },
                ..before
            };
            cmds.push(Box::new(SetTransform {
                layer,
                before,
                after,
            }));
        }
    }
    if !cmds.is_empty() {
        let _ = state.apply_undo_step(Box::new(Composite::new("Organizar selección", cmds)));
    }
}
pub(super) fn controls(state: &mut EditorState, ui: &mut egui::Ui) {
    let count = items(state).len();
    ui.add_enabled_ui(count > 0, |ui| {
        ui.menu_button(crate::i18n::tr("Align selection"), |ui| {
            for (label, vertical, fraction) in [
                ("Left", false, 0.0),
                ("Center", false, 0.5),
                ("Right", false, 1.0),
                ("Top", true, 0.0),
                ("Middle", true, 0.5),
                ("Bottom", true, 1.0),
            ] {
                if ui.button(crate::i18n::tr(label)).clicked() {
                    apply(state, Layout::Align { vertical, fraction });
                    ui.close();
                }
            }
        });
    });
    ui.add_enabled_ui(count >= 3, |ui| {
        ui.menu_button(crate::i18n::tr("Distribute"), |ui| {
            for (label, vertical) in [("Horizontal spacing", false), ("Vertical spacing", true)] {
                if ui.button(crate::i18n::tr(label)).clicked() {
                    apply(state, Layout::Distribute { vertical });
                    ui.close();
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{LayerContent, ShapeContent};
    fn fixture() -> (EditorState, Vec<LayerId>) {
        let mut s = EditorState::new_blank(600.0, 400.0);
        let mut ids = Vec::new();
        for (x, w) in [(10.0, 10.0), (50.0, 20.0), (130.0, 30.0)] {
            let id = s
                .doc
                .add_layer(
                    "shape",
                    Transform::new(x, 20.0, w, 20.0),
                    LayerContent::Shape(ShapeContent::default()),
                )
                .unwrap();
            s.selection.toggle(id);
            ids.push(id);
        }
        (s, ids)
    }
    #[test]
    fn distribute_equalizes_edge_gaps_and_preserves_outer_edges() {
        let (mut s, ids) = fixture();
        apply(&mut s, Layout::Distribute { vertical: false });
        assert_eq!(s.doc.layer(ids[0]).unwrap().transform.x, 10.0);
        assert_eq!(s.doc.layer(ids[1]).unwrap().transform.x, 65.0);
        assert_eq!(s.doc.layer(ids[2]).unwrap().transform.x, 130.0);
        assert_eq!(s.history.undo_depth(), 1);
        s.undo();
        assert_eq!(s.doc.layer(ids[1]).unwrap().transform.x, 50.0);
    }
    #[test]
    fn alignment_uses_selection_bounds_and_skips_locked_layers() {
        let (mut s, ids) = fixture();
        s.doc.layer_mut(ids[2]).unwrap().locked = true;
        apply(
            &mut s,
            Layout::Align {
                vertical: false,
                fraction: 1.0,
            },
        );
        assert_eq!(s.doc.layer(ids[0]).unwrap().transform.x, 60.0);
        assert_eq!(s.doc.layer(ids[1]).unwrap().transform.x, 50.0);
        assert_eq!(s.doc.layer(ids[2]).unwrap().transform.x, 130.0);
    }
    #[test]
    fn one_selected_group_aligns_all_children_to_the_page() {
        let (mut s, ids) = fixture();
        crate::layers_panel::group_selection(&mut s);
        apply(
            &mut s,
            Layout::Align {
                vertical: false,
                fraction: 0.5,
            },
        );
        assert_eq!(s.doc.layer(ids[0]).unwrap().transform.x, 225.0);
        assert_eq!(s.doc.layer(ids[2]).unwrap().transform.x, 345.0);
        s.undo();
        assert_eq!(s.doc.layer(ids[0]).unwrap().transform.x, 10.0);
    }
}
