console.log(">>> GraphQL LSP extension module loading <<<");

import * as path from "path";
import * as fs from "fs";
import * as https from "https";
import { promisify } from "util";
import { exec } from "child_process";
import {
  workspace,
  ExtensionContext,
  window,
  OutputChannel,
  ProgressLocation,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable,
} from "vscode-languageclient/node";

console.log(">>> GraphQL LSP extension imports complete <<<");

const execAsync = promisify(exec);

let client: LanguageClient;
let outputChannel: OutputChannel;

interface PlatformInfo {
  platform: string;
  arch: string;
  binaryName: string;
}

function getPlatformInfo(): PlatformInfo {
  const platform = process.platform;
  const arch = process.arch;

  let platformStr: string;
  let archStr: string;

  switch (platform) {
    case "darwin":
      platformStr = "apple-darwin";
      break;
    case "linux":
      platformStr = "unknown-linux-gnu";
      break;
    case "win32":
      platformStr = "pc-windows-msvc";
      break;
    default:
      throw new Error(`Unsupported platform: ${platform}`);
  }

  switch (arch) {
    case "x64":
      archStr = "x86_64";
      break;
    case "arm64":
      archStr = "aarch64";
      break;
    default:
      throw new Error(`Unsupported architecture: ${arch}`);
  }

  const binaryName = platform === "win32" ? "graphql-lsp.exe" : "graphql-lsp";

  return {
    platform: `${archStr}-${platformStr}`,
    arch: archStr,
    binaryName,
  };
}

async function findInPath(binaryName: string): Promise<string | null> {
  try {
    const cmd =
      process.platform === "win32"
        ? `where ${binaryName}`
        : `which ${binaryName}`;
    const { stdout } = await execAsync(cmd);
    const result = stdout.trim().split("\n")[0];
    return result || null;
  } catch {
    return null;
  }
}

async function downloadBinary(
  url: string,
  destination: string
): Promise<void> {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(destination);
    https
      .get(url, (response) => {
        if (response.statusCode === 302 || response.statusCode === 301) {
          const redirectUrl = response.headers.location;
          if (redirectUrl) {
            https
              .get(redirectUrl, (redirectResponse) => {
                redirectResponse.pipe(file);
                file.on("finish", () => {
                  file.close();
                  resolve();
                });
              })
              .on("error", (err) => {
                fs.unlink(destination, () => {});
                reject(err);
              });
          } else {
            reject(new Error("Redirect location not found"));
          }
        } else {
          response.pipe(file);
          file.on("finish", () => {
            file.close();
            resolve();
          });
        }
      })
      .on("error", (err) => {
        fs.unlink(destination, () => {});
        reject(err);
      });
  });
}

async function extractTarXz(
  archivePath: string,
  extractDir: string
): Promise<void> {
  const cmd =
    process.platform === "win32"
      ? `tar -xf "${archivePath}" -C "${extractDir}"`
      : `tar -xJf "${archivePath}" -C "${extractDir}"`;
  await execAsync(cmd);
}

async function extractZip(
  archivePath: string,
  extractDir: string
): Promise<void> {
  const cmd =
    process.platform === "win32"
      ? `powershell -command "Expand-Archive -Path '${archivePath}' -DestinationPath '${extractDir}'"`
      : `unzip -q "${archivePath}" -d "${extractDir}"`;
  await execAsync(cmd);
}

