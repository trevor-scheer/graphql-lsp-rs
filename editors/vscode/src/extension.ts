console.log(">>> GraphQL LSP extension module loading <<<");

import * as path from "path";
import { workspace, ExtensionContext, window, OutputChannel } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable,
} from "vscode-languageclient/node";

console.log(">>> GraphQL LSP extension imports complete <<<");

let client: LanguageClient;
let outputChannel: OutputChannel;

export function activate(context: ExtensionContext) {
  outputChannel = window.createOutputChannel("GraphQL LSP Debug");
  outputChannel.show(true); // true = preserve focus
  outputChannel.appendLine("=== GraphQL LSP extension activating ===");

  // Path to the LSP server binary
  // In development, resolve relative to the extension directory
  const serverCommand =
    process.env.GRAPHQL_LSP_PATH ||
    path.join(context.extensionPath, "../../target/debug/graphql-lsp");
  outputChannel.appendLine(`LSP server command: ${serverCommand}`);
  console.log(`LSP server command: ${serverCommand}`);

  const run: Executable = {
    command: serverCommand,
    options: {
      env: {
        ...process.env,
        RUST_LOG: process.env.RUST_LOG || "debug",
      },
    },
  };

  const serverOptions: ServerOptions = {
    run,
    debug: run,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "graphql" },
      { scheme: "file", pattern: "**/*.{graphql,gql}" },
      { scheme: "file", language: "typescript" },
      { scheme: "file", language: "typescriptreact" },
      { scheme: "file", language: "javascript" },
      { scheme: "file", language: "javascriptreact" },
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher(
        "**/*.{graphql,gql,ts,tsx,js,jsx}"
      ),
    },
    outputChannel: outputChannel,
  };

  outputChannel.appendLine("Creating language client...");

  client = new LanguageClient(
    "graphql-lsp",
    "GraphQL Language Server",
    serverOptions,
    clientOptions
  );

  outputChannel.appendLine("Starting language client...");

  client.start().then(
    () => {
      outputChannel.appendLine("Language client started successfully!");
    },
    (error) => {
      outputChannel.appendLine(`Failed to start language client: ${error}`);
      window.showErrorMessage(`GraphQL LSP failed to start: ${error}`);
    }
  );

  outputChannel.appendLine("Extension activated!");
  console.log("=== Extension activation complete ===");
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
