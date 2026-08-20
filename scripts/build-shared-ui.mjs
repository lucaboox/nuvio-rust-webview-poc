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
import { existsSync } from "node:fs";
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

const build = spawnSync("npx", ["vite", "build"], {
  cwd: sharedUi,
  stdio: "inherit",
  shell: true,
  env: { ...process.env, NUVIO_PLATFORM_MODULE: platformModule },
});
process.exit(build.status ?? 1);
