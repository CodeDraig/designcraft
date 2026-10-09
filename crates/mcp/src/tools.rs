//! MCP tool definitions (names, descriptions, JSON Schemas) and their implementations on top of a
//! [`Backend`].

mod schema;
pub use schema::tool_definitions;
mod result;
pub use result::ToolResult;

use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::headless::NEEDS_APP;

type Args = Map<String, Value>;

fn req_str<'a>(a: &'a Args, k: &str) -> Result<&'a str, String> {
    a.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| format!("missing string argument `{k}`"))
}

fn exec(b: &mut dyn Backend, cmd: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command": cmd, "params": params}))
}

fn need_ui(b: &dyn Backend, tool: &str) -> Result<(), String> {
    if b.has_ui() { Ok(()) } else { Err(format!("`{tool}` is not available headless; {NEEDS_APP}")) }
}

/// The given keys of `a` that are present, as a JSON object.
fn pick(a: &Args, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|k| a.get(*k).filter(|v| !v.is_null()).map(|v| (k.to_string(), v.clone()))).collect())
}

fn command_params(v: Option<&Value>) -> Result<Value, String> {
    match v {
        None | Some(Value::Null) => Ok(json!({})),
        Some(v @ Value::Object(_)) => Ok(v.clone()),
        Some(_) => Err("`params` must be an object".into()),
    }
}

