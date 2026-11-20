import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as fs from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { once } from 'node:events';
import { realpathSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const source = fileURLToPath(new URL('.', import.meta.url));
const folder = (id) => id.replace(':', '-');
const good = { digest: 'bootstrap-db03844', commit: '0'.repeat(40), commit_count: 10 };
const candidate = {
  digest: 'sha256:' + 'a'.repeat(64),
  commit: 'a'.repeat(40),
  commit_count: '11',
};
const stale = { digest: 'sha256:' + 'b'.repeat(64) };
const time = '2026-11-20T12:00:00Z';
const record = (stage = 'idle', fields = {}) => ({
  good,
  candidate: stage === 'idle' ? null : candidate,
  stage,
  since: time,
  heartbeat: time,
  last_abort: null,
  ...fields,
});
const health = ({ digest }) => ({ running: digest, committed: digest, failed: null });
const snapshot = '{"generated_at":"2026-11-20T10:00:00Z"}';
const sha256 = createHash('sha256').update(snapshot).digest('hex');

// Serialized into the fixture and executed in the child processes.
async function fake() {
  const assert = (await import('node:assert/strict')).default;
  const { appendFileSync, readFileSync, readlinkSync, symlinkSync, unlinkSync } =
    await import('node:fs');
  const { basename, dirname, join } = await import('node:path');
  const home = process.env.HOME;
  const file = (name) => join(home, name);
  const config = JSON.parse(readFileSync(file('config')));
  const current = file('releases/current');
  const trace = (...entry) => appendFileSync(file('trace'), JSON.stringify(entry) + '\n');
  async function fakeFetch(input, init) {
    const request = new Request(input, init);
    const { method, url } = request;
    const { hostname, pathname: key } = new URL(url);
    if (url === 'https://hn-session.hashcode.dev/healthz') {
      if (config.prod === null) throw new Error('Network unreachable');
      return Response.json(config.prod);
    }
    assert.equal(hostname, 'fixture.r2.cloudflarestorage.com');
    assert.match(request.headers.get('Authorization'), /^AWS4-HMAC-SHA256 /);
    if (key === '/preview/releases/rollout.json') {
      const { existsSync, writeFileSync } = await import('node:fs');
      const path = file('rollout.json');
      trace(method, 'rollout', basename(readlinkSync(current)));
      if (method === 'PUT') {
        writeFileSync(path, await request.text());
        return new Response(null, { status: 200 });
      }
      if (!existsSync(path)) return new Response(null, { status: 404 });
      const value = JSON.parse(readFileSync(path));
      if (value.good.digest === config.main.digest && config.readback) {
        if (config.readback === 'crash') process.exit(77);
        value.heartbeat = value.since = '2026-11-20T12:00:00Z';
      }
      return Response.json(value);
    }
    assert.equal(method, 'GET');
    if (key === '/preview/dev/latest.json')
      return Response.json({
        version: 1,
        key: 'dev/bundles/' + config.sha + '.json',
        sha256: config.sha,
      });
    assert.equal(key, '/preview/dev/bundles/' + config.sha + '.json');
    return new Response(config.snapshot);
  }
  globalThis.fetch = (...args) =>
    fakeFetch(...args).catch((error) => {
      if (error.code === 'ERR_ASSERTION') console.error(error);
      throw error;
    });

  const name = basename(process.argv[1] || '');
  const args = process.argv.slice(2);
  if (name === 'crane' && args[0] === 'digest') {
    trace('digest', ...args);
    assert.deepEqual(args, ['digest', 'ghcr.io/shaance/hn-preview:main']);
    console.log(config.main.digest);
  } else if (name === 'crane') {
    trace('export', ...args);
    assert.deepEqual(args, ['export', 'ghcr.io/shaance/hn-preview@' + config.main.digest, '-']);
    process.stdout.write(readFileSync(file('image.tar')));
  } else if (name === 'cosign') {
    trace('verify', args.at(-1));
    const { digest, commit, commit_count } = config.main;
    assert.equal(args.at(-1), 'ghcr.io/shaance/hn-preview@' + digest);
    console.log(
      JSON.stringify([
        {
          critical: {
            image: { 'docker-manifest-digest': config.badSignature ? 'another-digest' : digest },
          },
          optional: { commit, commit_count },
        },
      ]),
    );
  } else if (name === 'hn') {
    const [action, path] = args;
    trace(action, process.argv[1], path);
    if (action === '--validate-snapshot') {
      assert.equal(readFileSync(path, 'utf8'), config.snapshot);
      if (config.rejectSnapshot) process.exitCode = 2;
    } else if (action === '--capture-snapshot' && config.switchOnCapture) {
      unlinkSync(current);
      symlinkSync(config.switchOnCapture, current);
    }
  } else if (name === 'publish.mjs') {
    trace('publish', process.argv[1], args[0], process.env.HN_BIN);
    assert.equal(process.env.HN_BIN, join(dirname(process.argv[1]), 'hn'));
  }
}

async function fixture(t, initial = record(), dev = health(good), prod = health(good)) {
  const home = realpathSync(await fs.mkdtemp(join(tmpdir(), 'rollout-check-')));
  t.after(() => fs.rm(home, { recursive: true, force: true }));
  const file = (name) => join(home, name);
  const layout = file('releases');
  const current = join(layout, 'current');
  const stub = '#!/usr/bin/env node\n';
  await fs.mkdir(join(layout, good.digest), { recursive: true });
  await fs.symlink(good.digest, current);
  await fs.writeFile(file('fake.mjs'), 'await (' + fake.toString() + ')();\n');
  await fs.writeFile(file('crane'), stub, { mode: 0o700 });
  await fs.writeFile(file('cosign'), stub, { mode: 0o700 });
  const bin = file('image/usr/local/bin');
  const release = file('image/opt/release');
  await fs.mkdir(bin, { recursive: true });
  await fs.mkdir(join(release, 'node_modules'), { recursive: true });
  await fs.writeFile(join(bin, 'hn'), stub, { mode: 0o700 });
  await fs.writeFile(join(release, 'publish.mjs'), '');
  await fs.copyFile(join(bin, 'hn'), join(layout, good.digest, 'hn'));
  await fs.chmod(join(layout, good.digest, 'hn'), 0o700);
  await fs.copyFile(join(release, 'publish.mjs'), join(layout, good.digest, 'publish.mjs'));
  await exec('tar', [
    '-cf',
    file('image.tar'),
    '-C',
    file('image'),
    'opt/release',
    'usr/local/bin/hn',
  ]);

  const server = createServer((request, response) => {
    assert.equal(request.url, '/healthz');
    assert.equal(request.headers.host, 'writer.test');
    response.end(JSON.stringify(dev));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => server.close());
  const env = {
    ...process.env,
    HOME: home,
    NODE_OPTIONS: '--import=' + file('fake.mjs'),
    PORT: String(server.address().port),
    PREVIEW_BIND: '127.0.0.1',
    PREVIEW_HOST: 'writer.test',
    R2_ACCOUNT_ID: 'fixture',
    R2_BUCKET: 'preview',
    R2_ACCESS_KEY_ID: 'fixture',
    R2_SECRET_ACCESS_KEY: 'fixtures-only',
    COSIGN: file('cosign'),
    CRANE: file('crane'),
  };
  const config = { main: candidate, prod, snapshot, sha: sha256 };
  const configure = async (changes) =>
    fs.writeFile(file('config'), JSON.stringify(Object.assign(config, changes)));
  await configure({});
  const save = (value) => fs.writeFile(file('rollout.json'), JSON.stringify(value));
  if (initial) await save(initial);
  const cli = async (binary, args, expected) => {
    let result;
    try {
      result = await exec(binary, args, { env, timeout: 15_000 });
    } catch (error) {
      result = error;
    }
    assert.equal(result.code ?? 0, expected, result.stdout + result.stderr);
    return result.stdout.trim();
  };
  const run = async (arg, expected = 0) => {
    const stdout = await cli(
      process.execPath,
      [join(source, 'rollout.mjs'), ...(arg ? [arg] : [])],
      expected,
    );
    return stdout ? JSON.parse(stdout) : null;
  };
  return {
    layout,
    configure,
    save,
    run,
    setDev: (value) => {
      dev = value;
    },
    current: () => fs.readlink(current).then(basename),
    read: async () => JSON.parse(await fs.readFile(file('rollout.json'))),
    trace: async () =>
      (await fs.readFile(file('trace'), 'utf8'))
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line)),
    publish: async () => {
      // Install the wrapper at fixture paths without changing its command sequence.
      const wrapper = (await fs.readFile(join(source, 'run.sh'), 'utf8'))
        .replace('/opt/hn-preview/node/bin/node', process.execPath)
        .replace('/opt/hn-preview/tools', source)
        .replace('/var/lib/hn-publisher', home);
      await fs.writeFile(file('run.sh'), wrapper);
      return cli('/bin/bash', [file('run.sh')], 0);
    },
  };
}

