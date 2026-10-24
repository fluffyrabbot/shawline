import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  DidChangeConfigurationNotification,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from 'vscode-languageclient/node';

const INSTALL_COMMAND = 'cargo install --path crates/shawline-lsp';

/** Owns the `shawline-lsp` client so it can be restarted when its path changes. */
export class ServerController {
  private client: LanguageClient | undefined;

  async start(): Promise<void> {
    const configured = vscode.workspace.getConfiguration('shawline').get<string>('serverPath') || 'shawline-lsp';
    const command = resolveExecutable(configured);
    if (!command) {
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
    try {
      await client.start();
      this.client = client;
    } catch (error) {
      void vscode.window.showErrorMessage(`Shawline: could not start ${command}: ${String(error)}`);
    }
  }

  async stop(): Promise<void> {
    const client = this.client;
    this.client = undefined;
    if (client) {
      await client.stop();
    }
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
