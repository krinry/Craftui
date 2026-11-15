// @ts-check
const vscode = require('vscode');
const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

/** @type {import('child_process').ChildProcess | null} */
let lspProcess = null;
/** @type {vscode.DiagnosticCollection | null} */
let diagnosticCollection = null;
/** @type {vscode.OutputChannel | null} */
let outputChannel = null;

let nextRpcId = 1;
/** @type {Map<number, { resolve: Function, reject: Function }>} */
const pendingRpcRequests = new Map();

function findExecutable(binName) {
  const config = vscode.workspace.getConfiguration('craftui');
  const customPath = binName === 'craftui-lsp' ? config.get('lspPath') : config.get('cliPath');
  if (customPath && customPath !== binName && fs.existsSync(customPath)) {
    return customPath;
  }

  const workspaceFolders = vscode.workspace.workspaceFolders || [];
  for (const folder of workspaceFolders) {
    const candidates = [
      path.join(folder.uri.fsPath, 'target', 'debug', binName),
      path.join(folder.uri.fsPath, 'target', 'release', binName),
      path.join(folder.uri.fsPath, '..', 'target', 'debug', binName),
      path.join(folder.uri.fsPath, '..', 'target', 'release', binName)
    ];
    for (const cand of candidates) {
      if (fs.existsSync(cand)) {
        return cand;
      }
      if (process.platform === 'win32' && fs.existsSync(cand + '.exe')) {
        return cand + '.exe';
      }
    }
  }

  const envPath = process.env.PATH || '';
  const pathDirs = envPath.split(path.delimiter);
  for (const dir of pathDirs) {
    const full = path.join(dir, binName);
    if (fs.existsSync(full)) {
      return full;
    }
    if (process.platform === 'win32' && fs.existsSync(full + '.exe')) {
      return full + '.exe';
    }
  }
  return null;
}

function sendRpcMessage(proc, msg) {
  if (!proc || !proc.stdin || proc.killed) {
    return;
  }
  const json = JSON.stringify(msg);
  const header = `Content-Length: ${Buffer.byteLength(json, 'utf8')}\r\n\r\n`;
  proc.stdin.write(header + json, 'utf8');
}

function sendRpcRequest(proc, method, params) {
  return new Promise((resolve, reject) => {
    if (!proc || !proc.stdin || proc.killed) {
      return reject(new Error('LSP process not running'));
    }
    const id = nextRpcId++;
    pendingRpcRequests.set(id, { resolve, reject });
    sendRpcMessage(proc, { jsonrpc: '2.0', id, method, params });
  });
}

function sendRpcNotification(proc, method, params) {
  sendRpcMessage(proc, { jsonrpc: '2.0', method, params });
}

function startLsp(context) {
  stopLsp();
  const config = vscode.workspace.getConfiguration('craftui');
  if (!config.get('enableLsp', true)) {
    return;
  }

  const binPath = findExecutable('craftui-lsp');
  if (!binPath) {
    outputChannel?.appendLine('[CraftUI] craftui-lsp not found. Syntax highlighting active; build craftui-lsp for full LSP.');
    return;
  }

  outputChannel?.appendLine(`[CraftUI] Starting language server: ${binPath}`);
  const proc = spawn(binPath, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  lspProcess = proc;

  let buffer = Buffer.alloc(0);
  proc.stdout.on('data', (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (true) {
      const headerEnd = buffer.indexOf('\r\n\r\n');
      if (headerEnd === -1) {
        break;
      }
      const headerStr = buffer.slice(0, headerEnd).toString('ascii');
      const match = /Content-Length:\s*(\d+)/i.exec(headerStr);
      if (!match) {
        buffer = buffer.slice(headerEnd + 4);
        continue;
      }
      const contentLength = parseInt(match[1], 10);
      const totalMsgLen = headerEnd + 4 + contentLength;
      if (buffer.length < totalMsgLen) {
        break;
      }
      const bodyBuf = buffer.slice(headerEnd + 4, totalMsgLen);
      buffer = buffer.slice(totalMsgLen);
      try {
        const payload = JSON.parse(bodyBuf.toString('utf8'));
        handleRpcMessage(payload);
      } catch (err) {
        outputChannel?.appendLine(`[CraftUI LSP] JSON parse error: ${err}`);
      }
    }
  });

  proc.stderr.on('data', (chunk) => {
    outputChannel?.appendLine(`[craftui-lsp] ${chunk.toString('utf8').trim()}`);
  });

  proc.on('exit', (code, signal) => {
    outputChannel?.appendLine(`[CraftUI] craftui-lsp exited (code: ${code}, signal: ${signal})`);
    lspProcess = null;
  });

  sendRpcRequest(proc, 'initialize', {
    processId: process.pid,
    capabilities: {},
    rootUri: vscode.workspace.workspaceFolders?.[0]?.uri.toString() || null
  }).then(() => {
    sendRpcNotification(proc, 'initialized', {});
    outputChannel?.appendLine('[CraftUI] craftui-lsp connected.');
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.languageId === 'craftui') {
        notifyDocumentOpen(doc);
      }
    }
  }).catch((err) => {
    outputChannel?.appendLine(`[CraftUI] LSP initialize failed: ${err.message}`);
  });
}

