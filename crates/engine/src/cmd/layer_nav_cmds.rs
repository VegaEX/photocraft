//! Keyboard layer navigation, the shortcuts that have no menu item (#352): ⌥] / ⌥[ select the
//! layer above / below, ⌥. / ⌥, the top / bottom layer, and ⇧⌥] / ⇧⌥[ add the layer above /
//! below to the selection. They step through the layer stack, top to bottom: a group comes
//! before its children, and the children of a collapsed group are included, as in Photoshop
//! (a recorded Select Backward Layer right after Group Layers lands on the group's top child;
//! the Layers panel then opens the group to show it).

use photocraft_doc::{Document, Layer, LayerContent, LayerId};
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::layer_multi_cmds::set_selection;
use crate::{EngineError, Result, Session};

fn has_layers(s: &Session) -> std::result::Result<(), String> {
    let st = s.active().ok_or("no document open")?;
    if st.doc.layers.is_empty() { Err("the document has no layers".into()) } else { Ok(()) }
}

/// Every layer, top to bottom: each group before its children, collapsed or not.
pub fn stack_rows(doc: &Document) -> Vec<LayerId> {
    fn rec(layers: &[Layer], out: &mut Vec<LayerId>) {
        for l in layers.iter().rev() {
            out.push(l.id);
            if let LayerContent::Group(g) = &l.content {
                rec(&g.children, out);
            }
        }
    }
    let mut out = Vec::new();
    rec(&doc.layers, &mut out);
    out
}

#[derive(Clone, Copy)]
enum Step {
    Above,
    Below,
    Top,
    Bottom,
}

/// Selects (or with `add`, adds to the selection) the row `step` away from the active layer.
/// At the top or bottom row the selection stays as it is.
fn navigate(s: &mut Session, step: Step, add: bool) -> Result<Value> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let rows = stack_rows(&st.doc);
    let last = rows.len().checked_sub(1).ok_or_else(|| EngineError::Other("the document has no layers".into()))?;
    let current = st.active_layer.and_then(|a| rows.iter().position(|r| *r == a));
    let target = match (step, current) {
        (Step::Top, _) | (Step::Above, None) => 0,
        (Step::Bottom, _) | (Step::Below, None) => last,
        (Step::Above, Some(i)) => i.saturating_sub(1),
        (Step::Below, Some(i)) => (i + 1).min(last),
    };
    let id = rows.get(target).copied().ok_or_else(|| EngineError::Other("no layer there".into()))?;
    if add {
        let mut sel = st.selected_layers();
        if !sel.contains(&id) {
            sel.push(id);
        }
        let anchor = st.layer_anchor;
        set_selection(s, sel, Some(id), anchor)?;
    } else {
        s.select_layer(id)?;
    }
    let st = s.active().ok_or(EngineError::NoDocument)?;
    Ok(json!({"layer": id.0, "selected": st.selected_layers().iter().map(|l| l.0).collect::<Vec<_>>()}))
}