fn filter_commands(all: Value, a: &Args) -> Value {
    let needle = a.get("filter").and_then(Value::as_str).map(str::to_lowercase).filter(|s| !s.is_empty());
    let enabled_only = a.get("enabledOnly").and_then(Value::as_bool).unwrap_or(false);
    let Value::Array(list) = all else { return all };
    let hit = |c: &Value| {
        if enabled_only && c.get("enabled").and_then(Value::as_bool) == Some(false) {
            return false;
        }
        let Some(n) = &needle else { return true };
        let field = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_lowercase();
        let menu = c
            .get("menu")
            .and_then(Value::as_array)
            .map(|m| m.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
            .to_lowercase();
        field("id").contains(n) || field("label").contains(n) || menu.contains(n)
    };
    Value::Array(list.into_iter().filter(hit).collect())
}

fn batch(b: &mut dyn Backend, a: &Args) -> Result<ToolResult, String> {
    use designcraft_engine::script;
    let steps = match (a.get("commands").and_then(Value::as_array), a.get("script").and_then(Value::as_str)) {
        (Some(c), _) if !c.is_empty() => script::parse(&Value::Array(c.clone()).to_string())?,
        (_, Some(t)) => script::parse(t)?,
        _ => return Err("give a non-empty `commands` array or `script` text".into()),
    };
    if steps.is_empty() {
        return Err("the script has no steps".into());
    }
    let report = script::run(&steps, |id, p| exec(b, id, p));
    let v = report.to_json();
    Ok(if report.failed.is_some() { ToolResult { is_error: true, ..ToolResult::json(&v) } } else { ToolResult::json(&v) })
}

/// The story id from `story` or the text frame `frame` (else the selection).
fn story_id(b: &mut dyn Backend, a: &Args) -> Result<Value, String> {
    if let Some(s) = a.get("story").filter(|v| v.is_u64()) {
        return Ok(s.clone());
    }
    let r = exec(b, "story.get", pick(a, &["frame"]))?;
    r.get("id").cloned().ok_or_else(|| "no story found (give `story` or a text `frame`)".into())
}

fn render_page(b: &mut dyn Backend, a: &Args) -> Result<ToolResult, String> {
    let r = b.call("ui.render", pick(a, &["page", "scale", "bleed"]))?;
    let b64 = r.get("pngBase64").and_then(Value::as_str).ok_or("renderer returned no image")?.to_string();
    let path = a.get("path").and_then(Value::as_str);
    if let Some(path) = path {
        let png = designcraft_engine::cmd::base64_decode(&b64);
        std::fs::write(path, png).map_err(|e| format!("write {path}: {e}"))?;
    }
    Ok(ToolResult::image(b64, &json!({"width": r.get("width"), "height": r.get("height"), "path": path})))
}

fn screenshot(b: &mut dyn Backend, a: &Args) -> Result<ToolResult, String> {
    need_ui(b, "screenshot")?;
    // The app writes the capture to disk (loopback: same machine), then we read it back.
    let keep = a.get("path").and_then(Value::as_str);
    let target = match keep {
        Some(p) => p.to_string(),
        None => std::env::temp_dir()
            .join(format!("designcraft-mcp-{}-{}.png", std::process::id(), std::time::SystemTime::UNIX_EPOCH.elapsed().map_or(0, |d| d.as_millis())))
            .to_string_lossy()
            .to_string(),
    };
    let r = b.call("ui.screenshot", json!({"path": target}))?;
    let png = std::fs::read(&target).map_err(|e| format!("read {target}: {e}"))?;
    if keep.is_none() {
        std::fs::remove_file(&target).ok();
    }
    Ok(ToolResult::image(designcraft_engine::cmd::base64_encode(&png), &json!({"width": r.get("width"), "height": r.get("height"), "path": keep})))
}

fn pointer(b: &mut dyn Backend, a: &Args) -> Result<Value, String> {
    let events = a.get("events").and_then(Value::as_array).filter(|e| !e.is_empty()).ok_or("`events` must be a non-empty array")?;
    for (i, e) in events.iter().enumerate() {
        let kind = e.get("kind").and_then(Value::as_str).unwrap_or("");
        if crate::headless::pointer_kind(kind).is_none() {
            return Err(format!("event {i}: unknown kind `{kind}` (down, drag, up, move, doubleclick)"));
        }
        if !e.get("x").is_some_and(Value::is_number) || !e.get("y").is_some_and(Value::is_number) {
            return Err(format!("event {i}: numeric `x` and `y` required"));
        }
    }
    if let Some(t) = a.get("tool").and_then(Value::as_str) {
        b.call("ui.tool.select", json!({"tool": t}))?;
    }
    b.call("ui.pointer", pick(a, &["events", "mods"]))
}

fn dispatch(b: &mut dyn Backend, name: &str, a: &Args) -> Result<ToolResult, String> {
    let j = |v: Value| Ok(ToolResult::json(&v));
    match name {
        "list_commands" => {
            let all = b.call("engine.commands", json!({}))?;
            let list = filter_commands(all, a);
            j(json!({"count": list.as_array().map_or(0, Vec::len), "commands": list}))
        }
        "execute" => {
            let cmd = req_str(a, "command")?;
            let params = command_params(a.get("params"))?;
            j(exec(b, cmd, params)?)
        }
        "batch" => batch(b, a),
        "inspect_document" => j(b.call("document.inspect", json!({}))?),
        "get_story" => j(exec(b, "story.get", pick(a, &["story", "frame"]))?),
        "set_story_text" => {
            let text = a.get("text").and_then(Value::as_str).ok_or("missing string argument `text`")?.to_string();
            let sid = story_id(b, a)?;
            exec(b, "story.setText", json!({"story": sid, "text": text}))?;
            j(exec(b, "story.get", json!({"story": sid}))?)
        }
        "new_document" => {
            if a.get("sample").and_then(Value::as_bool) == Some(true) {
                return j(exec(b, "file.newSample", json!({}))?);
            }
            let mut p = a.clone();
            p.remove("sample");
            j(exec(b, "file.new", Value::Object(p))?)
        }
        "open_document" => j(b.call("app.open", json!({"path": req_str(a, "path")?}))?),
        "save_document" => j(exec(b, "file.save", pick(a, &["path"]))?),
        "place_image" => {
            if !a.contains_key("path") && !a.contains_key("base64") {
                return Err("give `path` or `base64`".into());
            }
            j(exec(b, "file.place", pick(a, &["path", "base64", "name", "frame", "spread", "x", "y", "width"]))?)
        }
        "render_page" => render_page(b, a),
        "export_png" => {
            req_str(a, "path")?;
            j(b.call("app.export", pick(a, &["path", "page", "scale"]))?)
        }
        "screenshot" => screenshot(b, a),
        "select_tool" => j(b.call("ui.tool.select", json!({"tool": req_str(a, "tool")?}))?),
        "pointer" => j(pointer(b, a)?),
        "key" => {
            req_str(a, "key")?;
            j(b.call("ui.key", pick(a, &["key", "shift", "alt", "cmd", "ctrl"]))?)
        }
        "type_text" => j(b.call("ui.text", json!({"text": req_str(a, "text")?}))?),
        "click" | "drag" => {
            need_ui(b, name)?;
            let keys: &[&str] = &["x", "y", "toX", "toY", "steps", "button", "count", "shift", "alt", "cmd", "ctrl"];
            j(b.call(if name == "click" { "ui.click" } else { "ui.drag" }, pick(a, keys))?)
        }
        "menu_list" => {
            need_ui(b, name)?;
            j(b.call("ui.menu.list", json!({}))?)
        }
        "ui_inspect" => {
            need_ui(b, name)?;
            j(b.call("ui.inspect", json!({}))?)
        }
        "ui_set" => {
            need_ui(b, name)?;
            j(b.call("ui.set", Value::Object(a.clone()))?)
        }
        "dialog_set" => {
            need_ui(b, name)?;
            j(b.call("ui.dialog.set", json!({"field": req_str(a, "field")?, "value": a.get("value")}))?)
        }
        "dialog_confirm" => {
            need_ui(b, name)?;
            j(b.call("ui.dialog.confirm", json!({}))?)
        }
        "dialog_cancel" => {
            need_ui(b, name)?;
            j(b.call("ui.dialog.cancel", json!({}))?)
        }
        other => Err(format!("unknown tool `{other}` (see tools/list)")),
    }
}

/// Run one tool. Failures (unknown tool, bad arguments, command errors) come back as an
/// `isError` result so the model can read and correct them.
pub fn call_tool(b: &mut dyn Backend, name: &str, args: &Value) -> ToolResult {
    let empty = Map::new();
    let a = match args {
        Value::Object(o) => o,
        Value::Null => &empty,
        _ => return ToolResult::error("tool arguments must be a JSON object"),
    };
    match dispatch(b, name, a) {
        Ok(r) => r,
        Err(e) => ToolResult::error(format!("{name}: {e}")),
    }
}