function stopLsp() {
  if (lspProcess) {
    try {
      lspProcess.kill();
    } catch (_) {}
    lspProcess = null;
  }
  for (const { reject } of pendingRpcRequests.values()) {
    reject(new Error('LSP stopped'));
  }
  pendingRpcRequests.clear();
}

function handleRpcMessage(msg) {
  if (msg.id !== undefined && pendingRpcRequests.has(msg.id)) {
    const { resolve, reject } = pendingRpcRequests.get(msg.id);
    pendingRpcRequests.delete(msg.id);
    if (msg.error) {
      reject(new Error(msg.error.message || 'RPC Error'));
    } else {
      resolve(msg.result);
    }
    return;
  }

  if (msg.method === 'textDocument/publishDiagnostics') {
    const { uri, diagnostics } = msg.params;
    if (!diagnosticCollection) return;
    const targetUri = vscode.Uri.parse(uri);
    const vscodeDiags = (diagnostics || []).map((d) => {
      const range = new vscode.Range(
        d.range.start.line,
        d.range.start.character,
        d.range.end.line,
        d.range.end.character
      );
      const severity = d.severity === 1
        ? vscode.DiagnosticSeverity.Error
        : vscode.DiagnosticSeverity.Warning;
      const diag = new vscode.Diagnostic(range, d.message, severity);
      diag.source = d.source || 'craftui-dsl';
      return diag;
    });
    diagnosticCollection.set(targetUri, vscodeDiags);
  }
}

function notifyDocumentOpen(document) {
  if (!lspProcess || document.languageId !== 'craftui') return;
  sendRpcNotification(lspProcess, 'textDocument/didOpen', {
    textDocument: {
      uri: document.uri.toString(),
      languageId: 'craftui',
      version: document.version,
      text: document.getText()
    }
  });
}

function notifyDocumentChange(event) {
  if (!lspProcess || event.document.languageId !== 'craftui') return;
  sendRpcNotification(lspProcess, 'textDocument/didChange', {
    textDocument: {
      uri: event.document.uri.toString(),
      version: event.document.version
    },
    contentChanges: [{ text: event.document.getText() }]
  });
}

function notifyDocumentClose(document) {
  if (!lspProcess || document.languageId !== 'craftui') return;
  sendRpcNotification(lspProcess, 'textDocument/didClose', {
    textDocument: { uri: document.uri.toString() }
  });
  diagnosticCollection?.delete(document.uri);
}

/**
 * @param {vscode.ExtensionContext} context
 */