test('idle waits for both running and committed, then admits the signed main digest', async (t) => {
  const host = await fixture(t, record(), { ...health(good), running: candidate.digest });
  assert.equal((await host.run()).decision, 'waiting');
  let result = await host.read();
  assert.notEqual(result.heartbeat, time);
  assert.equal(result.since, time);
  assert.deepEqual(
    (await host.trace()).map(([verb]) => verb),
    ['GET', 'PUT', 'GET'],
  );
  host.setDev(health(good));
  await host.configure({ prod: { ...health(good), committed: null } });
  assert.equal((await host.run()).decision, 'waiting');
  await host.configure({ prod: health(good) });
  assert.equal((await host.run()).decision, 'admit');
  result = await host.read();
  assert.deepEqual(result.candidate, candidate);
  assert.equal(result.stage, 'dev');
  assert.equal(result.since, result.heartbeat);
});

for (const [label, fields, count] of [
  ['equal count', {}, '10'],
  ['older count', {}, '9'],
  ['last abort', { last_abort: { digest: candidate.digest, reason: 'rejected' } }, '12'],
]) {
  test('idle excludes ' + label, async (t) => {
    const host = await fixture(t, record('idle', fields));
    await host.configure({ main: { ...candidate, commit_count: count } });
    assert.equal((await host.run()).decision, 'waiting');
    const result = await host.read();
    assert.equal(result.candidate, null);
    assert.deepEqual(result.last_abort, fields.last_abort ?? null);
    assert.notEqual(result.heartbeat, time);
  });
}

