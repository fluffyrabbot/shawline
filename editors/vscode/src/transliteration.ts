import * as vscode from 'vscode';

// The server transliterates through textDocument/onTypeFormatting, so this
// language-scoped setting is the on/off switch.
const LANGUAGE = 'shawline';
const SETTING = 'formatOnType';

function editorConfig(): vscode.WorkspaceConfiguration {
  const uri = vscode.window.activeTextEditor?.document.uri;
  return vscode.workspace.getConfiguration('editor', { languageId: LANGUAGE, uri });
}

function isOn(): boolean {
  return editorConfig().get<boolean>(SETTING, false);
}

/** Flips `editor.formatOnType` for `[shawline]` at the narrowest scope that already sets it. */
export async function toggleTransliteration(): Promise<void> {
  const config = editorConfig();
  const info = config.inspect<boolean>(SETTING);
  let target = vscode.ConfigurationTarget.Global;
  if (info?.workspaceFolderLanguageValue !== undefined) {
    target = vscode.ConfigurationTarget.WorkspaceFolder;
  } else if (info?.workspaceLanguageValue !== undefined) {
    target = vscode.ConfigurationTarget.Workspace;
  }
  await config.update(SETTING, !isOn(), target, true);
}

/** Status bar item showing whether ASCII keys turn into Shavian; clicking it toggles. */
export class TransliterationStatus implements vscode.Disposable {
  private readonly item = vscode.window.createStatusBarItem('shawline.transliteration', vscode.StatusBarAlignment.Right, 100);
  private readonly subscriptions: vscode.Disposable[];

  constructor() {
    this.item.name = 'Shawline Transliteration';
    this.item.command = 'shawline.toggleTransliteration';
    this.subscriptions = [
      vscode.window.onDidChangeActiveTextEditor(() => this.update()),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration(`editor.${SETTING}`)) {
          this.update();
        }
      }),
    ];
    this.update();
  }

  private update(): void {
    if (vscode.window.activeTextEditor?.document.languageId !== LANGUAGE) {
      this.item.hide();
      return;
    }
    if (isOn()) {
      this.item.text = '$(keyboard) Shavian';
      this.item.tooltip = 'Transliteration on: ASCII keys become Shavian as you type. Click to turn it off.';
    } else {
      this.item.text = '$(keyboard) ASCII';
      this.item.tooltip = 'Transliteration off: keys are inserted as typed. Click to turn it on.';
    }
    this.item.show();
  }

  dispose(): void {
    this.subscriptions.forEach((s) => s.dispose());
    this.item.dispose();
  }
}
