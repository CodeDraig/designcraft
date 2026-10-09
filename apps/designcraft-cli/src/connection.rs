//! MCP and running-application control connections.

use super::{Value, json};

/// `mcp` (headless, in-process engine) or `mcp --connect PORT|HOST:PORT` (drive a running app
/// started with `designcraft --control PORT`). JSON-RPC on stdin/stdout; logs on stderr.
pub(super) fn mcp(args: &[String]) -> Result<(), String> {
    use designcraft_mcp::{Backend, Headless, Remote, Server, control_addr};
    let mut connect: Option<String> = None;
    let mut sample = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--connect" => connect = Some(it.next().cloned().ok_or("--connect needs a port or host:port")?),
            "--sample" => sample = true,
            other => return Err(format!("unknown mcp option `{other}` (usage: designcraft-cli mcp [--connect PORT] [--sample])")),
        }
    }
    let backend: Box<dyn Backend> = match connect {
        Some(c) => {
            let addr = control_addr(&c);
            Box::new(
                Remote::connect(&addr)
                    .map_err(|e| format!("cannot connect to the DesignCraft app at {addr}: {e} (start it with `designcraft --control PORT`)"))?,
            )
        }
        None => {
            let mut h = Headless::with_document();
            if sample {
                h.session.execute("file.newSample", &json!({})).map_err(|e| e.to_string())?;
            }
            Box::new(h)
        }
    };
    eprintln!("designcraft-cli: MCP server on stdio ({})", backend.describe());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

/// `app`: one command or control-channel method in the running app.
pub(super) fn app(args: &[String]) -> Result<(), String> {
    use designcraft_mcp::{Backend, Remote, control_addr};
    let mut port = "7979".to_string();
    let mut method: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" | "--connect" => port = it.next().cloned().ok_or("--port needs a value")?,
            "--method" => method = Some(it.next().cloned().ok_or("--method needs a value")?),
            _ => rest.push(a.clone()),
        }
    }
    let addr = control_addr(&port);
    let mut r = Remote::connect(&addr)
        .map_err(|e| format!("cannot connect to the DesignCraft app at {addr}: {e} (start it with `designcraft --control {port}`)"))?;
    let json_arg =
        |s: Option<&String>| -> Result<Value, String> { s.map_or(Ok(json!({})), |t| serde_json::from_str(t).map_err(|e| format!("bad JSON: {e}"))) };
    let out = match method {
        Some(m) => r.call(&m, json_arg(rest.first())?)?,
        None => {
            let cmd = rest.first().ok_or("usage: designcraft-cli app [--port PORT] COMMAND [JSON]")?;
            r.call("engine.execute", json!({"command": cmd, "params": json_arg(rest.get(1))?}))?
        }
    };
    outln!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}
