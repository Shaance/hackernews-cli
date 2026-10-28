// Prove the publication checks fail when the guarded behavior is removed.
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, copyFile, readFile, writeFile, symlink, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root = new URL('.', import.meta.url);
const dir = await mkdtemp(join(tmpdir(), 'hn-sensitivity-'));
try {
  await mkdir(join(dir, 'fixtures'));
  for (const file of [
    'adopt.mjs',
    'publish.mjs',
    'publisher-check.mjs',
    'fixtures/snapshot.json',
  ]) {
    await copyFile(new URL(file, root), join(dir, file));
  }
  await symlink(fileURLToPath(new URL('node_modules', root)), join(dir, 'node_modules'), 'dir');
  for (const [file, before, after] of [
    ['publish.mjs', '.length < 40', '.length < 1'],
    ['adopt.mjs', 'published version: ${failure}', 'published version'],
  ]) {
    const source = await readFile(new URL(file, root), 'utf8');
    assert(source.includes(before), `Mutation no longer matches ${file}`);
    await writeFile(join(dir, file), source.replace(before, after));
    const result = spawnSync(process.execPath, [join(dir, 'publisher-check.mjs')], {
      env: {
        ...process.env,
        HN_BIN: process.env.HN_BIN || fileURLToPath(new URL('../target/debug/hn', root)),
      },
      encoding: 'utf8',
      timeout: 30000,
    });
    assert.equal(result.error, undefined);
    assert.notEqual(result.status, 0, `Checks accepted removed behavior in ${file}`);
    assert.match(result.stderr, /AssertionError/);
    console.log(`PASS regression detected in ${file}`);
    await writeFile(join(dir, file), source);
  }
} finally {
  await rm(dir, { recursive: true, force: true });
}
