import { createHash } from "node:crypto";
import { createReadStream, createWriteStream } from "node:fs";
import { mkdir, rename, rm, stat } from "node:fs/promises";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const EXPECTED_SHA256 = "07c68bb211f23a218ded0a36eb12207dc3aeb44e5318ffca6ce9dcc7c3173906";
const DOWNLOAD_URL =
  "https://github.com/lucaboox/nuvio-rust-webview-poc/releases/download/runtime-libmpv-v0.40.0-465-gf6c116491/libmpv-2.dll";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const runtimeDir = path.join(repoRoot, "shell", "runtime");
const targetPath = path.join(runtimeDir, "libmpv-2.dll");
const temporaryPath = `${targetPath}.tmp`;

async function sha256(filePath) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) {
    hash.update(chunk);
  }
  return hash.digest("hex");
}

async function isValidRuntime(filePath) {
  try {
    const file = await stat(filePath);
    return file.isFile() && (await sha256(filePath)) === EXPECTED_SHA256;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

if (process.platform !== "win32") {
  console.log("Skipping the Windows libmpv runtime on this platform.");
  process.exit(0);
}

await mkdir(runtimeDir, { recursive: true });

if (await isValidRuntime(targetPath)) {
  console.log(`Verified pinned libmpv runtime: ${targetPath}`);
  process.exit(0);
}

await rm(targetPath, { force: true });
await rm(temporaryPath, { force: true });

console.log(`Downloading pinned libmpv runtime from ${DOWNLOAD_URL}`);
const response = await fetch(DOWNLOAD_URL, { redirect: "follow" });
if (!response.ok || !response.body) {
  throw new Error(`Unable to download libmpv (${response.status} ${response.statusText})`);
}

try {
  await pipeline(Readable.fromWeb(response.body), createWriteStream(temporaryPath, { flags: "wx" }));
  const downloadedHash = await sha256(temporaryPath);
  if (downloadedHash !== EXPECTED_SHA256) {
    throw new Error(`libmpv checksum mismatch: expected ${EXPECTED_SHA256}, received ${downloadedHash}`);
  }
  await rename(temporaryPath, targetPath);
  console.log(`Downloaded and verified libmpv: ${targetPath}`);
} catch (error) {
  await rm(temporaryPath, { force: true });
  throw error;
}
