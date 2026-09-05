// Read-only preflight for the experimental ReShade/Feeder route. Never loads DLLs.
import { open, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export async function peMachine(file) {
  const handle = await open(file, 'r');
  try {
    const dos = Buffer.alloc(64);
    const first = await handle.read(dos, 0, dos.length, 0);
    if (first.bytesRead !== 64 || dos.readUInt16LE(0) !== 0x5a4d) throw new Error('Invalid DOS header');
    const pe = Buffer.alloc(6);
    const offset = dos.readUInt32LE(60);
    const second = await handle.read(pe, 0, pe.length, offset);
    if (offset < 64 || second.bytesRead !== 6 || pe.readUInt32LE(0) !== 0x4550) throw new Error('Invalid PE header');
    return pe.readUInt16LE(4);
  } finally {
    await handle.close();
  }
}

export async function checkBundle(directory) {
  const root = path.resolve(directory);
  const entries = await readdir(root, { withFileTypes: true });
  const files = new Map(entries.filter(e => e.isFile()).map(e => [e.name.toLowerCase(), e.name]));
  const checks = [];
  async function requireFile(relative, binary = false) {
    const location = path.join(root, relative);
    try {
      if (!(await stat(location)).isFile()) throw new Error('Not a file');
      if (binary) {
        const machine = await peMachine(location);
        if (machine !== 0x8664) throw new Error(`Requires Windows x64 (0x8664); found 0x${machine.toString(16)}`);
      }
      checks.push({ file: relative, ok: true });
    } catch (error) {
      checks.push({ file: relative, ok: false, reason: error.code === 'ENOENT' ? 'Missing' : error.message });
    }
  }
  for (const file of ['dxgi.dll', 'dlss5-feed.addon64', 'nvngx_dlss.dll', 'nvngx_dlssnr.dll']) {
    await requireFile(files.get(file) ?? file, true);
  }
  await requireFile('reshade-shaders/Shaders/DLSS5_Feed.fx');
  await requireFile('reshade-shaders/Shaders/ReShade.fxh');
  const consumers = [...files.keys()].filter(name =>
    name === 'deep-fried-chicken.addon64' || /^renodx-dlss5.*\.addon64$/.test(name));
  checks.push({ file: 'Neural consumer', ok: consumers.length === 1,
    reason: consumers.length === 0 ? 'Supply one compatible neural consumer: RenoDX DLSS5 or Deep Fried Chicken'
      : consumers.length > 1 ? 'Multiple neural consumers conflict; use exactly one' : undefined });
  for (const consumer of consumers) await requireFile(files.get(consumer), true);
  if (consumers.includes('deep-fried-chicken.addon64')) {
    await requireFile(files.get('deep-fried-chicken-nvngx.dll') ?? 'deep-fried-chicken-nvngx.dll', true);
    await requireFile(files.get('deep-fried-chicken.cfg') ?? 'deep-fried-chicken.cfg');
  }
  if ([...files.keys()].some(name => /^renodx-dlss(?:[.-]|$)/.test(name) && !name.startsWith('renodx-dlss5'))) {
    checks.push({ file: 'Feeder compatibility', ok: false, reason: 'renodx-dlss replaces the feeder; do not combine them' });
  }
  return { root, checks, basicFilesValid: checks.every(check => check.ok), playbackVerified: false,
    manualChecks: [
      'Use trusted x64 ReShade with full addon support; PE architecture alone does not establish provenance or addon support.',
      'Install a supported motion-vector provider and its dependencies; enable it before DLSS5_Feed with the matching provider definition.',
      'Confirm consumer/NGX/driver/GPU compatibility and actual successful neural frame evaluations in logs.',
      'Nuvio integration is not implemented yet. Passing this inventory check does not enable or prove DLSS playback.',
    ] };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (!process.argv[2]) {
    console.error('Usage: npm run dlss5:check -- "path/to/user-supplied-bundle"');
    process.exitCode = 2;
  } else {
    try {
      const report = await checkBundle(process.argv[2]);
      console.log(JSON.stringify(report, null, 2));
      process.exitCode = report.basicFilesValid ? 0 : 1;
    } catch (error) {
      console.error(`Cannot inspect bundle: ${error.message}`);
      process.exitCode = 2;
    }
  }
}
