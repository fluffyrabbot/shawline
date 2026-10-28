import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  DidChangeConfigurationNotification,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  State,
} from 'vscode-languageclient/node';

const INSTALL_COMMAND = 'cargo install --path crates/shawline-lsp';

/**
 * Owns the `shawline-lsp` client so it can be restarted when its path changes,
 * and shows the server's status beside the language name in the status bar.
 */
export class ServerController implements vscode.Disposable {
  private client: LanguageClient | undefined;
  private readonly status = vscode.languages.createLanguageStatusItem('shawline.server', { language: 'shawline' });

  constructor() {
    this.status.name = 'Shawline Language Server';
    this.show('stopped');
  }

  dispose(): void {
    this.status.dispose();
  }

  async start(): Promise<void> {
    const configured = vscode.workspace.getConfiguration('shawline').get<string>('serverPath') || 'shawline-lsp';
    const command = resolveExecutable(configured);
    if (!command) {
      this.show('missing', configured);
      void reportMissing(configured);
      return;
    }

    const serverOptions: ServerOptions = { command, args: [] };
    const clientOptions: LanguageClientOptions = {
      documentSelector: [{ language: 'shawline' }],
      outputChannelName: 'Shawline',
      initializationOptions: { gloss: glossSetting() },
    };
    const client = new LanguageClient('shawline', 'Shawline', serverOptions, clientOptions);
    client.onDidChangeState(({ newState }) => {
      // A client replaced by a restart no longer speaks for the server.
      if (this.client === client) {
        this.show(newState === State.Running ? 'running' : newState === State.Starting ? 'starting' : 'stopped', command);
      }
    });
    this.client = client;
    try {
      await client.start();
    } catch (error) {
      this.client = undefined;
      this.show('failed', command);
      void vscode.window.showErrorMessage(`Shawline: could not start ${command}: ${String(error)}`);
    }
  }

  /** Shows `state` in the language status item; `detail` is the server's path, or the setting when missing. */
  private show(state: 'starting' | 'running' | 'stopped' | 'missing' | 'failed', detail = ''): void {
    const item = this.status;
    item.busy = state === 'starting';
    item.severity =
      state === 'missing' || state === 'failed'
        ? vscode.LanguageStatusSeverity.Error
        : state === 'stopped'
          ? vscode.LanguageStatusSeverity.Warning
          : vscode.LanguageStatusSeverity.Information;
    item.text = {
      starting: 'Shawline server starting',
      running: 'Shawline server running',
      stopped: 'Shawline server stopped',
      missing: 'Shawline server not found',
      failed: 'Shawline server failed to start',
    }[state];
    item.detail = state === 'missing' ? `"${detail}" is not on PATH or in ~/.cargo/bin` : detail;
    item.command =
      state === 'missing'
        ? { title: 'Set Path', command: 'workbench.action.openSettings', arguments: ['shawline.serverPath'] }
        : state === 'starting'
          ? undefined
          : { title: 'Restart', command: 'shawline.restartServer' };
  }

  async stop(): Promise<void> {
    const client = this.client;
    this.client = undefined;
    if (client) {
      await client.stop();
    }
    this.show('stopped');
  }

  async restart(): Promise<void> {
    await this.stop();
    await this.start();
  }

  /** Tells the running server that `shawline.gloss` changed; it republishes its diagnostics. */
  async sendGloss(): Promise<void> {
    await this.client?.sendNotification(DidChangeConfigurationNotification.type, {
      settings: { shawline: { gloss: glossSetting() } },
    });
  }
}

/** Whether hovers and messages show readings (PLAN P9, the gloss layer). */
function glossSetting(): boolean {
  return vscode.workspace.getConfiguration('shawline').get<boolean>('gloss', true);
}

/**
 * Finds the server before spawning it, because a spawn failure surfaces only as
 * an opaque ENOENT in the output channel.
 */
function resolveExecutable(configured: string): string | undefined {
  const expanded = configured.startsWith('~') ? path.join(os.homedir(), configured.slice(1)) : configured;
  if (expanded.includes('/') || expanded.includes('\\')) {
    // Relative paths (e.g. target/debug/shawline-lsp) are relative to the first workspace folder.
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
    const absolute = path.resolve(root, expanded);
    return isFile(absolute) ? absolute : undefined;
  }

  const extensions = process.platform === 'win32' ? ['', ...(process.env.PATHEXT ?? '.EXE').split(';')] : [''];
  // ~/.cargo/bin is where `cargo install` puts it, and GUI-launched editors may not have it on PATH.
  const dirs = [...(process.env.PATH ?? '').split(path.delimiter), path.join(os.homedir(), '.cargo', 'bin')];
  for (const dir of dirs.filter(Boolean)) {
    for (const ext of extensions) {
      const candidate = path.join(dir, expanded + ext);
      if (isFile(candidate)) {
        return candidate;
      }
    }
  }
  return undefined;
}

function isFile(file: string): boolean {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

async function reportMissing(configured: string): Promise<void> {
  const copy = 'Copy Install Command';
  const settings = 'Open Settings';
  const choice = await vscode.window.showErrorMessage(
    `Shawline: language server "${configured}" not found. From a Shawline checkout, run \`${INSTALL_COMMAND}\`, ` +
      'or point the shawline.serverPath setting at the binary.',
    copy,
    settings,
  );
  if (choice === copy) {
    await vscode.env.clipboard.writeText(INSTALL_COMMAND);
  } else if (choice === settings) {
    await vscode.commands.executeCommand('workbench.action.openSettings', 'shawline.serverPath');
  }
}
