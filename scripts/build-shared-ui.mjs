/**
 * Builds the shared UI with this shell's capability layer in place of the
 * browser's.
 *
 * A script rather than a line in package.json because the environment variable
 * has to be set the same way on every platform, and because the path must be
 * absolute: the alias replaces an import specifier, and a relative path would
 * be resolved against whichever file happened to import it.
 *
 * The submodule is left untouched — nothing is written inside it but its own
 * dist, which is ignored there.
 */

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const sharedUi = join(root, "shared-ui");
const platformModule = resolve(root, "shell-ui", "platform.ts");

if (!existsSync(join(sharedUi, "package.json"))) {
  console.error(
    "shared-ui is empty. The UI is a submodule:\n" +
      "  git submodule update --init --recursive",
  );
  process.exit(1);
}

if (!existsSync(join(sharedUi, "node_modules"))) {
  console.log("Installing shared-ui dependencies…");
  const install = spawnSync("npm", ["install"], {
    cwd: sharedUi,
    stdio: "inherit",
    shell: true,
  });
  if (install.status !== 0) process.exit(install.status ?? 1);
}

/**
 * The account backend, which the UI compiles in rather than discovers.
 *
 * `.env.local` is not in the submodule and should not be — it holds this
 * installation's backend, not the shared UI's. Without it the bundle is built
 * with an empty backend and the app reports having none to sign into, which
 * looks like broken auth rather than missing configuration.
 *
 * Read from this repository's own `.env.local`, so the shell is configured
 * where the shell lives.
 */
const BACKEND_KEYS = [
  "NUVIO_SUPABASE_URL",
  "NUVIO_SUPABASE_FALLBACK_URL",
  "NUVIO_SUPABASE_ANON_KEY",
];

function backendEnv() {
  const file = join(root, ".env.local");
  const found = {};
  if (existsSync(file)) {
    for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
      const match = line.match(/^\s*([A-Z_][A-Z0-9_]*)\s*=\s*(.*)\s*$/);
      if (match) found[match[1]] = match[2].replace(/^["']|["']$/g, "");
    }
  }
  for (const key of BACKEND_KEYS)
    if (!found[key] && process.env[key]) found[key] = process.env[key];

  if (!found.NUVIO_SUPABASE_URL || !found.NUVIO_SUPABASE_ANON_KEY) {
    console.warn(
      `\nNo account backend configured. Create ${file} with:\n` +
        BACKEND_KEYS.map((key) => `  ${key}=…`).join("\n") +
        "\nBuilding anyway — the app will report having no backend to sign in to.\n",
    );
  }
  return found;
}

const build = spawnSync("npx", ["vite", "build"], {
  cwd: sharedUi,
  stdio: "inherit",
  shell: true,
  env: { ...process.env, ...backendEnv(), NUVIO_PLATFORM_MODULE: platformModule },
});
process.exit(build.status ?? 1);
