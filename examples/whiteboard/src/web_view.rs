use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use craftui::prelude::*;
use serde_json::json;

use crate::canvas::WhiteboardController;

/// Embedded responsive HTML5 Whiteboard web application.
/// Communicates seamlessly with Rust backend via Tauri/CraftUI IPC.
pub const WHITEBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
<title>CraftUI Excalidraw</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; user-select: none; }
  body, html { width: 100%; height: 100%; overflow: hidden; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; background: #ffffff; }
  #app { position: relative; width: 100%; height: 100%; display: flex; flex-direction: column; }
  
  /* Top-Left Hamburger */
  #hamburger {
    position: absolute; top: 14px; left: 14px; width: 40px; height: 40px;
    background: #ffffff; border: 1px solid #e0e0e0; border-radius: 8px;
    display: flex; align-items: center; justify-content: center; font-size: 18px;
    cursor: pointer; box-shadow: 0 1px 4px rgba(0,0,0,0.06); z-index: 100;
  }
  
  /* Top Floating Island */
  #toolbar {
    position: absolute; top: 14px; left: 50%; transform: translateX(-50%);
    display: flex; gap: 2px; padding: 4px 6px; background: #ffffff;
    border: 1px solid #e0e0e0; border-radius: 12px; box-shadow: 0 2px 8px rgba(0,0,0,0.08);
    z-index: 100; align-items: center;
  }
  .tool-btn {
    width: 36px; height: 36px; border: none; border-radius: 8px; background: transparent;
    font-size: 15px; cursor: pointer; display: flex; align-items: center; justify-content: center;
    color: #333333; transition: all 0.12s ease;
  }
  .tool-btn:hover { background: #f1f1f5; }
  .tool-btn.active { background: #e0dfff; color: #4338ca; font-weight: bold; }
  .divider { width: 1px; height: 20px; background: #e0e0e0; margin: 0 4px; }
  
  /* Hint banner below top island */
  #hint-banner {
    position: absolute; top: 62px; left: 50%; transform: translateX(-50%);
    display: flex; align-items: center; gap: 4px; font-size: 12px; color: #868e96;
    z-index: 90; pointer-events: none;
  }
  .key-badge {
    background: #ffffff; border: 1px solid #ced4da; border-radius: 4px;
    padding: 1px 5px; font-size: 11px; color: #495057;
  }

  /* Top-Right Actions */
  #top-right {
    position: absolute; top: 14px; right: 14px; display: flex; gap: 10px; align-items: center;
    z-index: 100;
  }
  .try-plus-btn {
    padding: 8px 12px; background: #f5f3ff; border: 1px solid #c7d2fe; border-radius: 10px;
    color: #4f46e5; font-size: 12px; font-weight: 600; cursor: pointer; display: flex; gap: 6px; align-items: center;
  }
  .share-btn {
    padding: 8px 14px; background: #4f46e5; border: none; border-radius: 8px;
    color: #ffffff; font-size: 13px; font-weight: 500; cursor: pointer; display: flex; gap: 6px; align-items: center;
  }
  
  /* Canvas */
  #canvas { width: 100%; height: 100%; display: block; touch-action: none; }
  
  /* Bottom-Left Zoom & Undo/Redo */
  #bottom-left {
    position: absolute; bottom: 16px; left: 16px; display: flex; gap: 8px; z-index: 100;
  }
  .island-pill {
    background: #ffffff; border: 1px solid #e0e0e0; border-radius: 8px;
    padding: 4px 6px; display: flex; gap: 4px; align-items: center; box-shadow: 0 1px 4px rgba(0,0,0,0.06);
  }
  .pill-btn {
    border: none; background: transparent; font-size: 12px; color: #333333;
    padding: 4px 6px; border-radius: 4px; cursor: pointer;
  }
  .pill-btn:hover { background: #f0f0f5; }

  /* Bottom-Right Badges */
  #bottom-right {
    position: absolute; bottom: 16px; right: 16px; display: flex; gap: 10px; align-items: center;
    z-index: 100;
  }
  .round-btn {
    width: 32px; height: 32px; background: #ffffff; border: 1px solid #e0e0e0; border-radius: 8px;
    display: flex; align-items: center; justify-content: center; cursor: pointer; font-size: 14px;
    box-shadow: 0 1px 4px rgba(0,0,0,0.06);
  }
