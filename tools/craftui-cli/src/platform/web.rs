use std::io::{Read, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use craftui_core::{CraftError, Result};
use crate::doctor::find_in_path;
use crate::manifest::xml_escape;

pub fn mime_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

pub fn resolve_static_asset_path(root: &Path, request_target: &str) -> Option<PathBuf> {
    let clean_target = request_target.split('?').next().unwrap_or("/");
    let trimmed = clean_target.trim_start_matches('/');
    let rel = if trimmed.is_empty() {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(trimmed)
    };
    if rel.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    let joined = root.join(&rel);
    if joined.is_dir() {
        Some(joined.join("index.html"))
    } else {
        Some(joined)
    }
}

pub fn build_http_response(root: &Path, request_target: &str) -> Vec<u8> {
    let Some(asset_path) = resolve_static_asset_path(root, request_target) else {
        let body = b"403 Forbidden";
        return format!(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            std::str::from_utf8(body).unwrap_or_default()
        )
        .into_bytes();
    };
    match std::fs::read(&asset_path) {
        Ok(bytes) => {
            let mime = mime_type_for_path(&asset_path);
            let mut resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .into_bytes();
            resp.extend_from_slice(&bytes);
            resp
        }
        Err(_) => {
            let body = b"404 Not Found";
            format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n404 Not Found",
                body.len()
            )
            .into_bytes()
        }
    }
}

fn handle_http_connection(mut stream: TcpStream, root: &Path) {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let target = req
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let resp = build_http_response(root, target);
    let _ = stream.write_all(&resp);
    let _ = stream.flush();
}

pub fn serve_web_directory(root: &Path, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .map_err(|e| CraftError::Platform(format!("failed binding http server on {addr}: {e}")))?;
    println!(
        "Serving CraftUI Web bundle ({}) at http://{} (Ctrl+C to stop)",
        root.display(),
        addr
    );
    for stream in listener.incoming().flatten() {
        let dir = root.to_path_buf();
        std::thread::spawn(move || handle_http_connection(stream, &dir));
    }
    Ok(())
}


pub fn emit_web_bundle(out_dir: &Path, app_name: &str, built_wasm: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CraftError::Platform(format!("failed creating {}: {e}", out_dir.display())))?;

    let mut wasm_bindgen_ran = false;
    if let Some(wasm_path) = built_wasm
        && wasm_path.is_file()
    {
        let raw_dest = out_dir.join(
            wasm_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("craftui_app.wasm")),
        );
        std::fs::copy(wasm_path, &raw_dest).map_err(|e| {
            CraftError::Platform(format!("failed copying wasm {}: {e}", wasm_path.display()))
        })?;

        if find_in_path("wasm-bindgen").is_some() {
            let status = Command::new("wasm-bindgen")
                .arg(wasm_path)
                .args(["--target", "web", "--out-name", "craftui_app", "--out-dir"])
                .arg(out_dir)
                .status()
                .map_err(|e| CraftError::Platform(format!("wasm-bindgen failed: {e}")))?;
            if !status.success() {
                return Err(CraftError::Platform(format!(
                    "wasm-bindgen exited with status {status}"
                )));
            }
            wasm_bindgen_ran = true;

            let bg_wasm = out_dir.join("craftui_app_bg.wasm");
            if bg_wasm.is_file() && find_in_path("wasm-opt").is_some() {
                let _ = Command::new("wasm-opt")
                    .arg("-O3")
                    .arg(&bg_wasm)
                    .arg("-o")
                    .arg(&bg_wasm)
                    .status();
            }
        }
    }

    let safe_title = xml_escape(app_name);
    let index_html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
  <title>{safe_title} — CraftUI</title>
  <style>
    html, body {{
      margin: 0;
      padding: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
      background: #0b0e14;
      color: #f8fafc;
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    #craftui-canvas {{
      width: 100vw;
      height: 100vh;
      display: block;
      outline: none;
    }}
    #craftui-status {{
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 14px;
      pointer-events: none;
      opacity: 0.78;
    }}
  </style>
</head>
<body>
  <canvas id="craftui-canvas" tabindex="0"></canvas>
  <div id="craftui-status">Loading {safe_title}...</div>
  <script type="module">
    const status = document.getElementById("craftui-status");
    try {{
      const mod = await import("./craftui_app.js");
      await mod.default();
      status.remove();
    }} catch (err) {{
      status.textContent = "CraftUI WASM bundle pending wasm-bindgen output (./craftui_app.js): " + err;
    }}
  </script>
</body>
</html>
"#
    );

    let readme = format!(
        "# {app_name} (Web / WASM Bundle)\n\nGenerated by `craftui build --platform web`.\n{}\nServe locally with built-in HTTP/WASM MIME server:\n```sh\ncraftui run --platform web --skip-cargo\n```\n",
        if wasm_bindgen_ran {
            "JS glue (`craftui_app.js`) and `craftui_app_bg.wasm` generated via `wasm-bindgen`.\n"
        } else {
            "Note: `wasm-bindgen` was not detected on PATH, so only raw HTML + `.wasm` (if built) were written.\nInstall `cargo install wasm-bindgen-cli` and run `wasm-bindgen <target.wasm> --target web --out-name craftui_app --out-dir .` inside this directory.\n"
        }
    );

    std::fs::write(out_dir.join("index.html"), index_html)
        .and_then(|_| std::fs::write(out_dir.join("README.md"), readme))
        .map_err(|e| CraftError::Platform(format!("failed writing web bundle: {e}")))?;
    Ok(())
}

