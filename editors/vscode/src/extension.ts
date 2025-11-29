console.log(">>> GraphQL LSP extension module loading <<<");

import { workspace, ExtensionContext, window, OutputChannel } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable,
} from "vscode-languageclient/node";
import { findServerBinary } from "./binaryManager";

console.log(">>> GraphQL LSP extension imports complete <<<");

let client: LanguageClient;
let outputChannel: OutputChannel;

export async function activate(context: ExtensionContext) {
  outputChannel = window.createOutputChannel("GraphQL LSP Debug");
  outputChannel.show(true);
  outputChannel.appendLine("=== GraphQL LSP extension activating ===");

  try {
    const config = workspace.getConfiguration("graphql-lsp");
    const customPath = config.get<string>("serverPath");

    const serverCommand = await findServerBinary(
      context,
      outputChannel,
      customPath
    );
    outputChannel.appendLine(`Using LSP server at: ${serverCommand}`);

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

    await client.start();
    outputChannel.appendLine("Language client started successfully!");
  } catch (error) {
    const errorMessage = `Failed to start GraphQL LSP: ${error}`;
    outputChannel.appendLine(errorMessage);
    window.showErrorMessage(errorMessage);
    throw error;
  }

  outputChannel.appendLine("Extension activated!");
  console.log("=== Extension activation complete ===");
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