</style>
</head>
<body>
<div id="app">
  <!-- Top-Left Hamburger -->
  <button id="hamburger" title="Menu">☰</button>

  <!-- Top-Center Floating Island -->
  <div id="toolbar">
    <button class="tool-btn" data-tool="lock" title="Lock Tool">🔓</button>
    <button class="tool-btn" data-tool="hand" title="Hand (H)">✋</button>
    <button class="tool-btn active" data-tool="select" title="Selection (V)">↖</button>
    <button class="tool-btn" data-tool="rectangle" title="Rectangle (R)">▭</button>
    <button class="tool-btn" data-tool="diamond" title="Diamond (D)">◇</button>
    <button class="tool-btn" data-tool="ellipse" title="Ellipse (O)">◯</button>
    <button class="tool-btn" data-tool="arrow" title="Arrow (A)">➔</button>
    <button class="tool-btn" data-tool="line" title="Line (L)">—</button>
    <button class="tool-btn" data-tool="pen" title="Draw (P)">✏</button>
    <button class="tool-btn" data-tool="text" title="Text (T)">A</button>
    <button class="tool-btn" data-tool="card" title="Card (9)">▤</button>
    <button class="tool-btn" data-tool="eraser" title="Eraser (E)">⌫</button>
    <div class="divider"></div>
    <button class="tool-btn" data-tool="more" title="More Tools">⋮</button>
  </div>

  <!-- Hint banner -->
  <div id="hint-banner">
    To move canvas, hold <span class="key-badge">Scroll wheel</span> or <span class="key-badge">Space</span> while dragging, or use the hand tool
  </div>

  <!-- Top-Right Actions -->
  <div id="top-right">
    <button class="try-plus-btn">🎁 Try Excalidraw+</button>
    <button class="share-btn">↗ Share</button>
  </div>

  <!-- Fullscreen Infinite Canvas -->
  <canvas id="canvas"></canvas>

  <!-- Bottom-Left Zoom & Undo/Redo -->
  <div id="bottom-left">
    <div class="island-pill">
      <button class="pill-btn" id="zoom-lbl">100%</button>
      <button class="pill-btn">🗺</button>
    </div>
    <div class="island-pill">
      <button class="pill-btn" id="btn-undo">↶</button>
      <button class="pill-btn" id="btn-redo">↷</button>
    </div>
  </div>

  <!-- Bottom-Right Badges -->
  <div id="bottom-right">
    <div class="round-btn" title="Security" style="color:#4f46e5;">🛡</div>
    <div class="round-btn" title="Help" style="font-weight:bold; color:#666;">?</div>
  </div>
</div>

