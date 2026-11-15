#!/usr/bin/env node
// End-to-end smoke test for a CraftUI web bundle, run in headless Chrome.
//
// CraftUI draws into a <canvas>, so there is no DOM to inspect for content.
// It does, however, mirror every widget into an accessibility tree
// (`#craftui-a11y-root`, one element per widget with `aria-label` and the
// widget's on-screen rectangle). This script drives the app through that tree,
// exactly as a screen reader user would find things, and sends real mouse and
// keyboard input through the Chrome DevTools Protocol.
//
// Usage: node web-smoke.mjs <bundle-dir> <out-dir>
//
// Needs Node 22+ (built-in WebSocket) and Chrome. No npm packages.
//
// Exit code 0 means every check passed. Screenshots, the console output and
// the final DOM are written to <out-dir> either way.

import { spawn } from "node:child_process";
import { createReadStream, existsSync, mkdirSync, mkdtempSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize, resolve, sep } from "node:path";

const [bundleDirArg, outDirArg] = process.argv.slice(2);
if (!bundleDirArg || !outDirArg) {
  console.error("usage: web-smoke.mjs <bundle-dir> <out-dir>");
  process.exit(2);
}
const bundleDir = resolve(bundleDirArg);
const outDir = resolve(outDirArg);
mkdirSync(outDir, { recursive: true });

const VIEWPORT = { width: 390, height: 800 };
const WELCOME = "Welcome to CraftUI Notes";
const TYPED_TITLE = "Smoke test note";
const STORAGE_KEY = "craftui-notes/v1";

const log = [];
const say = (line) => {
  console.log(line);
  log.push(line);
};
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// --- Static file server -----------------------------------------------------

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".css": "text/css; charset=utf-8",
  ".png": "image/png",
};

const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  const file = normalize(join(bundleDir, path === "/" ? "index.html" : path));
  // Require a separator boundary: a prefix check alone also accepts sibling
  // directories that share the bundle's name (for example
  // `/x/dist/notes-web-secret` for `bundleDir = /x/dist/notes-web`).
  const inside = file === bundleDir || file.startsWith(bundleDir + sep);
  if (!inside || !existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404).end("not found");
    return;
  }
  res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
  createReadStream(file).pipe(res);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const url = `http://127.0.0.1:${server.address().port}/`;

// --- Chrome -----------------------------------------------------------------

const chromePath =
  process.env.CHROME_BIN ||
  ["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find(existsSync);
if (!chromePath) {
  console.error("Chrome was not found. Set CHROME_BIN.");
  process.exit(2);
}

const chrome = spawn(
  chromePath,
  [
    "--headless=new",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--hide-scrollbars",
    "--no-first-run",
    "--enable-unsafe-swiftshader",
    "--remote-debugging-port=0",
    `--user-data-dir=${mkdtempSync(join(tmpdir(), "craftui-smoke-"))}`,
    `--window-size=${VIEWPORT.width},${VIEWPORT.height}`,
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "pipe"] },
);

// Startup below awaits (the DevTools URL and the WebSocket) and can reject. Node
// then exits on the unhandled rejection and the cleanup at the end never runs,
// so kill Chrome from an exit hook as well. The normal path still calls
// `chrome.kill()` before it exits, which makes this a no-op.
process.on("exit", () => chrome.kill());

const browserWsUrl = await new Promise((done, fail) => {
  let stderr = "";
  const timer = setTimeout(() => fail(new Error("Chrome did not start:\n" + stderr)), 30_000);
  chrome.stderr.on("data", (chunk) => {
    stderr += chunk;
    const match = stderr.match(/DevTools listening on (ws:\/\/\S+)/);
    if (match) {
      clearTimeout(timer);
      done(match[1]);
    }
  });
});

// --- Minimal DevTools Protocol client ---------------------------------------

const socket = new WebSocket(browserWsUrl);
await new Promise((done, fail) => {
  socket.addEventListener("open", done, { once: true });
  socket.addEventListener("error", fail, { once: true });
});

let nextId = 1;
const pending = new Map();
const consoleLines = [];
socket.addEventListener("message", (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    const { done, fail } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? fail(new Error(`${msg.error.message} ${msg.error.data ?? ""}`)) : done(msg.result);
  } else if (msg.method === "Runtime.consoleAPICalled") {
    const text = msg.params.args.map((a) => a.value ?? a.description ?? "").join(" ");
    consoleLines.push(`[console.${msg.params.type}] ${text}`);
  } else if (msg.method === "Runtime.exceptionThrown") {
    const d = msg.params.exceptionDetails;
    consoleLines.push(`[exception] ${d.exception?.description ?? d.text}`);
  }
});
const send = (method, params = {}, sessionId) =>
  new Promise((done, fail) => {
    const id = nextId++;
    pending.set(id, { done, fail });
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });

