// Run with: node --test editors/vscode/test/executable.test.js
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const bundled = (name) => path.join('/extension', 'bin', name);
const workspaceBinary = (name) => path.join('/workspace', 'target', 'debug', name);

const source = fs.readFileSync(path.join(__dirname, '..', 'extension.js'), 'utf8');

function load({ existing, configured = {}, platform = 'linux' }) {
  const extensionDir = '/extension';
  const vscode = {
    workspace: {
      getConfiguration: () => ({ get: (key) => configured[key] }),
      workspaceFolders: [{ uri: { fsPath: '/workspace' } }],
    },
  };
  const sandbox = {
    require(name) {
      if (name === 'vscode') return vscode;
      if (name === 'fs') return { existsSync: (p) => existing.includes(p) };
      return require(name);
    },
    __dirname: extensionDir,
    process: { platform, env: { PATH: '/usr/bin' } },
    module: { exports: {} },
    Buffer,
    console,
  };
  vm.runInNewContext(source + '\nmodule.exports.findExecutable = findExecutable;', sandbox);
  return sandbox.module.exports.findExecutable;
}

test('configured executable overrides bundled and workspace binaries', () => {
  const find = load({ existing: ['/custom/lsp', bundled('craftui-lsp')], configured: { lspPath: '/custom/lsp' } });
  assert.equal(find('craftui-lsp'), '/custom/lsp');
});

test('bundled executable is preferred over workspace and PATH', () => {
  const find = load({ existing: [bundled('craftui'), workspaceBinary('craftui')] });
  assert.equal(find('craftui'), bundled('craftui'));
});

test('windows bundled executable uses exe extension', () => {
  const find = load({ existing: [bundled('craftui-lsp.exe')], platform: 'win32' });
  assert.equal(find('craftui-lsp'), bundled('craftui-lsp.exe'));
});

test('development install falls back to workspace binary', () => {
  const find = load({ existing: [workspaceBinary('craftui-lsp')] });
  assert.equal(find('craftui-lsp'), workspaceBinary('craftui-lsp'));
});