<script>
  // High-performance canvas drawing loop with sketchy strokes
  const canvas = document.getElementById('canvas');
  const ctx = canvas.getContext('2d');
  let activeTool = 'select';
  let elements = [];
  let isDown = false;
  let startX = 0, startY = 0;
  let currentStroke = [];

  function resize() {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    render();
  }
  window.addEventListener('resize', resize);

  document.querySelectorAll('.tool-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const tool = btn.getAttribute('data-tool');
      if (tool === 'clear') {
        elements = [];
        render();
        syncToRust();
        return;
      }
      document.querySelectorAll('.tool-btn').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      activeTool = tool;
    });
  });

  canvas.addEventListener('pointerdown', e => {
    isDown = true;
    startX = e.clientX;
    startY = e.clientY;
    currentStroke = [{ x: startX, y: startY }];
  });

  canvas.addEventListener('pointermove', e => {
    if (!isDown) return;
    if (activeTool === 'pen') {
      currentStroke.push({ x: e.clientX, y: e.clientY });
      render();
    } else if (activeTool !== 'select') {
      render();
      drawPreview(e.clientX, e.clientY);
    }
  });

  canvas.addEventListener('pointerup', e => {
    if (!isDown) return;
    isDown = false;
    const endX = e.clientX;
    const endY = e.clientY;

    if (activeTool === 'rectangle' && (Math.abs(endX - startX) > 4 || Math.abs(endY - startY) > 4)) {
      elements.push({ type: 'rect', x: Math.min(startX, endX), y: Math.min(startY, endY), w: Math.abs(endX - startX), h: Math.abs(endY - startY) });
    } else if (activeTool === 'arrow') {
      elements.push({ type: 'arrow', x1: startX, y1: startY, x2: endX, y2: endY });
    } else if (activeTool === 'ellipse') {
      elements.push({ type: 'ellipse', cx: (startX + endX)/2, cy: (startY + endY)/2, rx: Math.abs(endX - startX)/2, ry: Math.abs(endY - startY)/2 });
    } else if (activeTool === 'diamond') {
      elements.push({ type: 'diamond', x: Math.min(startX, endX), y: Math.min(startY, endY), w: Math.abs(endX - startX), h: Math.abs(endY - startY) });
    } else if (activeTool === 'line') {
      elements.push({ type: 'line', x1: startX, y1: startY, x2: endX, y2: endY });
    } else if (activeTool === 'pen' && currentStroke.length > 1) {
      elements.push({ type: 'pen', pts: [...currentStroke] });
    }

    render();
    syncToRust();
  });

  function render() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    // Subtle dot grid
    ctx.fillStyle = '#e2e8f0';
    const step = 28;
    for (let x = 0; x < canvas.width; x += step) {
      for (let y = 0; y < canvas.height; y += step) {
        ctx.fillRect(x, y, 2, 2);
      }
    }

    ctx.strokeStyle = '#1e1e1e';
    ctx.lineWidth = 2.5;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    for (const el of elements) {
      drawElement(el);
    }

    if (isDown && activeTool === 'pen' && currentStroke.length > 1) {
      ctx.beginPath();
      ctx.moveTo(currentStroke[0].x, currentStroke[0].y);
      for (let i = 1; i < currentStroke.length; i++) {
        ctx.lineTo(currentStroke[i].x, currentStroke[i].y);
      }
      ctx.stroke();
    }

    document.getElementById('elem-count').innerText = `${elements.length} elements`;
  }

  function drawElement(el) {
    ctx.beginPath();
    if (el.type === 'rect') {
      ctx.strokeRect(el.x, el.y, el.w, el.h);
    } else if (el.type === 'diamond') {
      ctx.moveTo(el.x + el.w/2, el.y);
      ctx.lineTo(el.x + el.w, el.y + el.h/2);
      ctx.lineTo(el.x + el.w/2, el.y + el.h);
      ctx.lineTo(el.x, el.y + el.h/2);
      ctx.closePath();
      ctx.stroke();
    } else if (el.type === 'ellipse') {
      ctx.ellipse(el.cx, el.cy, el.rx, el.ry, 0, 0, Math.PI * 2);
      ctx.stroke();
    } else if (el.type === 'arrow') {
      ctx.moveTo(el.x1, el.y1);
      ctx.lineTo(el.x2, el.y2);
      const angle = Math.atan2(el.y2 - el.y1, el.x2 - el.x1);
      ctx.lineTo(el.x2 - 18 * Math.cos(angle - 0.45), el.y2 - 18 * Math.sin(angle - 0.45));
      ctx.moveTo(el.x2, el.y2);
      ctx.lineTo(el.x2 - 18 * Math.cos(angle + 0.45), el.y2 - 18 * Math.sin(angle + 0.45));
      ctx.stroke();
    } else if (el.type === 'line') {
      ctx.moveTo(el.x1, el.y1);
      ctx.lineTo(el.x2, el.y2);
      ctx.stroke();
    } else if (el.type === 'pen' && el.pts) {
      ctx.moveTo(el.pts[0].x, el.pts[0].y);
      for (let i = 1; i < el.pts.length; i++) {
        ctx.lineTo(el.pts[i].x, el.pts[i].y);
      }
      ctx.stroke();
    }
  }

  function drawPreview(x, y) {
    ctx.save();
    ctx.strokeStyle = '#6366f1';
    ctx.setLineDash([6, 6]);
    ctx.beginPath();
    if (activeTool === 'rectangle') {
      ctx.strokeRect(Math.min(startX, x), Math.min(startY, y), Math.abs(x - startX), Math.abs(y - startY));
    } else if (activeTool === 'arrow') {
      ctx.moveTo(startX, startY);
      ctx.lineTo(x, y);
      ctx.stroke();
    }
    ctx.restore();
  }

  function syncToRust() {
    if (window.__TAURI__ && window.__TAURI__.core) {
      window.__TAURI__.core.invoke('sync_elements', { count: elements.length });
    }
  }

  // Load initial state from Rust
  window.addEventListener('load', () => {
    resize();
    if (window.__TAURI__ && window.__TAURI__.core) {
      window.__TAURI__.core.invoke('get_whiteboard_status').then(res => {
        console.log('Rust bridge initialized:', res);
      });
    }
  });