test('advances one stage per tick and validates before PUT and readback before flip', async (t) => {
  const last_abort = { ...stale, reason: 'older rejection' };
  const host = await fixture(t, record('dev', { last_abort }), health(candidate));
  assert.equal((await host.run()).stage, 'prod');
  assert.equal((await host.run()).decision, 'waiting');
  await host.configure({ prod: health(candidate) });
  assert.equal((await host.run()).stage, 'writer');
  assert.equal(await host.current(), good.digest);
  assert.equal((await host.run()).decision, 'promote');
  const trace = (await host.trace()).slice(-6);
  assert.deepEqual(
    trace.map(([verb]) => verb),
    ['GET', 'verify', 'export', '--validate-snapshot', 'PUT', 'GET'],
  );
  assert.equal(trace[3][1], join(host.layout, folder(candidate.digest), 'hn'));
  assert.deepEqual(trace.slice(-2), [
    ['PUT', 'rollout', good.digest],
    ['GET', 'rollout', good.digest],
  ]);
  assert.equal(await host.current(), folder(candidate.digest));
  assert.equal(await fs.readlink(join(host.layout, 'previous')), join(host.layout, good.digest));
  const result = await host.read();
  assert.deepEqual(
    result,
    record('idle', {
      good: candidate,
      last_abort,
      since: result.heartbeat,
      heartbeat: result.heartbeat,
    }),
  );
});

