# DesignCraft — instructions for agents

DesignCraft is a clean-room, open-source, Rust-native page-layout application targeting Adobe InDesign parity — and superiority (speed, openness, agent control). It runs natively on macOS, Windows and Linux, and on the web via WASM. Siblings with the same conventions: `../drawcraft` (Illustrator-class), `../photocraft` (Photoshop), `../pdfcraft` (Acrobat), `../filmcraft` (Premiere), `../lightcraft` (Lightroom).

## Failure handling and document preservation

Changes must not introduce known panic paths in the operation being modified. Invalid input and unsupported operations must return an explicit error rather than silently substituting document data.
If a reproducible crash prevents completing or verifying the requested change, fix it first and add a regression test. Report unrelated crashes without expanding the task.
Failed commands and imports must preserve the last valid document state. Do not claim this is verified solely because a panic was caught.
Follow the repository’s enforced restrictions on panic-producing APIs, indexing, arithmetic, recursion, and allocation.

## Non-negotiables

- **Expose application operations as commands.** Implement document operations in
  `crates/engine/src/cmd/*`. Each command declares its ID, label, menu path, shortcut
  where applicable, parameters, availability check (`enabled`), and implementation
  (`run`), with tests for its behavior. Menus, panels, tools and automation must use
  these commands to perform the same operation. Interactive tools use Begin/Preview/Commit
  stages to support previews, cancellation and a committed edit.

- **Keep interface commands accessible to agents.** Register UI operations, such as
  zooming or arranging panels, in `UI_COMMANDS` in `crates/ui-egui/src/menus.rs`.
  Both engine and UI commands must be discoverable and callable through the running
  app's control channel and connected MCP. UI commands require the application UI;
  they are unavailable in headless MCP. “UI-only” identifies the implementation's
  location and runtime requirement, not a restriction to human use.

- **Keep the engine independent of the GUI stack.** Crates below the L6 UI layer
  must not depend on `egui`, `eframe`, `winit` or `rfd`. Keep windowing, widgets and
  native file dialogs in the UI layer; pass their results to the engine. Follow the
  dependency boundaries checked by `cargo xtask layers`. This allows headless use
  and replacement of the frontend without moving document behavior into a new UI.

- **Put document behavior in the engine and presentation behavior in the UI.**
  Panels read engine state and invoke `app.run(id, params)` for operations; they
  must not independently implement document mutation, validation or undo behavior.
  Focus, scrolling, dialog state and other presentation logic belong in the UI.
  Use `theme::Tokens` for application-interface colours. Document artwork and
  user-selected document colours retain their own values.

- **Implement application behavior in Rust.** Do not add handwritten JavaScript
  or TypeScript. Toolchain-generated browser glue is permitted. Existing build,
  packaging and development scripts are outside the application-language restriction.

## Running and looking at the app

Run these commands from the repository root. Cargo builds and runs the requested
binary; the examples do not require `designcraft-cli` to be installed on PATH.

- **Launch the desktop app with the sample magazine and control server:**

  ```sh
  cargo run --release -p designcraft -- --sample --control 7979
  ```

- **Control the running app:** send newline-delimited JSON over TCP to
  `127.0.0.1:7979`. For example, create a text frame and then capture the window:

  ```json
  {"id":1,"method":"engine.execute","params":{"command":"frame.create","params":{"rect":[36,36,300,200],"content":"text"}}}
  {"id":2,"method":"ui.screenshot","params":{"path":"/tmp/designcraft-window.png"}}
  ```

  Inspect each response for errors. Methods are implemented in
  `crates/ui-egui/src/control.rs` and documented in `docs/control-protocol.md`.
  Choose output paths that do not overwrite files you need to preserve.

- **Verify UI changes with a whole-window image:** request `ui.screenshot`, then
  open and inspect the resulting PNG. This captures the interface, including
  panels and dialogs. It can time out when no frame is presented, such as when
  the screen is locked or the window is hidden. The old comparison references
  under `plan/indesign/` are absent from this checkout; do not claim comparison
  against them unless those files are available and have actually been inspected.

- **Render the whole UI offscreen when a window screenshot is unavailable:**
  create a JSON-lines script that reproduces the state to inspect, ending with
  a screenshot instruction such as:

  ```json
  {"shot":"/tmp/designcraft-offscreen.png"}
  ```

  Run it with:

  ```sh
  cargo run -p designcraft-ui-egui --example ui_shot -- script.jsonl
  ```

  This uses the offscreen wgpu test harness and requires a working wgpu backend.
  It creates a separate app instance with the sample document by default; it does
  not capture the current desktop session. The script accepts control-channel
  requests, `{"steps": n}` to advance frames, and `{"shot": "/absolute/path.png"}`
  to save the UI. A first line of `{"empty": true}` disables the initial sample.
  Inspect the output for errors and open the saved PNG to verify the result.

- **Inspect document output separately from the interface:** `ui.render` renders
  a document page from the running app without requiring a presented frame. It
  does not capture panels, dialogs or editing overlays. For an independent
  headless document render, use:

  ```sh
  cargo run --release -p designcraft-cli -- run --sample \
    --cmd 'frame.create={"rect":[0,0,100,100]}' --export out.png
  cargo run --release -p designcraft-cli -- run --sample --all-pages out-pages
  ```

  The second command writes `page-001.png`, `page-002.png`, and so on. These page
  renders verify document appearance, not UI appearance or interaction.

- **Shell commands:** do not assume `mv` or `cp` is aliased. Check with `type mv cp`
  if behavior is unclear; do not add `-f` merely to bypass an assumed alias.

- **When using parallel agents:** assign separate `CARGO_TARGET_DIR` values and
  explicit file/crate ownership. Only remove a target directory after verifying
  that it was created exclusively for that agent's disposable build output and
  that no build is still using it. Do not remove shared or pre-existing targets.