async function downloadAndInstallBinary(
  context: ExtensionContext,
  platformInfo: PlatformInfo,
  outputChannel: OutputChannel
): Promise<string> {
  const storageDir = context.globalStorageUri.fsPath;
  if (!fs.existsSync(storageDir)) {
    fs.mkdirSync(storageDir, { recursive: true });
  }

  const binaryDir = path.join(storageDir, "bin");
  if (!fs.existsSync(binaryDir)) {
    fs.mkdirSync(binaryDir, { recursive: true });
  }

  const binaryPath = path.join(binaryDir, platformInfo.binaryName);

  // Check if already downloaded
  if (fs.existsSync(binaryPath)) {
    outputChannel.appendLine(`Binary already exists at: ${binaryPath}`);
    return binaryPath;
  }

  // Fetch latest release info from GitHub
  outputChannel.appendLine("Fetching latest release information...");

  const releaseUrl =
    "https://api.github.com/repos/trevor-scheer/graphql-lsp/releases/latest";

  return new Promise((resolve, reject) => {
    https
      .get(
        releaseUrl,
        {
          headers: {
            "User-Agent": "vscode-graphql-lsp",
          },
        },
        (response) => {
          let data = "";
          response.on("data", (chunk) => {
            data += chunk;
          });
          response.on("end", async () => {
            try {
              const release = JSON.parse(data);
              const version = release.tag_name;

              outputChannel.appendLine(`Latest version: ${version}`);

              // Construct download URL
              const isWindows = process.platform === "win32";
              const extension = isWindows ? "zip" : "tar.xz";
              const archiveName = `graphql-lsp-${platformInfo.platform}.${extension}`;
              const downloadUrl = `https://github.com/trevor-scheer/graphql-lsp/releases/download/${version}/${archiveName}`;

              outputChannel.appendLine(`Downloading from: ${downloadUrl}`);

              const archivePath = path.join(storageDir, archiveName);

              await window.withProgress(
                {
                  location: ProgressLocation.Notification,
                  title: "Downloading GraphQL LSP server...",
                  cancellable: false,
                },
                async () => {
                  await downloadBinary(downloadUrl, archivePath);
                }
              );

              outputChannel.appendLine("Download complete, extracting...");

              // Extract the archive
              if (isWindows) {
                await extractZip(archivePath, storageDir);
              } else {
                await extractTarXz(archivePath, storageDir);
              }

              // Find the binary in the extracted files
              const extractedBinaryPath = path.join(
                storageDir,
                platformInfo.binaryName
              );

              if (fs.existsSync(extractedBinaryPath)) {
                // Move binary to bin directory
                fs.renameSync(extractedBinaryPath, binaryPath);
              } else {
                throw new Error(`Binary not found after extraction`);
              }

              // Make executable on Unix systems
              if (!isWindows) {
                fs.chmodSync(binaryPath, 0o755);
              }

              // Clean up
              fs.unlinkSync(archivePath);

              outputChannel.appendLine(
                `Binary installed successfully at: ${binaryPath}`
              );
              resolve(binaryPath);
            } catch (error) {
              reject(error);
            }
          });
        }
      )
      .on("error", reject);
  });
}

async function findServerBinary(
  context: ExtensionContext,
  outputChannel: OutputChannel
): Promise<string> {
  // 1. Check custom path from settings
  const config = workspace.getConfiguration("graphql-lsp");
  const customPath = config.get<string>("serverPath");

  if (customPath && customPath.trim() !== "") {
    outputChannel.appendLine(`Checking custom path: ${customPath}`);
    if (fs.existsSync(customPath)) {
      outputChannel.appendLine(`Found binary at custom path: ${customPath}`);
      return customPath;
    } else {
      outputChannel.appendLine(`Custom path does not exist: ${customPath}`);
    }
  }

  const platformInfo = getPlatformInfo();

  // 2. Check in PATH
  outputChannel.appendLine("Searching for graphql-lsp in PATH...");
  const pathBinary = await findInPath("graphql-lsp");
  if (pathBinary) {
    outputChannel.appendLine(`Found binary in PATH: ${pathBinary}`);
    return pathBinary;
  }

  // 3. Check in extension storage (previously downloaded)
  const storageDir = context.globalStorageUri.fsPath;
  const storedBinaryPath = path.join(
    storageDir,
    "bin",
    platformInfo.binaryName
  );
  if (fs.existsSync(storedBinaryPath)) {
    outputChannel.appendLine(`Found binary in storage: ${storedBinaryPath}`);
    return storedBinaryPath;
  }

  // 4. Check dev path (for development)
  const devPath = path.join(
    context.extensionPath,
    "../../target/debug/graphql-lsp"
  );
  if (fs.existsSync(devPath)) {
    outputChannel.appendLine(`Found binary at dev path: ${devPath}`);
    return devPath;
  }

  // 5. Download from GitHub releases
  outputChannel.appendLine(
    "Binary not found, downloading from GitHub releases..."
  );

  try {
    const downloadedPath = await downloadAndInstallBinary(
      context,
      platformInfo,
      outputChannel
    );
    return downloadedPath;
  } catch (error) {
    throw new Error(
      `Failed to find or download graphql-lsp binary. You can install it manually with: cargo install graphql-lsp\n\nError: ${error}`
    );
  }
}

export async function activate(context: ExtensionContext) {
  outputChannel = window.createOutputChannel("GraphQL LSP Debug");
  outputChannel.show(true);
  outputChannel.appendLine("=== GraphQL LSP extension activating ===");

  try {
    const serverCommand = await findServerBinary(context, outputChannel);
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