function activate(context) {
  outputChannel = vscode.window.createOutputChannel('CraftUI');
  diagnosticCollection = vscode.languages.createDiagnosticCollection('craftui');
  context.subscriptions.push(outputChannel, diagnosticCollection);

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(notifyDocumentOpen),
    vscode.workspace.onDidChangeTextDocument(notifyDocumentChange),
    vscode.workspace.onDidCloseTextDocument(notifyDocumentClose)
  );

  // Hover Provider via LSP
  context.subscriptions.push(
    vscode.languages.registerHoverProvider('craftui', {
      async provideHover(document, position) {
        if (!lspProcess) return null;
        try {
          const res = await sendRpcRequest(lspProcess, 'textDocument/hover', {
            textDocument: { uri: document.uri.toString() },
            position: { line: position.line, character: position.character }
          });
          if (!res || !res.contents) return null;
          const val = typeof res.contents === 'string' ? res.contents : res.contents.value;
          return new vscode.Hover(new vscode.MarkdownString(val));
        } catch (_) {
          return null;
        }
      }
    })
  );

  // Completion Provider via LSP
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider('craftui', {
      async provideCompletionItems(document, position) {
        if (!lspProcess) return null;
        try {
          const res = await sendRpcRequest(lspProcess, 'textDocument/completion', {
            textDocument: { uri: document.uri.toString() },
            position: { line: position.line, character: position.character }
          });
          const list = Array.isArray(res) ? res : res?.items || [];
          return list.map((item) => {
            const ci = new vscode.CompletionItem(item.label);
            ci.detail = item.detail;
            ci.insertText = item.insertText || item.label;
            return ci;
          });
        } catch (_) {
          return null;
        }
      }
    }, '.', ':', ' ')
  );

  // Color Provider via LSP
  context.subscriptions.push(
    vscode.languages.registerColorProvider('craftui', {
      async provideDocumentColors(document) {
        if (!lspProcess) return [];
        try {
          const res = await sendRpcRequest(lspProcess, 'textDocument/documentColor', {
            textDocument: { uri: document.uri.toString() }
          });
          if (!Array.isArray(res)) return [];
          return res.map((ci) => {
            const range = new vscode.Range(
              ci.range.start.line,
              ci.range.start.character,
              ci.range.end.line,
              ci.range.end.character
            );
            const color = new vscode.Color(
              ci.color.red,
              ci.color.green,
              ci.color.blue,
              ci.color.alpha
            );
            return new vscode.ColorInformation(range, color);
          });
        } catch (_) {
          return [];
        }
      },
      async provideColorPresentations(color, context) {
        if (!lspProcess) return [];
        try {
          const res = await sendRpcRequest(lspProcess, 'textDocument/colorPresentation', {
            textDocument: { uri: context.document.uri.toString() },
            color: {
              red: color.red,
              green: color.green,
              blue: color.blue,
              alpha: color.alpha
            },
            range: {
              start: { line: context.range.start.line, character: context.range.start.character },
              end: { line: context.range.end.line, character: context.range.end.character }
            }
          });
          if (!Array.isArray(res)) return [];
          return res.map((cp) => new vscode.ColorPresentation(cp.label));
        } catch (_) {
          return [];
        }
      }
    })
  );

  // Commands
  context.subscriptions.push(
    vscode.commands.registerCommand('craftui.preview', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || !editor.document.fileName.endsWith('.cui')) {
        vscode.window.showWarningMessage('Please focus a .cui file to open live preview.');
        return;
      }
      const cli = findExecutable('craftui') || 'craftui';
      const term = vscode.window.createTerminal('CraftUI Preview');
      term.show();
      term.sendText(`${cli} preview "${editor.document.fileName}"`);
    }),
    vscode.commands.registerCommand('craftui.check', () => {
      const editor = vscode.window.activeTextEditor;
      const target = editor ? editor.document.fileName : '.';
      const cli = findExecutable('craftui') || 'craftui';
      const term = vscode.window.createTerminal('CraftUI Check');
      term.show();
      term.sendText(`${cli} check "${target}"`);
    }),
    vscode.commands.registerCommand('craftui.permissionsCheck', () => {
      const cli = findExecutable('craftui') || 'craftui';
      const term = vscode.window.createTerminal('CraftUI Permissions');
      term.show();
      term.sendText(`${cli} permissions check craftui.toml`);
    }),
    vscode.commands.registerCommand('craftui.startMcp', () => {
      const cli = findExecutable('craftui') || 'craftui';
      const term = vscode.window.createTerminal('CraftUI MCP');
      term.show();
      term.sendText(`${cli} mcp --stdio`);
    }),
    vscode.commands.registerCommand('craftui.restartLsp', () => {
      startLsp(context);
      vscode.window.showInformationMessage('CraftUI Language Server restarted.');
    })
  );

  startLsp(context);
}

function deactivate() {
  stopLsp();
}

module.exports = {
  activate,
  deactivate
};
