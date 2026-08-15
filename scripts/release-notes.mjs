import { readFile } from "node:fs/promises";

const config = JSON.parse(await readFile(new URL("../shell/tauri.conf.json", import.meta.url), "utf8"));
const changelog = await readFile(new URL("../CHANGELOG.md", import.meta.url), "utf8");
const version = String(config.version || "").trim();

if (!version) throw new Error("shell/tauri.conf.json does not contain a version");

const lines = changelog.replace(/\r\n/g, "\n").split("\n");
const heading = `## [${version}]`;
const start = lines.findIndex((line) => line === heading || line.startsWith(`${heading} - `));
if (start < 0) {
  throw new Error(`CHANGELOG.md has no release section for ${version}`);
}

let end = lines.length;
for (let index = start + 1; index < lines.length; index += 1) {
  if (/^## \[.+]/.test(lines[index])) {
    end = index;
    break;
  }
}

const notes = lines.slice(start + 1, end).join("\n").trim();
if (!notes) throw new Error(`CHANGELOG.md section ${version} is empty`);

process.stdout.write(`${notes}\n`);
