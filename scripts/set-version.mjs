import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("Usage: npm run version:set -- 0.1.0-alpha.2");
  process.exit(1);
}

function updateJson(path) {
  const value = JSON.parse(readFileSync(path, "utf8"));
  value.version = version;
  if (value.packages?.[""]) value.packages[""].version = version;
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

updateJson(resolve(root, "package.json"));
updateJson(resolve(root, "package-lock.json"));
updateJson(resolve(root, "shell", "tauri.conf.json"));

const cargoPath = resolve(root, "shell", "Cargo.toml");
const cargo = readFileSync(cargoPath, "utf8").replace(
  /^(\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m,
  `$1"${version}"`,
);
writeFileSync(cargoPath, cargo);

console.log(`Nuvio version set to ${version}`);
