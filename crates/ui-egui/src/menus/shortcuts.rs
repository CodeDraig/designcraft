//! Shortcut resolution, presentation, and key dispatch.

use super::{DesignApp, UI_COMMANDS, Value, activate, ui_label};

/// The default shortcut of a command.
pub fn default_shortcut(id: &str) -> Option<&'static str> {
    designcraft_engine::find_command(id).and_then(|c| c.shortcut).or_else(|| ui_label(id).and_then(|(_, s)| s))
}

/// The shortcut in effect for a command (the user's, else the default).
pub fn shortcut_of(app: &DesignApp, id: &str) -> Option<String> {
    match app.ui.shortcuts.get(id) {
        Some(s) if s.is_empty() => None,
        Some(s) => Some(s.clone()),
        None => default_shortcut(id).map(str::to_string),
    }
}

/// Every command with a shortcut in effect: (id, shortcut). UI commands come first: where one
/// shares keys with an engine command (⌘N, ⌘O…) it's the one with the dialog.
pub fn effective_shortcuts(app: &DesignApp) -> Vec<(String, String)> {
    UI_COMMANDS
        .iter()
        .map(|c| c.0)
        .chain(designcraft_engine::command_specs().iter().map(|c| c.id))
        .filter_map(|id| shortcut_of(app, id).map(|s| (id.to_string(), s)))
        .collect()
}

/// A shortcut as written in this app ("Cmd+Alt+Shift+K") from a key press.
pub fn shortcut_string(m: egui::Modifiers, key: egui::Key) -> String {
    let mut s = String::new();
    if m.command {
        s += "Cmd+";
    }
    if m.alt {
        s += "Alt+";
    }
    if m.shift {
        s += "Shift+";
    }
    s += key.name();
    s
}

/// A shortcut for display: ⌃⌥⇧⌘ order on macOS, Ctrl+Alt+Shift+ elsewhere.
pub fn shortcut_text(sc: &str) -> String {
    let mac = cfg!(target_os = "macos");
    // The key is the last part ("Cmd+-" ends in an empty part before "-").
    let (mods, key) = match sc.rsplit_once('+') {
        Some((m, "")) => (m.trim_end_matches('+'), "+"),
        Some((m, k)) => (m, k),
        None => ("", sc),
    };
    let has = |m: &str| mods.split('+').any(|x| x == m);
    let mut out = String::new();
    for (m, sym, word) in [("Ctrl", "⌃", "Ctrl+"), ("Alt", "⌥", "Alt+"), ("Shift", "⇧", "Shift+"), ("Cmd", "⌘", "Ctrl+")] {
        if has(m) && !(m == "Cmd" && !mac && has("Ctrl")) {
            out += if mac { sym } else { word };
        }
    }
    out + key
}

pub(super) fn parse_shortcut(sc: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let mut m = egui::Modifiers::NONE;
    let mut key = None;
    for part in sc.split('+').filter(|s| !s.is_empty()) {
        match part {
            "Cmd" => m.command = true,
            "Shift" => m.shift = true,
            "Alt" => m.alt = true,
            "Ctrl" => m.ctrl = true,
            "=" => key = Some(egui::Key::Equals),
            "-" => key = Some(egui::Key::Minus),
            "[" => key = Some(egui::Key::OpenBracket),
            "]" => key = Some(egui::Key::CloseBracket),
            ";" => key = Some(egui::Key::Semicolon),
            "'" => key = Some(egui::Key::Quote),
            "," => key = Some(egui::Key::Comma),
            "." => key = Some(egui::Key::Period),
            "\\" => key = Some(egui::Key::Backslash),
            "Delete" => key = Some(egui::Key::Delete),
            k => key = egui::Key::from_name(k),
        }
    }
    if sc.ends_with("+-") || sc == "-" {
        key = Some(egui::Key::Minus);
    }
    key.map(|k| (m, k))
}

/// Global keyboard shortcuts: menu commands and single-key tool shortcuts.
pub fn shortcuts(app: &mut DesignApp, ctx: &egui::Context) {
    // Only a focused text field takes the keys: the canvas (or a button) having focus after a click
    // must not swallow tool shortcuts until Esc clears it.
    if ctx.text_edit_focused() || app.ui.dialog.is_some() || app.ui.palette.is_some() {
        return;
    }
    let typing = app.session.wants_text();
    let events = ctx.input(|i| i.events.clone());
    for e in events {
        let egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } = e else { continue };
        // Command shortcuts.
        let mut fired: Option<String> = None;
        let all = effective_shortcuts(app);
        for (id, sc) in &all {
            let id = id.as_str();
            if let Some((m, k)) = parse_shortcut(sc)
                && k == key
                && m.command == modifiers.command
                && m.shift == modifiers.shift
                && m.alt == modifiers.alt
                && (m.command || m.alt || !typing)
                && !(id == "edit.clear" && typing)
            {
                // Single-key shortcuts (W) only when not typing.
                if !m.command && !m.alt && !m.shift && typing {
                    continue;
                }
                if id == "edit.clear" || (typing && matches!(id, "edit.copy" | "edit.cut" | "edit.paste" | "edit.pasteInPlace")) {
                    continue; // Handled by the active tool / text clipboard events.
                }
                fired = Some(id.to_string());
                break;
            }
        }
        if let Some(id) = fired {
            if app.native_shortcuts.contains(&id) && !app.ui.shortcuts.contains_key(&id) {
                continue; // The native menu handles it.
            }
            // Like choosing the menu item: "…" commands open their dialog.
            activate(app, &id, &Value::Null);
            continue;
        }
        // Tool shortcuts (single keys, Shift+key).
        if !typing && !modifiers.command && !modifiers.alt {
            let name = key.name();
            let sc = if modifiers.shift { format!("Shift+{name}") } else { name.to_string() };
            if let Some(tool) = designcraft_tools::tool_for_shortcut(&sc) {
                log::debug!("tool shortcut {sc} -> {tool}");
                if std::env::var_os("DESIGNCRAFT_DEBUG_KEYS").is_some() {
                    eprintln!("tool shortcut {sc} ({key:?}, {modifiers:?}) -> {tool}");
                }
                app.select_tool(tool);
            } else if key == egui::Key::Escape && app.session.tool_id() != "selection" && !app.session.tool_busy() {
                app.select_tool("selection");
            }
        }
    }
}
