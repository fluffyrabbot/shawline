import * as vscode from 'vscode';
import { installFont, offerFontOnce } from './font';
import { ServerController } from './server';
import { TransliterationStatus, toggleTransliteration } from './transliteration';

let server: ServerController | undefined;

export function activate(context: vscode.ExtensionContext): void {
  server = new ServerController();
  const status = new TransliterationStatus();

  context.subscriptions.push(
    server,
    status,
    vscode.commands.registerCommand('shawline.toggleTransliteration', toggleTransliteration),
    vscode.commands.registerCommand('shawline.installFont', () => installFont(context)),
    vscode.commands.registerCommand('shawline.restartServer', () => server?.restart()),
    // Trust lifts the restriction on shawline.serverPath, so the folder's own setting may now apply.
    vscode.workspace.onDidGrantWorkspaceTrust(() => server?.restart()),
    vscode.workspace.onDidChangeConfiguration((event) => {
      // A restart reads the gloss setting afresh, so it needs no separate update.
      if (event.affectsConfiguration('shawline.serverPath')) {
        void server?.restart();
      } else if (event.affectsConfiguration('shawline.gloss')) {
        void server?.sendGloss();
      }
    }),
  );

  void server.start();
  void offerFontOnce(context);
}

export function deactivate(): Promise<void> | undefined {
  return server?.stop();
}