</script>
</body>
</html>
"#;

thread_local! {
    static WHITEBOARD_CTRL: RefCell<Option<WhiteboardController>> = const { RefCell::new(None) };
}

/// Create an IPC Router for bidirectional communication between the webview whiteboard and Rust.
pub fn create_whiteboard_ipc_router(
    ctrl: WhiteboardController,
    services: Option<Arc<PlatformServices>>,
) -> Rc<RefCell<IpcRouter>> {
    WHITEBOARD_CTRL.with(|c| {
        *c.borrow_mut() = Some(ctrl);
    });

    let mut router = IpcRouter::new();

    router.register("get_whiteboard_status", |_payload| {
        WHITEBOARD_CTRL.with(|cell| {
            if let Some(ctrl) = cell.borrow().as_ref() {
                let count = ctrl.elements.get().len();
                let tool = ctrl.tool.get().name();
                let zoom = ctrl.view.get().zoom;
                Ok(json!({
                    "status": "ok",
                    "element_count": count,
                    "tool": tool,
                    "zoom": zoom,
                    "hybrid_bridge": "Tauri-Compatible CraftUI IPC",
                }))
            } else {
                Err("Whiteboard controller not initialized".into())
            }
        })
    });

    router.register("sync_elements", |payload| {
        let count = payload.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        let _ = count;
        WHITEBOARD_CTRL.with(|cell| {
            let count = cell
                .borrow()
                .as_ref()
                .map(|c| c.elements.get().len())
                .unwrap_or(0);
            Ok(json!({ "acknowledged": true, "synced_elements": count }))
        })
    });

    let services_perm = services.clone();
    router.register("query_platform_security", move |_payload| {
        if let Some(svc) = &services_perm {
            let admin_status = svc.check_permission(&Permission::AdminPrivileges);
            let camera_status = svc.check_permission(&Permission::Camera);
            let storage_status = svc.check_permission(&Permission::StorageRead);
            Ok(json!({
                "admin": format!("{:?}", admin_status),
                "camera": format!("{:?}", camera_status),
                "storage": format!("{:?}", storage_status),
                "accessibility_settings_available": true,
                "admin_settings_available": true,
            }))
        } else {
            Ok(json!({ "status": "no_services" }))
        }
    });

    Rc::new(RefCell::new(router))
}