const { targetId } = await send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
const cdp = (method, params) => send(method, params, sessionId);

await cdp("Page.enable");
await cdp("Runtime.enable");
await cdp("Emulation.setDeviceMetricsOverride", { ...VIEWPORT, deviceScaleFactor: 1, mobile: false });

const evaluate = async (expression) => {
  const { result, exceptionDetails } = await cdp("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (exceptionDetails) {
    throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  }
  return result.value;
};

const screenshot = async (name) => {
  const { data } = await cdp("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, name), Buffer.from(data, "base64"));
};

/** Rectangle of the first accessibility element matching `selector`, or null. */
const rectOf = (selector) =>
  evaluate(`(() => {
    const el = document.querySelector(${JSON.stringify("#craftui-a11y-root " + selector)});
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  })()`);

const waitFor = async (what, probe, timeoutMs = 30_000) => {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await probe();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
};

const label = (text) => `[aria-label=${JSON.stringify(text)}]`;

const click = async (selector) => {
  const rect = await waitFor(`element ${selector}`, () => rectOf(selector));
  const x = rect.x + rect.width / 2;
  const y = rect.y + rect.height / 2;
  await cdp("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
  await cdp("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await sleep(150);
};

const load = async () => {
  await cdp("Page.navigate", { url });
  await waitFor("the accessibility tree", () => evaluate(`!!document.querySelector("#craftui-a11y-root [data-craftui-node]")`), 60_000);
};

// --- The checks -------------------------------------------------------------

const results = [];
const check = async (name, body) => {
  try {
    await body();
    results.push({ name, ok: true });
    say(`ok    ${name}`);
  } catch (err) {
    results.push({ name, ok: false, error: String(err.message ?? err) });
    say(`FAIL  ${name}: ${err.message ?? err}`);
  }
};

await check("the app starts and shows the welcome note", async () => {
  await load();
  await waitFor("the welcome note", () => rectOf(label(WELCOME)));
  await sleep(500);
  await screenshot("web-1-list.png");
});

await check("the canvas is painted (not a single flat colour)", async () => {
  const { data } = await cdp("Page.captureScreenshot", { format: "png" });
  // A flat image compresses to almost nothing; real UI with text does not.
  const bytes = Buffer.from(data, "base64").length;
  if (bytes < 6_000) throw new Error(`screenshot is only ${bytes} bytes, the page looks blank`);
});

await check("tapping + opens the editor", async () => {
  await click(label("plus"));
  // The delete button only exists in the editor toolbar.
  await waitFor("the editor toolbar", () => rectOf(label("trash")));
  await screenshot("web-2-editor.png");
});

await check("a phone-sized window shows one pane at a time", async () => {
  // The viewport is narrower than the 720dp breakpoint, so opening a note
  // must replace the list rather than sit beside it.
  const search = await rectOf(`input${label("Search notes")}`);
  if (search) throw new Error(`the list is still shown next to the editor (search field at x=${search.x})`);
  await waitFor("the back button", () => rectOf(label("chevron_left")));
});

await check("typing a title saves the note to localStorage", async () => {
  // Text fields are announced by their placeholder, which is also how the
  // test finds the title field.
  await click(`input${label("Title")}`);
  await cdp("Input.insertText", { text: TYPED_TITLE });
  await waitFor("the note in localStorage", () =>
    evaluate(`(localStorage.getItem(${JSON.stringify(STORAGE_KEY)}) ?? "").includes(${JSON.stringify(TYPED_TITLE)})`),
  );
  await screenshot("web-3-typed.png");
});

await check("the note is still there after a reload", async () => {
  await load();
  await waitFor("the typed note in the list", () => rectOf(label(TYPED_TITLE)));
  await screenshot("web-4-after-reload.png");
});

// --- Report -----------------------------------------------------------------

const dom = await evaluate("document.documentElement.outerHTML").catch((e) => `could not read DOM: ${e}`);
writeFileSync(join(outDir, "web-dom.html"), dom);
writeFileSync(join(outDir, "web-console.log"), consoleLines.join("\n") + "\n");
writeFileSync(join(outDir, "web-smoke.log"), log.join("\n") + "\n");
writeFileSync(join(outDir, "web-smoke.json"), JSON.stringify(results, null, 2) + "\n");

socket.close();
chrome.kill();
server.close();

const failed = results.filter((r) => !r.ok);
if (failed.length > 0) {
  console.error(`\n${failed.length} of ${results.length} checks failed.`);
  if (consoleLines.length > 0) console.error("\nBrowser console:\n" + consoleLines.join("\n"));
  process.exit(1);
}
console.log(`\nAll ${results.length} checks passed.`);
process.exit(0);
