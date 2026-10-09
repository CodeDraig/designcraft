//! Command script loading, execution, and output delivery.

use super::{Value, json};

/// `script`: run a command script headless (or in the running app with `--connect`).
pub(super) fn script(args: &[String]) -> Result<(), String> {
    use designcraft_mcp::{Backend, Headless, Remote, control_addr};
    let mut file: Option<String> = None;
    let mut setup: Vec<(String, Value)> = Vec::new();
    let mut connect: Option<String> = None;
    let mut save: Option<String> = None;
    let mut export: Vec<String> = Vec::new();
    let mut keep_going = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--in" => {
                let p = val()?;
                let cmd = if p.to_lowercase().ends_with(".idml") { "file.openIdml" } else { "file.open" };
                setup.push((cmd.into(), json!({"path": p})));
            }
            "--sample" => setup.push(("file.newSample".into(), json!({}))),
            "--connect" => connect = Some(val()?),
            "--save" => save = Some(val()?),
            "--export" => export.push(val()?),
            "--keep-going" => keep_going = true,
            f if !f.starts_with("--") && file.is_none() => file = Some(f.to_string()),
            other => return Err(format!("unknown script option `{other}`")),
        }
    }
    let text = match file.as_deref() {
        None | Some("-") => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).map_err(|e| e.to_string())?;
            t
        }
        Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
    };
    let steps = designcraft_engine::script::parse(&text)?;
    let mut backend: Box<dyn Backend> = match &connect {
        Some(c) => {
            let addr = control_addr(c);
            Box::new(
                Remote::connect(&addr)
                    .map_err(|e| format!("cannot connect to the DesignCraft app at {addr}: {e} (start it with `designcraft --control PORT`)"))?,
            )
        }
        None => Box::new(Headless::with_document()),
    };
    let b = &mut *backend;
    let exec = |b: &mut dyn Backend, id: &str, p: Value| b.call("engine.execute", json!({"command": id, "params": p}));
    for (c, p) in &setup {
        exec(b, c, p.clone())?;
    }
    let report = if keep_going {
        // Run every step; failures become {"error": …} results.
        let mut results: Vec<Value> = Vec::new();
        for st in &steps {
            let r = designcraft_engine::script::resolve(&st.params, &results).and_then(|p| exec(b, &st.command, p));
            results.push(r.unwrap_or_else(|e| json!({"error": e, "command": st.command})));
        }
        designcraft_engine::script::Report { results, failed: None }
    } else {
        designcraft_engine::script::run(&steps, |id, p| exec(b, id, p))
    };
    if report.failed.is_none() {
        if let Some(out) = &save {
            exec(b, "file.saveAs", json!({"path": out}))?;
        }
        for out in &export {
            let lower = out.to_lowercase();
            let p = json!({"path": out});
            if lower.ends_with(".pdf") {
                exec(b, "file.exportPdf", p)?;
            } else if lower.ends_with(".idml") {
                exec(b, "file.exportIdml", p)?;
            } else if lower.ends_with(".epub") {
                exec(b, "file.exportEpub", p)?;
            } else {
                // PNG / JPEG of the first page.
                b.call("app.export", p)?;
            }
        }
    }
    outln!("{}", serde_json::to_string_pretty(&report.to_json()).unwrap_or_default());
    match report.failed {
        Some((i, c, e)) => Err(format!("step {i} ({c}) failed: {e}")),
        None => Ok(()),
    }
}
