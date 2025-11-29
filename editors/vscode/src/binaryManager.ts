import * as path from "path";
import * as fs from "fs";
import * as https from "https";
import { promisify } from "util";
import { exec } from "child_process";
import { ExtensionContext, window, OutputChannel, ProgressLocation } from "vscode";

const execAsync = promisify(exec);

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

  if (fs.existsSync(binaryPath)) {
    outputChannel.appendLine(`Binary already exists at: ${binaryPath}`);
    return binaryPath;
  }

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

              if (isWindows) {
                await extractZip(archivePath, storageDir);
              } else {
                await extractTarXz(archivePath, storageDir);
              }

              const extractedBinaryPath = path.join(
                storageDir,
                platformInfo.binaryName
              );

              if (fs.existsSync(extractedBinaryPath)) {
                fs.renameSync(extractedBinaryPath, binaryPath);
              } else {
                throw new Error(`Binary not found after extraction`);
              }

              if (!isWindows) {
                fs.chmodSync(binaryPath, 0o755);
              }

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

export async function findServerBinary(
  context: ExtensionContext,
  outputChannel: OutputChannel,
  customPath?: string
): Promise<string> {
  if (customPath && customPath.trim() !== "") {
    outputChannel.appendLine(`Checking custom path: ${customPath}`);
    if (fs.existsSync(customPath)) {
      outputChannel.appendLine(`Found binary at custom path: ${customPath}`);
      return customPath;
    } else {
      outputChannel.appendLine(`Custom path does not exist: ${customPath}`);
    }
  }

  const envPath = process.env.GRAPHQL_LSP_PATH;
  if (envPath && envPath.trim() !== "") {
    outputChannel.appendLine(`Checking GRAPHQL_LSP_PATH: ${envPath}`);
    if (fs.existsSync(envPath)) {
      outputChannel.appendLine(`Found binary at GRAPHQL_LSP_PATH: ${envPath}`);
      return envPath;
    } else {
      outputChannel.appendLine(`GRAPHQL_LSP_PATH does not exist: ${envPath}`);
    }
  }

  const platformInfo = getPlatformInfo();

  const devPath = path.join(
    context.extensionPath,
    "../../target/debug/graphql-lsp"
  );
  if (fs.existsSync(devPath)) {
    outputChannel.appendLine(`Found binary at dev path: ${devPath}`);
    return devPath;
  }

  outputChannel.appendLine("Searching for graphql-lsp in PATH...");
  const pathBinary = await findInPath("graphql-lsp");
  if (pathBinary) {
    outputChannel.appendLine(`Found binary in PATH: ${pathBinary}`);
    return pathBinary;
  }

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