pub fn specs() -> Vec<CommandSpec> {
    macro_rules! spec {
        ($id:expr, $label:expr, $sc:expr, $step:expr, $add:expr) => {
            CommandSpec {
                id: $id,
                label: $label,
                menu: &[],
                shortcut: Some($sc),
                params: "{} (steps through the layer stack, into collapsed groups too)",
                enabled: has_layers,
                journal: true,
                run: |s, _| navigate(s, $step, $add),
            }
        };
    }
    vec![
        spec!("layer.selectAbove", "Select Layer Above", "Alt+]", Step::Above, false),
        spec!("layer.selectBelow", "Select Layer Below", "Alt+[", Step::Below, false),
        spec!("layer.selectTop", "Select Top Layer", "Alt+.", Step::Top, false),
        spec!("layer.selectBottom", "Select Bottom Layer", "Alt+,", Step::Bottom, false),
        spec!("layer.addAboveToSelection", "Add Layer Above to Selection", "Alt+Shift+]", Step::Above, true),
        spec!("layer.addBelowToSelection", "Add Layer Below to Selection", "Alt+Shift+[", Step::Below, true),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bottom to top: Background, A, Group { B, C }, D. Stack rows: D, Group, C, B, A, Background.
    fn session() -> (Session, [LayerId; 6]) {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 16, "height": 16})).unwrap();
        let bg = s.active().unwrap().doc.layers[0].id;
        let id = |s: &mut Session, cmd: &str, p: Value| LayerId(s.execute(cmd, p).unwrap()["layer"].as_u64().unwrap());
        let a = id(&mut s, "layer.new.layer", json!({"name": "A"}));
        let b = id(&mut s, "layer.new.layer", json!({"name": "B"}));
        let c = id(&mut s, "layer.new.layer", json!({"name": "C"}));
        s.execute("layer.select", json!({"layer": b.0})).unwrap();
        s.execute("layer.select", json!({"layer": c.0, "mode": "add"})).unwrap();
        s.execute("layer.groupLayers", json!({})).unwrap();
        let group = s.active().unwrap().doc.layers.iter().find(|l| l.is_group()).unwrap().id;
        // D goes above the closed group (an open one would take it inside, as in Photoshop).
        let d = id(&mut s, "layer.new.layer", json!({"name": "D"}));
        // Group Layers makes a collapsed group; open it so the panel shows B and C.
        s.execute("layer.setExpanded", json!({"layer": group.0, "expanded": true})).unwrap();
        (s, [bg, a, group, b, c, d])
    }

    fn active(s: &Session) -> LayerId {
        s.active().unwrap().active_layer.unwrap()
    }

    #[test]
    fn steps_follow_the_stack_and_stop_at_the_ends() {
        let (mut s, [bg, a, group, b, c, d]) = session();
        assert_eq!(stack_rows(&s.active().unwrap().doc), vec![d, group, c, b, a, bg]);
        s.execute("layer.select", json!({"layer": d.0})).unwrap();
        for want in [group, c, b, a, bg, bg] {
            s.execute("layer.selectBelow", json!({})).unwrap();
            assert_eq!(active(&s), want);
        }
        for want in [a, b, c, group, d, d] {
            s.execute("layer.selectAbove", json!({})).unwrap();
            assert_eq!(active(&s), want);
        }
        s.execute("layer.selectBottom", json!({})).unwrap();
        assert_eq!((active(&s), s.active().unwrap().selected_layers()), (bg, vec![bg]));
        s.execute("layer.selectTop", json!({})).unwrap();
        assert_eq!(active(&s), d);
    }

    #[test]
    fn collapsed_groups_are_stepped_into_and_adding_extends_the_selection() {
        let (mut s, [bg, a, group, b, c, d]) = session();
        s.execute("layer.setExpanded", json!({"layer": group.0, "expanded": false})).unwrap();
        assert_eq!(stack_rows(&s.active().unwrap().doc), vec![d, group, c, b, a, bg]);
        s.execute("layer.select", json!({"layer": d.0})).unwrap();
        for want in [group, c] {
            s.execute("layer.selectBelow", json!({})).unwrap();
            assert_eq!(active(&s), want);
        }
        s.execute("layer.select", json!({"layer": a.0})).unwrap();
        s.execute("layer.addAboveToSelection", json!({})).unwrap();
        s.execute("layer.addAboveToSelection", json!({})).unwrap();
        let mut sel = s.active().unwrap().selected_layers();
        sel.sort();
        let mut want = vec![a, b, c];
        want.sort();
        assert_eq!((active(&s), sel), (c, want));
        s.execute("layer.addBelowToSelection", json!({})).unwrap();
        assert_eq!(active(&s), b, "the active layer moves; the selection keeps every layer");
        assert_eq!(s.active().unwrap().selected_layers().len(), 3);
    }

    /// The TrueGrit Debaser Ink Distressing actions: Group Layers (which makes a closed group),
    /// then Select Backward Layer and target its layer mask. Before, the step skipped the closed
    /// group and landed on the line art, so targeting the mask failed.
    #[test]
    fn select_below_after_group_layers_lands_on_the_new_groups_child() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 16, "height": 16})).unwrap();
        s.execute("layer.new.layer", json!({"name": "Line Art"})).unwrap();
        s.execute("select.all", json!({})).unwrap();
        let fill = LayerId(s.execute("layer.newFillLayer.solidColor", json!({"color": "#000000"})).unwrap()["layer"].as_u64().unwrap());
        let group = LayerId(s.execute("layer.groupLayers", json!({})).unwrap()["layer"].as_u64().unwrap());
        let doc = &s.active().unwrap().doc;
        assert!(matches!(&doc.layer(group).unwrap().content, LayerContent::Group(g) if !g.expanded));
        s.execute("layer.setProps", json!({"blend": "Multiply"})).unwrap();
        s.execute("layer.selectBelow", json!({})).unwrap();
        assert_eq!(active(&s), fill);
        s.execute("channel.target", json!({"channel": "mask"})).unwrap();
    }

    #[test]
    fn selecting_never_dirties_and_bad_states_are_errors() {
        let (mut s, _) = session();
        let st = s.active_mut().unwrap();
        st.saved_revision = st.revision;
        s.execute("layer.selectTop", json!({"junk": [1, 2]})).unwrap();
        assert!(!s.active().unwrap().is_dirty(), "selecting a layer is not an edit");
        let mut empty = Session::new();
        for c in specs() {
            assert!(!empty.is_enabled(c.id));
            assert!(empty.execute(c.id, json!({})).is_err());
        }
        empty.execute("file.new", json!({"width": 4, "height": 4})).unwrap();
        empty
            .edit("clear", |doc, active| {
                doc.layers.clear();
                *active = None;
                Ok(())
            })
            .unwrap();
        for c in specs() {
            assert!(empty.execute(c.id, json!({})).is_err(), "{}: no layers", c.id);
        }
    }
}
