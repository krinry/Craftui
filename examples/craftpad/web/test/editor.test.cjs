const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '..', 'index.html'), 'utf8');
const script = html.match(/<script type="module">([\s\S]*?)<\/script>/)[1]
  .replace('import("./craftui_app.js")', '__import()');

async function setup(moduleFactory) {
  const elements = new Map();
  const element = (id) => {
    if (!elements.has(id)) elements.set(id, {
      value: '', textContent: '', innerHTML: '', style: {},
      classList: { add() {}, remove() {}, toggle() {} },
      addEventListener(type, callback) { this[type] = callback; },
    });
    return elements.get(id);
  };
  const timers = [];
  const context = {
    document: { getElementById: element },
    window: { dispatchEvent() {} },
    Event: class Event {},
    setTimeout: (fn) => { timers.push(fn); return timers.length; },
    clearTimeout: () => {},
    __import: moduleFactory,
  };
  await vm.runInNewContext(`(async () => { ${script} })()`, context);
  return { element, timers };
}

test('editor sends source to runtime, preserves state on reload and resets on restart', async () => {
  const calls = [];
  const { element, timers } = await setup(async () => ({
    default: async () => {}, init_preview() {},
    preview_source(source, reset) { calls.push({ source, reset }); },
  }));
  assert.equal(calls.length, 1);
  assert.match(calls[0].source, /export component Counter/);
  assert.equal(calls[0].reset, true);
  element('code-editor').value = 'updated source';
  element('code-editor').input();
  timers.at(-1)();
  assert.deepEqual(calls.at(-1), { source: 'updated source', reset: false });
  element('restart-btn').click();
  assert.deepEqual(calls.at(-1), { source: 'updated source', reset: true });
  element('sample-select').change({ target: { value: 'login' } });
  assert.match(calls.at(-1).source, /export component LoginForm/);
  assert.equal(calls.at(-1).reset, true);
});

test('reports validation failure and retains it across dock tabs', async () => {
  const { element } = await setup(async () => ({
    default: async () => {}, init_preview() {},
    preview_source() { throw new Error('invalid source'); },
  }));
  assert.match(element('dock-content').textContent, /invalid source/);
  element('tab-console').click();
  element('tab-problems').click();
  assert.match(element('dock-content').textContent, /invalid source/);
  assert.equal(element('error-badge').textContent, '!');
});

test('reports a missing WASM bundle instead of claiming success', async () => {
  const { element } = await setup(async () => { throw new Error('module missing'); });
  assert.match(element('dock-content').textContent, /module missing/);
  assert.equal(element('analyzer-status').textContent, 'Runtime failed to load');
});
