//! Workspace, pane, panel, and shortcut configuration commands.

use super::{DesignApp, Value, effective_shortcuts, json, parse_shortcut, run_ui, shortcut_of, ui_label};

pub(super) fn run(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let flag = |b: &mut bool| {
        *b = !*b;
        Ok(json!(*b))
    };
    Some(match id {
        "window.hideMenuItem" => {
            let Some(item) = p.get("item").and_then(Value::as_str) else { return Some(Err("missing item".into())) };
            let hide = p.get("hidden").and_then(Value::as_bool).unwrap_or(true);
            app.ui.hidden_menu_items.retain(|x| x != item);
            if hide {
                app.ui.hidden_menu_items.push(item.to_string());
            }
            app.ui.show_full_menus = false;
            Ok(Value::Null)
        }
        "window.hidePanels" | "window.hidePanelsExceptTools" => {
            let mode = if id == "window.hidePanels" { 1 } else { 2 };
            app.ui.hidden_panels = if app.ui.hidden_panels == mode { 0 } else { mode };
            Ok(json!(app.ui.hidden_panels))
        }
        "window.nextDocument" | "window.previousDocument" => {
            let n = app.session.documents().len();
            if n > 1 {
                let cur = app.session.active_index().unwrap_or(0);
                let next = if id == "window.nextDocument" { (cur + 1) % n } else { (cur + n - 1) % n };
                app.session.set_active(next);
            }
            Ok(json!(app.session.active_index()))
        }
        "window.panel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("properties").to_string();
            if crate::dock::DOCK_TABS.iter().any(|(id, _, _)| *id == panel) {
                app.ui.dock_tab = panel;
                app.ui.dock_expanded = true;
                app.ui.open_panel = None;
            } else if !app.ui.floating.iter().any(|(p, _)| *p == panel) {
                app.ui.open_panel = if app.ui.open_panel.as_deref() == Some(panel.as_str()) { None } else { Some(panel) };
            }
            Ok(Value::Null)
        }
        "window.setShortcut" => {
            let Some(cmd) = p.get("id").and_then(Value::as_str) else { return Some(Err("missing id".into())) };
            if designcraft_engine::find_command(cmd).is_none() && ui_label(cmd).is_none() {
                return Some(Err(format!("unknown command `{cmd}`")));
            }
            match p.get("shortcut") {
                None | Some(Value::Null) => {
                    app.ui.shortcuts.remove(cmd);
                }
                Some(Value::String(sc)) if sc.is_empty() => {
                    app.ui.shortcuts.insert(cmd.to_string(), String::new());
                }
                Some(Value::String(sc)) => {
                    if parse_shortcut(sc).is_none() {
                        return Some(Err(format!("can't read shortcut `{sc}`")));
                    }
                    app.ui.shortcuts.insert(cmd.to_string(), sc.clone());
                }
                Some(_) => return Some(Err("shortcut: a string or null".into())),
            }
            let sc = shortcut_of(app, cmd);
            let conflicts: Vec<String> =
                effective_shortcuts(app).into_iter().filter(|(i, s)| i != cmd && Some(s) == sc.as_ref()).map(|(i, _)| i).collect();
            Ok(json!({"shortcut": sc, "conflicts": conflicts}))
        }
        "window.resetShortcuts" => {
            app.ui.shortcuts.clear();
            Ok(Value::Null)
        }
        "window.richBlack" => {
            app.ui.rich_black = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.rich_black);
            app.canvas.shown = None;
            Ok(json!({"on": app.ui.rich_black}))
        }
        "window.uiScale" => {
            let v = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
            app.ui.ui_scale = v.clamp(0.5, 3.0);
            Ok(json!({"scale": app.ui.ui_scale}))
        }
        "window.floatPanel" | "window.dockPanel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("");
            if !crate::dock::ICON_PANELS.iter().any(|(id, _, _)| *id == panel) {
                return Some(Err(format!("{id}: unknown panel `{panel}`")));
            }
            if id == "window.floatPanel" {
                let at = egui::pos2(
                    p.get("x").and_then(Value::as_f64).unwrap_or(400.0) as f32,
                    p.get("y").and_then(Value::as_f64).unwrap_or(160.0) as f32,
                );
                crate::dock::float_panel(app, panel, at);
            } else {
                crate::dock::dock_panel(app, panel);
            }
            Ok(json!({"floating": app.ui.floating.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>()}))
        }
        "window.controlBar" => flag(&mut app.ui.control_bar),
        "window.split" => {
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(!app.split);
            app.split = on;
            if on {
                app.second_window = false;
            } else {
                app.switch_pane(0);
                app.focus_pane = 0;
            }
            Ok(json!(on))
        }
        "window.newWindow" => {
            // The second view moves into its own window (on the web: a floating window).
            app.second_window = p.get("on").and_then(Value::as_bool).unwrap_or(true);
            app.split = false;
            if !app.second_window {
                app.switch_pane(0);
                app.focus_pane = 0;
            }
            Ok(json!(app.second_window))
        }
        "window.taskBar" => flag(&mut app.ui.task_bar),
        "window.toolsDoubleColumn" => flag(&mut app.ui.tools_double_column),
        "window.newWorkspace" => {
            let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("newWorkspace", json!({"name": ""})));
                return Some(Ok(Value::Null));
            };
            let u = &app.ui;
            let w = crate::SavedWorkspace {
                name: name.to_string(),
                control_bar: u.control_bar,
                task_bar: u.task_bar,
                tools_double_column: u.tools_double_column,
                dock_tab: u.dock_tab.clone(),
                dock_expanded: u.dock_expanded,
                open_panel: u.open_panel.clone(),
                floating: u.floating.clone(),
            };
            app.ui.custom_workspaces.retain(|x| x.name != name);
            app.ui.custom_workspaces.push(w);
            app.ui.workspace = name.to_string();
            Ok(Value::Null)
        }
        "window.deleteWorkspace" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            let n = app.ui.custom_workspaces.len();
            app.ui.custom_workspaces.retain(|x| x.name != name);
            if app.ui.custom_workspaces.len() == n {
                return Some(Err(format!("no saved workspace `{name}`")));
            }
            if app.ui.workspace == name {
                app.ui.workspace = "Essentials".into();
            }
            Ok(Value::Null)
        }
        "window.resetWorkspace" => {
            let name = app.ui.workspace.clone();
            return run_ui(app, "window.workspace", &json!({"name": name}));
        }
        "window.workspace" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("Essentials");
            if let Some(w) = app.ui.custom_workspaces.iter().find(|w| w.name == name).cloned() {
                app.ui.control_bar = w.control_bar;
                app.ui.task_bar = w.task_bar;
                app.ui.tools_double_column = w.tools_double_column;
                app.ui.dock_tab = w.dock_tab;
                app.ui.dock_expanded = w.dock_expanded;
                app.ui.open_panel = w.open_panel;
                app.ui.floating = w.floating;
                app.ui.workspace = w.name;
                return Some(Ok(Value::Null));
            }
            // Workspaces choose which bars and panels are visible.
            app.ui.control_bar = matches!(name, "Advanced" | "Typography" | "Printing and Proofing" | "Book");
            app.ui.dock_tab = if name == "Typography" { "properties".into() } else { app.ui.dock_tab.clone() };
            app.ui.open_panel = match name {
                "Typography" => Some("paragraphStyles".into()),
                "Printing and Proofing" => Some("swatches".into()),
                _ => None,
            };
            app.ui.workspace = name.to_string();
            Ok(Value::Null)
        }
        "window.brightness" => {
            let b = p.get("brightness").and_then(Value::as_str).and_then(crate::theme::Brightness::parse).unwrap_or(crate::theme::Brightness::Dark);
            app.ui.brightness = b;
            app.restyle = true;
            Ok(Value::Null)
        }
        _ => return None,
    })
}
