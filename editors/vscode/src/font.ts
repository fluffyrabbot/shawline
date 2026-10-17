import * as vscode from 'vscode';

const PROMPTED_KEY = 'shawline.fontPrompted';

function instructions(): string {
  switch (process.platform) {
    case 'darwin':
      return 'Double-click ShawlineMono.ttf and choose Install Font, then reload the window.';
    case 'win32':
      return 'Right-click ShawlineMono.ttf and choose Install, then restart VS Code.';
    default:
      return 'Copy ShawlineMono.ttf into ~/.local/share/fonts, run `fc-cache -f`, then restart VS Code.';
  }
}

/** Reveals the bundled font in the OS file manager; extensions can't install fonts themselves. */
export async function installFont(context: vscode.ExtensionContext): Promise<void> {
  const font = vscode.Uri.joinPath(context.extensionUri, 'fonts', 'ShawlineMono.ttf');
  try {
    await vscode.workspace.fs.stat(font);
  } catch {
    void vscode.window.showErrorMessage(
      'Shawline: ShawlineMono.ttf is not bundled with this build. From a Shawline checkout, run ' +
        '`cargo run -p shawline-font --example build -- ShawlineMono.ttf` and install the result.',
    );
    return;
  }

  await vscode.commands.executeCommand('revealFileInOS', font);
  const reload = 'Reload Window';
  const choice = await vscode.window.showInformationMessage(
    `Shawline Mono is revealed in your file manager. ${instructions()} ` +
      'Drawings line up only in this font at line height 1.',
    reload,
  );
  if (choice === reload) {
    await vscode.commands.executeCommand('workbench.action.reloadWindow');
  }
}

/** One-time nudge, since misaligned boxes are the first thing a new user sees without the font. */
export async function offerFontOnce(context: vscode.ExtensionContext): Promise<void> {
  if (context.globalState.get<boolean>(PROMPTED_KEY)) {
    return;
  }
  await context.globalState.update(PROMPTED_KEY, true);
  const install = 'Install Font';
  const choice = await vscode.window.showInformationMessage(
    'Shawline drawings line up only in the Shawline Mono font.',
    install,
  );
  if (choice === install) {
    await installFont(context);
  }
}