for (const mode of ['crash', 'different']) {
  test('keeps current behind R2 after ' + mode + ' and reconciles on the next tick', async (t) => {
    const host = await fixture(t, record('writer'), health(candidate), health(candidate));
    await host.configure({ readback: mode });
    const result = await host.run(undefined, mode === 'crash' ? 77 : 1);
    if (mode === 'different') assert.equal(result.reason, 'Rollout readback differs');
    assert.deepEqual((await host.read()).good, candidate);
    assert.equal(await host.current(), good.digest);
    await host.configure({ readback: null });
    await fs.mkdir(join(host.layout, folder(stale.digest)));
    const next = await host.run();
    assert.equal(next.reconcile_writer, true);
    assert.equal(await host.current(), folder(candidate.digest));
    assert.deepEqual(
      (await fs.readdir(host.layout)).sort(),
      ['current', 'previous', good.digest, folder(candidate.digest)].sort(),
    );
  });
}

for (const [stage, fault, failed] of [
  ['dev', {}, 'dev'],
  ['prod', {}, 'prod'],
  ['writer', {}, 'dev'],
  ['writer', { badSignature: true }],
  ['writer', { rejectSnapshot: true }],
]) {
  test(`aborts ${stage} on ${failed || Object.keys(fault)[0]}`, async (t) => {
    const rejected = { ...health(good), failed: candidate.digest };
    const host = await fixture(
      t,
      record(stage),
      failed === 'dev' ? rejected : health(candidate),
      failed === 'prod' ? rejected : health(candidate),
    );
    await host.configure(fault);
    assert.equal((await host.run()).decision, 'abort');
    const result = await host.read();
    assert.equal(result.stage, 'idle');
    assert.equal(result.candidate, null);
    assert.deepEqual(result.good, good);
    assert.equal(result.last_abort.digest, candidate.digest);
    assert.match(result.last_abort.reason, /rejected|signature/);
    assert.equal(await host.current(), good.digest);
  });
}

for (const stage of ['idle', 'dev', 'prod', 'writer']) {
  test('unreachable production leaves ' + stage + ' pending with a heartbeat', async (t) => {
    const host = await fixture(t, record(stage), health(stage === 'idle' ? good : candidate), null);
    assert.equal((await host.run()).decision, 'waiting');
    const result = await host.read();
    assert.deepEqual(result, { ...record(stage), heartbeat: result.heartbeat });
    assert.notEqual(result.heartbeat, time);
  });
}

test('--check leaves record, staging and current alone', async (t) => {
  const host = await fixture(t, record('writer'), health(candidate), health(candidate));
  assert.equal((await host.run('--check')).decision, 'validate-writer');
  assert.deepEqual(await host.read(), record('writer'));
  assert.deepEqual((await fs.readdir(host.layout)).sort(), [good.digest, 'current'].sort());
  assert.deepEqual(
    (await host.trace()).map(([verb]) => verb),
    ['GET', 'verify'],
  );
  await host.save(record('idle', { good: candidate }));
  assert.equal((await host.run('--check')).reconcile_writer, true);
  assert.equal(await host.current(), good.digest);
});

test('a missing rollout record is an error', async (t) => {
  const host = await fixture(t, null);
  assert.match((await host.run(undefined, 1)).reason, /HTTP 404/);
  assert.deepEqual(
    (await host.trace()).map(([verb]) => verb),
    ['GET'],
  );
});

test('publish reconciles from R2 and uses one resolved release across all commands', async (t) => {
  const host = await fixture(t, record('idle', { good: candidate }));
  await host.configure({ switchOnCapture: good.digest });
  await host.publish();
  const trace = await host.trace();
  assert.deepEqual(
    trace.map(([verb]) => verb),
    ['GET', 'verify', 'export', '--capture-snapshot', '--prepare-snapshot', 'publish'],
  );
  const dir = join(host.layout, folder(candidate.digest));
  const pending = join(host.layout, '..', 'candidate.json');
  assert.deepEqual(trace.slice(-3), [
    ['--capture-snapshot', join(dir, 'hn'), pending],
    ['--prepare-snapshot', join(dir, 'hn'), pending],
    ['publish', join(dir, 'publish.mjs'), pending, join(dir, 'hn')],
  ]);
  assert.equal(await host.current(), good.digest);
});
