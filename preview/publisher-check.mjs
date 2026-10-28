// Process-boundary fault injection. Exercises real scripts/native validation, not real R2.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
const dir = await mkdtemp(join(tmpdir(), 'hn-publisher-check-'));
const root = new URL('.', import.meta.url),
  hash = (b) => createHash('sha256').update(b).digest('hex');
const old = await readFile(new URL('fixtures/snapshot.json', root));
const changed = JSON.parse(old);
changed.generated_at = new Date(Date.parse(changed.generated_at) + 60000).toISOString();
const bytes = Buffer.from(JSON.stringify(changed)),
  digest = hash(bytes);
const manifest = (b) => ({
  version: 1,
  generated_at: changed.generated_at,
  key: `dev/bundles/${hash(b)}.json`,
  sha256: hash(b),
});
const store = join(dir, 'store'),
  target = join(dir, 'snapshot.json'),
  candidate = join(dir, 'candidate.json');
const preload = join(dir, 'transport.mjs'),
  calls = join(dir, 'restarts');
await mkdir(join(store, 'dev', 'bundles'), { recursive: true });
await writeFile(candidate, bytes);
await writeFile(
  preload,
  `
import {readFile,writeFile} from 'node:fs/promises';
const realFetch=globalThis.fetch;
globalThis.fetch=async(input,init)=>{
 const req=new Request(input,init),url=new URL(req.url),fault=process.env.FAULT;
 if(url.hostname==='127.0.0.1')return realFetch(input,init);
 if(!req.headers.get('Authorization')?.startsWith('AWS4-HMAC-SHA256 '))throw Error('Unsigned R2 request');
 const key=url.pathname.split('/').slice(2).join('/'),file=process.env.STORE+'/'+key;
 if(req.method==='PUT'){
  if((fault==='bundle-upload' && key.includes('/bundles/')) || (fault==='pointer-upload' && key.endsWith('/latest.json')))return new Response('',{status:503});
  await writeFile(file,Buffer.from(await req.arrayBuffer()));return new Response('');
 }
 if(fault==='bundle-readback' && key.includes('/bundles/'))return new Response('corrupt');
 return new Response(await readFile(file));
};
`,
);
await writeFile(
  join(dir, 'systemctl'),
  `#!/bin/sh
set -eu
test "$1 $2 $3" = '--user restart hn-preview.service'
count=0
if test -f "$CALLS"; then count=$(cat "$CALLS"); fi
count=$((count + 1))
printf '%s' "$count" > "$CALLS"
if test "$FAULT" = service-start && test "$count" = 1; then exit 1; fi
`,
  { mode: 0o755 },
);
const env = {
  ...process.env,
  R2_ACCOUNT_ID: 'test',
  R2_BUCKET: 'snapshots',
  R2_ACCESS_KEY_ID: 'test',
  R2_SECRET_ACCESS_KEY: 'test',
  PREVIEW_CHANNEL: 'dev',
  SNAPSHOT: target,
  STORE: store,
  CALLS: calls,
  PATH: dir + ':' + process.env.PATH,
  HN_BIN: process.env.HN_BIN || new URL('../target/debug/hn', root).pathname,
};
// Real HTTP boundary: production binds an IP but requires its public Host.
let rejectHealth = false;
const health = createServer(async (req, res) => {
  if (rejectHealth || req.headers.host !== 'hn-session.example.test') {
    res.writeHead(403).end();
    return;
  }
  res.end(JSON.stringify({ snapshot: hash(await readFile(target)) }));
});
await new Promise((resolve) => health.listen(0, '127.0.0.1', resolve));
env.PORT = String(health.address().port);
env.PREVIEW_HOST = 'hn-session.example.test';
const checks = [];
async function reset(bundle = bytes) {
  await writeFile(target, old);
  await writeFile(calls, '0');
  await writeFile(join(store, 'dev', 'latest.json'), JSON.stringify(manifest(bundle)));
  await writeFile(join(store, manifest(bundle).key), bundle);
}
function run(script, fault = '') {
  rejectHealth = fault === 'health-host';
  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [
        '--import',
        preload,
        new URL(script, root).pathname,
        ...(script === 'publish.mjs' ? [candidate] : []),
      ],
      { env: { ...env, FAULT: fault }, timeout: 20000 },
    );
    let stdout = '',
      stderr = '';
    child.stdout.on('data', (data) => (stdout += data));
    child.stderr.on('data', (data) => (stderr += data));
    child.on('error', reject);
    child.on('close', (status) => resolve({ status, stdout, stderr }));
  });
}
try {
  const unavailable = spawnSync(env.HN_BIN, ['--prepare-snapshot', candidate], {
    env: { PATH: process.env.PATH, HOME: dir, XDG_CONFIG_HOME: join(dir, 'config') },
    encoding: 'utf8',
    timeout: 10000,
  });
  assert.notEqual(unavailable.status, 0);
  assert.match(unavailable.stderr, /configured summary API key/);
  assert.deepEqual(await readFile(candidate), bytes);
  checks.push('Missing summary credentials fail without replacing the candidate');
  // This exact model output reached the live preview on 2026-10-26.
  const broken = structuredClone(changed);
  broken.articles['50225695'].summary = 'Author Salvatore Sanfilippo... ';
  const brokenBytes = Buffer.from(JSON.stringify(broken));
  await writeFile(candidate, brokenBytes);
  await reset(old);
  const priorManifest = await readFile(join(store, 'dev', 'latest.json'));
  const rejected = await run('publish.mjs');
  assert.notEqual(rejected.status, 0, rejected.stdout);
  assert.match(rejected.stderr, /Summary for 50225695 is too short/);
  assert.deepEqual(await readFile(join(store, 'dev', 'latest.json')), priorManifest);
  await assert.rejects(readFile(join(store, manifest(brokenBytes).key)), { code: 'ENOENT' });
  await writeFile(candidate, bytes);
  checks.push(
    'Exact failed AI fragment rejected before bundle upload; published pointer unchanged',
  );
  for (const fault of ['bundle-upload', 'bundle-readback', 'pointer-upload']) {
    await reset(old);
    const previous = await readFile(join(store, 'dev', 'latest.json'));
    const result = await run('publish.mjs', fault);
    assert.notEqual(result.status, 0, result.stdout);
    assert.deepEqual(await readFile(join(store, 'dev', 'latest.json')), previous);
    checks.push(fault + ': last published pointer preserved');
  }
  await reset(old);
  let result = await run('publish.mjs');
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(await readFile(join(store, 'dev', 'latest.json'))).sha256, digest);
  assert.deepEqual(await readFile(join(store, manifest(bytes).key)), bytes);
  checks.push('Publish validates, signs, uploads, reads back and advances pointer');
  for (const fault of ['bundle-readback', 'invalid-schema', 'service-start', 'health-host']) {
    await reset(fault === 'invalid-schema' ? Buffer.from('{"version":99}') : bytes);
    result = await run('adopt.mjs', fault);
    assert.notEqual(result.status, 0, result.stdout);
    assert.deepEqual(await readFile(target), old);
    if (fault === 'health-host') assert.match(result.stderr, /Health check HTTP 403/);
    assert.equal(
      await readFile(calls, 'utf8'),
      ['service-start', 'health-host'].includes(fault) ? '2' : '0',
    );
    await assert.rejects(readFile(target + '.pending'), { code: 'ENOENT' });
    checks.push(fault + ': local snapshot preserved/recovered');
  }
  await reset();
  result = await run('adopt.mjs');
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(await readFile(target), bytes);
  assert.deepEqual(await readFile(target + '.previous'), old);
  assert.equal(await readFile(calls, 'utf8'), '1');
  result = await run('adopt.mjs');
  assert.equal(result.status, 0, result.stderr);
  assert.equal(await readFile(calls, 'utf8'), '1');
  checks.push(
    'Adoption passes real HTTP Host validation, keeps rollback, restarts once; unchanged version does not restart',
  );
  console.log(
    JSON.stringify(
      {
        checks,
        limits:
          'Injected R2 and systemctl; native validator and HTTP health transport are real. This harness does not test actual R2 or deployed services.',
      },
      null,
      2,
    ),
  );
} finally {
  await new Promise((resolve) => health.close(resolve));
  await rm(dir, { recursive: true, force: true });
}
