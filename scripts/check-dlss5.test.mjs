import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { checkBundle, peMachine } from './check-dlss5.mjs';

async function fixture(t) {
  const directory = await mkdtemp(path.join(tmpdir(), 'nuvio-dlss5-test-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}
function pe(machine) {
  const data = Buffer.alloc(134);
  data.writeUInt16LE(0x5a4d, 0);
  data.writeUInt32LE(128, 60);
  data.writeUInt32LE(0x4550, 128);
  data.writeUInt16LE(machine, 132);
  return data;
}
test('reads x64 and x86 PE headers without loading a binary', async t => {
  const dir = await fixture(t);
  for (const machine of [0x8664, 0x14c]) {
    const file = path.join(dir, `${machine}.dll`);
    await writeFile(file, pe(machine));
    assert.equal(await peMachine(file), machine);
  }
});
test('rejects malformed headers', async t => {
  const dir = await fixture(t);
  const file = path.join(dir, 'invalid.dll');
  for (const data of [Buffer.alloc(2), Buffer.alloc(64), pe(0x8664).subarray(0, 130)]) {
    await writeFile(file, data);
    await assert.rejects(peMachine(file), /Invalid/);
  }
});
test('missing bundle components cannot be reported as working playback', async t => {
  const report = await checkBundle(await fixture(t));
  assert.equal(report.basicFilesValid, false);
  assert.equal(report.playbackVerified, false);
  assert.ok(report.checks.some(c => c.file === 'Neural consumer' && !c.ok));
});
test('flags 32-bit proxy and conflicting consumers', async t => {
  const dir = await fixture(t);
  await writeFile(path.join(dir, 'dxgi.dll'), pe(0x14c));
  for (const name of ['renodx-dlss5.addon64', 'deep-fried-chicken.addon64']) {
    await writeFile(path.join(dir, name), pe(0x8664));
  }
  const report = await checkBundle(dir);
  assert.match(report.checks.find(c => c.file === 'dxgi.dll').reason, /Requires Windows x64/);
  assert.match(report.checks.find(c => c.file === 'Neural consumer').reason, /Multiple/);
});
