import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { createServer } from 'node:net';
import {
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readlink,
  rm,
  stat,
  symlink,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { setTimeout as wait } from 'node:timers/promises';
import { test } from 'node:test';

const digest = (c) => 'sha256:' + c.repeat(64);
const image = (id) =>
  digest(id.startsWith('bootstrap') ? '0' : (parseInt(id[7], 16) ^ 8).toString(16));
const folder = (id) => id.replace(':', '-');
const bootstrap = { digest: 'bootstrap-db03844', commit: '0'.repeat(40), commit_count: 10 };
const release = (c, count) => ({ digest: digest(c), commit: c.repeat(40), commit_count: count });
const candidate = release('a', 11);
const next = release('b', 12);
const heartbeat = '2026-11-20T12:00:00Z';
const rollout = (good = bootstrap, proposed = candidate, stage = 'dev') => ({
  good,
  candidate: proposed,
  stage,
  since: heartbeat,
  heartbeat,
  last_abort: null,
});
const idle = rollout(bootstrap, null, 'idle');
const exec = promisify(execFile);
const source = fileURLToPath(new URL('.', import.meta.url));
const dependencies = fileURLToPath(new URL('../node_modules', import.meta.url));

// This function is serialized into the temporary host and run only by the stub processes.
async function fakeProcess() {
  const assert = (await import('node:assert/strict')).default;
  const { appendFileSync, existsSync, readFileSync, rmSync, writeFileSync } =
    await import('node:fs');
  const { basename, join } = await import('node:path');
  const { spawn } = await import('node:child_process');
  const { createHash } = await import('node:crypto');
  const { createServer } = await import('node:http');
  const { setTimeout: wait } = await import('node:timers/promises');
  const { WebSocketServer } = await import('ws');
  const { HOME, SNAPSHOT, PORT, PREVIEW_HOST, PREVIEW_ORIGIN, DOCKER_RUNTIME } = process.env;
  const file = (name) => join(HOME, name);
  const read = (name) => JSON.parse(readFileSync(file(name), 'utf8'));
  const save = (name, value) => writeFileSync(file(name), JSON.stringify(value));
  const config = read('fake-config');
  const fault = config.fault;
  const state = '.local/state/hn-preview';
  const trace = (...entry) => appendFileSync(file('calls'), JSON.stringify(entry) + '\n');
  const name = basename(process.argv[1]);
  const args = process.argv.slice(2);
  const [first, second] = args;
  if (name !== 'gateway') trace(name, ...args);
  const matches = () => {
    const raw = readFileSync(file('.local/lib/hn-preview/current/release.env'), 'utf8');
    return Object.fromEntries(
      raw
        .trim()
        .split('\n')
        .map((line) => line.split('=')),
    );
  };

  if (name === 'cosign') {
    const id = args.at(-1).split('@')[1];
    assert.deepEqual(args, [
      'verify',
      '--output',
      'json',
      '--certificate-identity',
      'https://github.com/Shaance/hn/.github/workflows/preview-server.yml@refs/heads/main',
      '--certificate-oidc-issuer',
      'https://token.actions.githubusercontent.com',
      'ghcr.io/shaance/hn-preview@' + id,
    ]);
    const row = config.releases[id];
    if (fault === 'sigstore' || fault === 'identity') {
      // Cosign can wrap both types of failures under the same diagnostic.
      console.error(
        '2026/11/20 12:00:00 error during command execution: no matching signatures: ' +
          (fault === 'sigstore'
            ? 'verifying transparency log: request timed out'
            : 'none of the expected identities matched what was in the certificate'),
      );
      process.exitCode = 1;
    } else {
      console.log(
        JSON.stringify([
          {
            // Copied from real cosign v3.1.4 output on the development host.
            critical: {
              identity: { 'docker-reference': 'ghcr.io/shaance/hn-preview@' + id },
              image: { 'docker-manifest-digest': id },
              type: 'https://sigstore.dev/cosign/sign/v1',
            },
            optional: {
              commit: row.commit,
              commit_count: fault === 'annotations' ? '999' : String(row.commit_count),
            },
          },
        ]),
      );
    }
  } else if (name === 'docker') {
    const images = read('images');
    const print = (text) => console.log(text);
    if (first === 'pull') {
      if (fault === 'registry') {
        console.error('GET: registry unavailable');
        process.exitCode = 1;
        return;
      }
      images.push(config.releases[second.split('@')[1]].image);
      save('images', [...new Set(images)]);
    } else if (first === 'image' && second === 'inspect') {
      const ref = args.at(-1);
      const id = ref.includes('@') ? config.releases[ref.split('@')[1]].image : ref;
      if (fault === 'inspect-unavailable') {
        console.error('Cannot connect to the Docker daemon');
        process.exitCode = 1;
        return;
      }
      if (!images.includes(id)) {
        console.error('Error response from daemon: No such image: ' + id);
        process.exitCode = 1;
        return;
      }
      print(id);
    } else if (first === 'image' && second === 'ls') {
      print(images.join('\n'));
    } else if (first === 'image' && second === 'rm') {
      assert(images.includes(args.at(-1)));
      save(
        'images',
        images.filter((id) => id !== args.at(-1)),
      );
    } else if (first === 'create') {
      assert(images.includes(second));
      save('export-container', second);
      print('export-container');
    } else if (first === 'cp') {
      assert(existsSync(file('export-container')));
      if (second.endsWith('/hn')) {
        const command =
          fault === 'snapshot-signal'
            ? 'kill -KILL "$$"'
            : fault === 'snapshot-timeout'
              ? 'exec sleep 70'
              : 'test "' + fault + '" != snapshot';
        writeFileSync(
          args[2],
          `#!/bin/sh
test "$1" = --validate-snapshot && test "$2" = "$SNAPSHOT" && test -s "$2" || exit 2
${command}
`,
          { mode: fault === 'snapshot-launch' ? 0o644 : 0o755 },
        );
      } else {
        assert.equal(second, 'export-container:/opt/release/.');
        writeFileSync(join(args[2], 'server.mjs'), '// extracted gateway\n');
      }
    } else if (first === 'rm') {
      assert.equal(second, 'export-container');
      rmSync(file('export-container'));
      if (fault === 'change-record') save('rollout', config.changed);
    } else if (first === 'ps' && second === '-q') {
      assert.deepEqual(args.slice(2), ['--filter', 'label=hn-preview=local']);
      if (config.busy) print('visitor');
    } else if (first === 'ps' && second === '-aq') {
      const id = args.at(-1).split('=')[1];
      if (existsSync(file(id))) print(id);
    } else if (first === 'container' && second === 'inspect') {
      const container = read(args.at(-1));
      print(JSON.stringify(container));
    } else assert.fail(args.join(' '));
  } else if (name === 'systemctl') {
    const marker = file('gateway-pid');
    if (existsSync(marker)) {
      process.kill(Number(readFileSync(marker, 'utf8')), 'SIGTERM');
      for (let i = 0; i < 500 && existsSync(marker); i++) await wait(10);
      assert(!existsSync(marker), 'old gateway did not stop');
    }
    if (first === 'stop') return;
    assert.deepEqual(args, ['--user', 'restart', 'hn-preview.service']);
    const child = spawn(file('gateway'), [], {
      detached: true,
      env: { ...process.env, ...matches() },
      stdio: 'ignore',
    });
    writeFileSync(marker, String(child.pid));
    child.unref();
  } else if (name === 'gateway') {
    const { RELEASE, PREVIEW_IMAGE } = process.env;
    const targeted = RELEASE === config.target;
    const server = createServer((req, res) => {
      if (req.headers.host !== PREVIEW_HOST || req.url !== '/healthz')
        return res.writeHead(403).end();
      const snapshot = createHash('sha256').update(readFileSync(SNAPSHOT)).digest('hex');
      res.end(
        JSON.stringify({
          ...read(state + '/committed.json'),
          snapshot:
            (targeted && fault === 'health') ||
            fault === 'both-health' ||
            (!targeted && fault === 'full-forever-bad-good')
              ? 'bad-snapshot'
              : snapshot,
          running: targeted && fault === 'running' ? 'wrong-release' : RELEASE,
        }),
      );
    });
    const wss = new WebSocketServer({ noServer: true });
    let sessions = 0;
    const started = Date.now();
    server.on('upgrade', (req, socket, head) => {
      if (
        req.headers.host !== PREVIEW_HOST ||
        req.headers.origin !== PREVIEW_ORIGIN ||
        req.url !== '/session'
      )
        return socket.end('HTTP/1.1 403 Forbidden\r\n\r\n');
      if (
        targeted &&
        (fault === 'full-forever' ||
          fault === 'full-forever-bad-good' ||
          (fault === 'full' && sessions++ === 0))
      ) {
        trace('session-full');
        if (fault === 'full-forever' && Date.now() - started > 298_000) {
          trace('session-pending');
          return setTimeout(
            () => socket.end('HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n'),
            4000,
          );
        }
        return socket.end('HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n');
      }
      wss.handleUpgrade(req, socket, head, (ws) => {
        const id = 'f'.repeat(64);
        save(id, {
          Image: targeted && fault === 'image' ? 'wrong-image' : PREVIEW_IMAGE,
          HostConfig: {
            Runtime: targeted ? DOCKER_RUNTIME || 'runc' : 'runsc-rootless',
            NetworkMode: targeted && fault === 'container' ? 'bridge' : 'none',
            ReadonlyRootfs: true,
            Memory: 268435456,
          },
        });
        ws.on('close', () => rmSync(file(id), { force: true }));
        ws.send(Buffer.from('To'));
        ws.send(JSON.stringify({ type: 'ready', id }));
        const snapshot = JSON.parse(readFileSync(SNAPSHOT));
        const title = snapshot.items[snapshot.feeds.best[0]].title;
        if (targeted && fault === 'session') ws.send(JSON.stringify({ type: 'ended' }));
        else if (targeted && fault === 'error-screen') {
          ws.send(Buffer.from("p | New Couldn't load Best stories"));
          ws.send(JSON.stringify({ type: 'ended' }));
        } else {
          ws.send(Buffer.from('p | New ' + title));
        }
      });
    });
    process.on('SIGTERM', () => {
      for (const ws of wss.clients) ws.terminate();
      server.close(() => {
        rmSync(file('gateway-pid'));
        process.exit(0);
      });
    });
    server.listen(Number(PORT), '127.0.0.1');
  } else assert.fail(name);
}

async function fixture(context, options = {}) {
  const home = await mkdtemp(join(tmpdir(), 'hn-update-check-'));
  const tooling = join(home, 'tools');
  const lib = join(home, '.local/lib/hn-preview');
  const state = join(home, '.local/state/hn-preview');
  const snapshot = join(home, 'snapshot.json');
  const json = (path, value) => writeFile(path, JSON.stringify(value));
  const read = async (path) => JSON.parse(await readFile(path, 'utf8'));
  const server = createServer();
  await new Promise((resolve, reject) =>
    server.once('error', reject).listen(0, '127.0.0.1', resolve),
  );
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  const env = {
    ...process.env,
    HOME: home,
    PORT: String(port),
    SNAPSHOT: snapshot,
    PREVIEW_BIND: '127.0.0.1',
    PREVIEW_HOST: 'hn.example.test',
    PREVIEW_ORIGIN: 'https://preview.example.test',
    DOCKER_RUNTIME: options.runtime ?? 'runsc-rootless',
    PREVIEW_ROLE: options.role ?? 'dev',
    COSIGN: join(home, 'cosign'),
    DOCKER: join(home, 'docker'),
    SYSTEMCTL: join(home, 'systemctl'),
    R2_ACCOUNT_ID: 'test',
    R2_BUCKET: 'snapshots',
    R2_ACCESS_KEY_ID: 'test',
    R2_SECRET_ACCESS_KEY: 'secret-sentinel',
  };
  if (options.runtime === null) delete env.DOCKER_RUNTIME;
  const systemctl = (...args) => exec(env.SYSTEMCTL, args, { env, timeout: 9000 });
  context.after(async () => {
    try {
      await systemctl('stop');
    } finally {
      await rm(home, { recursive: true, force: true });
    }
  });
  await Promise.all([tooling, lib, state].map((dir) => mkdir(dir, { recursive: true })));
  await Promise.all(
    ['update.mjs', 'release.mjs', 'session.mjs'].map((name) =>
      cp(join(source, name), join(tooling, name)),
    ),
  );
  await symlink(dependencies, join(home, 'node_modules'));
  const stub = `#!${process.execPath}\n'use strict';\n(${fakeProcess.toString()})().catch((error) => { console.error(error); process.exitCode = 1; });\n`;
  for (const name of ['docker', 'cosign', 'systemctl', 'gateway'])
    await writeFile(join(home, name), stub, { mode: 0o755 });
  await writeFile(join(tooling, 'VERSION'), 'fixture-tools\n');
  const preload = join(home, 'r2.mjs');
  await writeFile(
    preload,
    `
    import assert from 'node:assert/strict';
    import {existsSync} from 'node:fs';
    import {readFile} from 'node:fs/promises';
    globalThis.fetch = async (input, init) => {
      const req = new Request(input, init);
      assert.equal(req.method, 'GET');
      assert.equal(req.url, 'https://test.r2.cloudflarestorage.com/snapshots/releases/rollout.json');
      assert.match(req.headers.get('Authorization'), /^AWS4-HMAC-SHA256 /);
      if (existsSync(process.env.HOME + '/r2-unavailable')) return new Response('', {status: 503});
      return new Response(await readFile(process.env.HOME + '/rollout'));
    };
  `,
  );
  const proposed = options.record ?? rollout();
  const releases = [bootstrap, candidate, next, ...(options.extra ?? [])];
  const staged = options.staged ?? [bootstrap];
  const id = options.current ?? bootstrap.digest;
  const config = {
    fault: options.fault,
    busy: options.busy,
    changed: idle,
    target: proposed.candidate?.digest ?? proposed.good.digest,
    releases: Object.fromEntries(
      releases.map((row) => [row.digest, { ...row, image: image(row.digest) }]),
    ),
  };
  const setRecord = (record) => json(join(home, 'rollout'), record);
  const save = (name, value) => json(join(state, name + '.json'), value);
  await Promise.all([
    cp(new URL('../fixtures/snapshot.json', import.meta.url), snapshot),
    json(join(home, 'fake-config'), config),
    json(
      join(home, 'images'),
      (options.available ?? staged).map((row) => image(row.digest)),
    ),
    setRecord(proposed),
    save('committed', { committed: options.committed ?? bootstrap.digest }),
  ]);
  for (const row of staged) {
    const dir = join(lib, folder(row.digest));
    await mkdir(dir);
    await writeFile(
      join(dir, 'release.env'),
      `PREVIEW_IMAGE=${image(row.digest)}\nRELEASE=${row.digest}\n`,
    );
    await writeFile(
      join(dir, 'hn'),
      '#!/bin/sh\ntest "$1" = --validate-snapshot && test -s "$2"\n',
      { mode: 0o755 },
    );
  }
  if (options.attempt) await save('attempt', options.attempt);
  await symlink(folder(id), join(lib, 'current'));
  await systemctl('--user', 'restart', 'hn-preview.service');
  for (let i = 0; i < 100; i++) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/healthz`);
      await response.body.cancel();
      break;
    } catch (error) {
      if (i === 99) throw error;
      await wait(20);
    }
  }
  await writeFile(join(home, 'calls'), '');
  return {
    setRecord,
    save,
    stopGateway: () => systemctl('stop'),
    setFault: async (fault) => {
      config.fault = fault;
      await json(join(home, 'fake-config'), config);
    },
    setOffline: async (offline) => {
      if (offline) await writeFile(join(home, 'r2-unavailable'), '');
      else await rm(join(home, 'r2-unavailable'), { force: true });
    },
    status: () => read(join(state, 'committed.json')),
    images: () => read(join(home, 'images')),
    entries: () => readdir(lib),
    current: () => readlink(join(lib, 'current')),
    calls: async () =>
      (await readFile(join(home, 'calls'), 'utf8'))
        .trim()
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line)),
    exists: async (name) => (await readdir(state)).includes(name + '.json'),
    async fingerprint() {
      const values = [];
      for (const dir of [state, lib, ...staged.map((row) => join(lib, folder(row.digest)))]) {
        for (const name of (await readdir(dir)).sort()) {
          const path = join(dir, name);
          values.push([
            path,
            (await stat(path)).mtimeMs,
            name === 'current' ? await readlink(path) : null,
          ]);
        }
      }
      return values;
    },
    async run(expected = 0, ...args) {
      const child = spawn(
        process.execPath,
        ['--import', preload, join(tooling, 'update.mjs'), ...args],
        {
          env,
          timeout: options.runTimeout ?? 25_000,
          stdio: ['ignore', 'pipe', 'pipe'],
        },
      );
      const collect = async (stream) => {
        let output = '';
        for await (const chunk of stream) output += chunk;
        return output;
      };
      const [code, stdout, stderr] = await Promise.all([
        new Promise((resolve, reject) => child.once('error', reject).once('close', resolve)),
        collect(child.stdout),
        collect(child.stderr),
      ]);
      assert.equal(stderr, '');
      assert.doesNotMatch(stdout, /secret-sentinel/);
      assert.equal(code, expected, stdout);
      return JSON.parse(stdout);
    },
  };
}

const restartCount = (calls) => calls.filter(([name]) => name === 'systemctl').length;
const journal = { target: candidate.digest, known_good: bootstrap.digest };
const scenarios = [];
const scenario = (name, run) => scenarios.push({ name, run });

scenario('first apply from bootstrap and no-op heartbeat refresh', async (t) => {
  const host = await fixture(t);
  assert.equal((await host.run()).decision, 'committed');
  assert.equal(await host.current(), folder(candidate.digest));
  assert.equal(await host.exists('attempt'), false);
  assert.deepEqual(await host.status(), {
    committed: candidate.digest,
    failed: null,
    reason: null,
    tools: 'fixture-tools',
    rollout: rollout(),
  });
  const calls = await host.calls();
  assert.equal(restartCount(calls), 1);
  assert(calls.some(([name, call]) => name === 'cosign' && call === 'verify'));
  assert(calls.some(([name, call]) => name === 'docker' && call === 'container'));
  const refreshed = { ...rollout(), heartbeat: '2026-11-20T12:15:00Z' };
  await host.setRecord(refreshed);
  assert.equal((await host.run()).decision, 'no-op');
  assert.deepEqual(await host.calls(), calls);
  assert.deepEqual((await host.status()).rollout, refreshed);
});

for (const fault of ['sigstore', 'registry', 'identity', 'annotations', 'snapshot']) {
  scenario(fault + ' staging failure preserves the running release', async (t) => {
    const host = await fixture(t, { fault });
    const result = await host.run(1);
    const status = await host.status();
    const transient = ['sigstore', 'registry'].includes(fault);
    assert.equal(result.decision, transient ? 'error' : 'failed');
    assert.equal(status.failed, transient ? undefined : candidate.digest);
    if (transient) assert.deepEqual(status, { committed: bootstrap.digest });
    else assert.match(status.reason, /signature|snapshot/);
    assert.equal(status.committed, bootstrap.digest);
    assert.equal(restartCount(await host.calls()), 0);
    assert.equal(await host.current(), bootstrap.digest);
    assert.equal(await host.exists('attempt'), false);
  });
}

for (const fault of ['health', 'running', 'session', 'error-screen', 'image', 'container']) {
  scenario(fault + ' rolls back and suppresses retries', async (t) => {
    const host = await fixture(t, { fault });
    const result = await host.run(1);
    assert.equal(result.decision, 'rolled-back');
    assert.equal(await host.current(), bootstrap.digest);
    const status = await host.status();
    assert.equal(status.failed, candidate.digest);
    assert.equal(status.committed, bootstrap.digest);
    assert.equal(await host.exists('attempt'), false);
    assert.match(status.reason, /health|session/);
    const calls = await host.calls();
    assert.equal(restartCount(calls), 2);
    assert.equal((await host.run()).decision, 'skip-failed');
    assert.deepEqual(await host.calls(), calls);
  });
}

scenario('both-health keeps the attempt until good passes its checks', async (t) => {
  const host = await fixture(t, { fault: 'both-health' });
  assert.equal((await host.run(1)).decision, 'rollback-failed');
  let status = await host.status();
  assert.equal(status.committed, null);
  assert.equal(status.failed, candidate.digest);
  assert.match(status.reason, /Gateway health.*known_good also failed.*Gateway health/);
  assert.equal(await host.exists('attempt'), true);
  assert.equal(await host.current(), bootstrap.digest);
  await host.setFault(null);
  assert.equal((await host.run(1)).decision, 'rolled-back');
  status = await host.status();
  assert.equal(status.committed, bootstrap.digest);
  assert.equal(status.failed, candidate.digest);
  assert.equal(await host.exists('attempt'), false);
  assert.equal((await host.run()).decision, 'skip-failed');
});

scenario('abort preserves failed marker and a different candidate clears it', async (t) => {
  const host = await fixture(t, { fault: 'health' });
  await host.run(1);
  await host.setRecord(idle);
  assert.equal((await host.run()).decision, 'no-op');
  assert.equal((await host.status()).failed, candidate.digest);
  await host.setRecord(rollout(bootstrap, next));
  assert.equal((await host.run()).decision, 'committed');
  assert.equal((await host.status()).failed, null);
});

for (const [label, id] of [
  ['before flip', bootstrap.digest],
  ['after flip', candidate.digest],
]) {
  scenario('recover attempt ' + label, async (t) => {
    const host = await fixture(t, {
      staged: [bootstrap, candidate],
      current: id,
      attempt: journal,
    });
    assert.equal((await host.run()).decision, 'committed');
    assert.equal((await host.status()).committed, candidate.digest);
    assert.equal(await host.current(), folder(candidate.digest));
    assert.equal(await host.exists('attempt'), false);
    assert.equal(restartCount(await host.calls()), 1);
  });
}

scenario('recover superseded attempt by checking record good', async (t) => {
  const host = await fixture(t, {
    staged: [bootstrap, candidate],
    current: candidate.digest,
    attempt: journal,
    record: idle,
  });
  assert.equal((await host.run(1)).decision, 'rolled-back');
  assert.equal(await host.current(), bootstrap.digest);
  assert.equal((await host.status()).failed, candidate.digest);
  assert.equal(await host.exists('attempt'), false);
  assert.equal(restartCount(await host.calls()), 1);
});

scenario('recover after a rollback outcome was already written', async (t) => {
  const host = await fixture(t, { staged: [bootstrap, candidate], attempt: journal });
  await host.save('committed', {
    committed: bootstrap.digest,
    failed: candidate.digest,
    reason: 'Gateway health unavailable',
  });
  assert.equal((await host.run(1)).decision, 'rolled-back');
  assert.equal(await host.current(), bootstrap.digest);
  assert.equal((await host.status()).failed, candidate.digest);
  assert.equal(await host.exists('attempt'), false);
  assert.equal(restartCount(await host.calls()), 1);
});

for (const current of [bootstrap.digest, candidate.digest]) {
  scenario(`restore good without R2 when target is unhealthy (${current})`, async (t) => {
    const host = await fixture(t, {
      staged: [bootstrap, candidate],
      current,
      attempt: journal,
      fault: 'health',
    });
    await host.setOffline(true);
    const before = await host.status();
    assert.equal((await host.run(1, '--check')).decision, 'error');
    assert.deepEqual(await host.status(), before);
    assert.equal(restartCount(await host.calls()), 0);
    assert.equal((await host.run(1)).decision, 'recovery-pending');
    assert.equal(await host.current(), bootstrap.digest);
    assert.equal((await host.status()).committed, bootstrap.digest);
    assert.equal(await host.exists('attempt'), true);
    assert.equal(restartCount(await host.calls()), 1);
    await host.setFault(null);
    await host.setOffline(false);
    assert.equal((await host.run()).decision, 'committed');
    assert.equal((await host.status()).committed, candidate.digest);
    assert.equal(await host.exists('attempt'), false);
  });
}

scenario('keep a healthy target running while R2 is unavailable', async (t) => {
  const host = await fixture(t, {
    staged: [bootstrap, candidate],
    current: candidate.digest,
    attempt: journal,
  });
  await host.setOffline(true);
  assert.equal((await host.run(1)).decision, 'recovery-pending');
  assert.equal(await host.current(), folder(candidate.digest));
  assert.equal(await host.exists('attempt'), true);
  assert.equal(restartCount(await host.calls()), 0);
});

scenario('offline recovery keeps an unverified rollback uncommitted', async (t) => {
  const host = await fixture(t, {
    staged: [bootstrap, candidate],
    current: candidate.digest,
    attempt: journal,
    fault: 'both-health',
  });
  await host.setOffline(true);
  assert.equal((await host.run(1)).decision, 'recovery-pending');
  const status = await host.status();
  assert.equal(status.committed, null);
  assert.equal(status.failed, null);
  assert.match(status.reason, /known_good also failed/);
  assert.equal(await host.exists('attempt'), true);
});

for (const [label, record, current] of [
  ['candidate', rollout(), bootstrap.digest],
  ['good', rollout(candidate, null, 'idle'), bootstrap.digest],
]) {
  scenario(`reuse retained ${label} while Sigstore is unavailable`, async (t) => {
    const host = await fixture(t, {
      staged: [bootstrap, candidate],
      record,
      current,
      fault: 'sigstore',
    });
    assert.equal((await host.run()).decision, 'committed');
    assert.equal(await host.current(), folder(candidate.digest));
    const calls = await host.calls();
    assert(
      calls.some(
        ([name, verb, action]) => name === 'docker' && verb === 'image' && action === 'inspect',
      ),
    );
    assert(
      !calls.some(([name, verb]) => name === 'cosign' || (name === 'docker' && verb === 'pull')),
    );
  });
}

scenario('missing retained image triggers signature verification and pull', async (t) => {
  const host = await fixture(t, { staged: [bootstrap, candidate], available: [bootstrap] });
  assert.equal((await host.run()).decision, 'committed');
  const calls = await host.calls();
  assert(calls.some(([name]) => name === 'cosign'));
  assert(calls.some(([name, verb]) => name === 'docker' && verb === 'pull'));
});

scenario('an unavailable Docker daemon does not trigger a pull', async (t) => {
  const host = await fixture(t, { staged: [bootstrap, candidate], fault: 'inspect-unavailable' });
  assert.equal((await host.run(1)).decision, 'error');
  assert.equal(await host.current(), bootstrap.digest);
  assert.deepEqual(await host.status(), { committed: bootstrap.digest });
  assert(
    !(await host.calls()).some(
      ([name, verb]) => name === 'cosign' || (name === 'docker' && verb === 'pull'),
    ),
  );
});

for (const runtime of ['runc', null]) {
  scenario(`reject session runtime ${runtime}`, async (t) => {
    const host = await fixture(t, { runtime });
    assert.equal((await host.run(1)).decision, 'rolled-back');
    assert.equal(await host.current(), bootstrap.digest);
    assert.equal((await host.status()).failed, candidate.digest);
    assert.match((await host.status()).reason, /container configuration/);
  });
}

scenario('restart the current good release when the gateway is down', async (t) => {
  const host = await fixture(t, { record: idle });
  await host.stopGateway();
  assert.equal((await host.run()).decision, 'committed');
  assert.equal((await host.status()).committed, bootstrap.digest);
  assert.equal((await host.status()).failed, null);
  assert.equal(restartCount(await host.calls()), 2);
});

scenario('restart a current release when another build is running', async (t) => {
  const host = await fixture(t, { record: idle, fault: 'running' });
  await host.setFault(null);
  assert.equal((await host.run()).decision, 'committed');
  assert.equal((await host.status()).committed, bootstrap.digest);
  assert.equal(restartCount(await host.calls()), 1);
});

scenario('a failing good release remains eligible for recovery', async (t) => {
  const host = await fixture(t, { record: idle, fault: 'running' });
  assert.equal((await host.run(1)).decision, 'check-failed');
  let status = await host.status();
  assert.equal(status.committed, null);
  assert.equal(status.failed, null);
  assert.match(status.reason, /health/);
  assert.equal(await host.exists('attempt'), true);
  await host.setFault(null);
  assert.equal((await host.run()).decision, 'committed');
  status = await host.status();
  assert.equal(status.committed, bootstrap.digest);
  assert.equal(status.failed, null);
  assert.equal(await host.exists('attempt'), false);
});

scenario('a good release with a signature mismatch remains eligible', async (t) => {
  const host = await fixture(t, { record: rollout(candidate, null, 'idle'), fault: 'annotations' });
  assert.equal((await host.run(1)).decision, 'check-failed');
  const status = await host.status();
  assert.equal(status.committed, bootstrap.digest);
  assert.equal(status.failed, null);
  assert.match(status.reason, /signature/);
  await host.setFault(null);
  assert.equal((await host.run()).decision, 'committed');
  assert.equal((await host.status()).committed, candidate.digest);
});

for (const fault of ['snapshot-signal', 'snapshot-launch']) {
  scenario(`${fault} classifies validator failure`, async (t) => {
    const host = await fixture(t, { fault });
    const result = await host.run(1);
    const status = await host.status();
    assert.equal(result.decision, fault === 'snapshot-signal' ? 'failed' : 'error');
    assert.equal(status.failed, fault === 'snapshot-signal' ? candidate.digest : undefined);
    assert.equal(await host.current(), bootstrap.digest);
    assert.equal(restartCount(await host.calls()), 0);
  });
}

scenario('record changes during staging', async (t) => {
  const host = await fixture(t, { fault: 'change-record' });
  assert.equal((await host.run()).decision, 'record-changed');
  assert.equal(await host.current(), bootstrap.digest);
  assert.equal(await host.exists('attempt'), false);
  assert.equal(restartCount(await host.calls()), 0);
  assert.deepEqual(await host.status(), { committed: bootstrap.digest });
});

scenario('idle deferral keeps its first timestamp and overrides after two hours', async (t) => {
  const host = await fixture(t, { busy: true });
  assert.equal((await host.run()).decision, 'deferred');
  const first = await host.run(0, '--check');
  assert.equal(first.decision, 'deferred');
  assert.equal(first.busy, true);
  assert.equal(first.deferred.target, candidate.digest);
  assert.equal(await host.current(), bootstrap.digest);
  assert.equal((await host.run()).decision, 'deferred');
  assert.deepEqual((await host.run(0, '--check')).deferred, first.deferred);
  assert.equal(restartCount(await host.calls()), 0);
  await host.save('deferred', { target: candidate.digest, since: Date.now() - 7_201_000 });
  assert.equal((await host.run()).decision, 'committed');
  assert.equal(await host.exists('deferred'), false);
});

scenario('prune preserves good, current and previous directories and images', async (t) => {
  const stale = release('c', 9);
  const host = await fixture(t, {
    record: rollout(bootstrap, next),
    staged: [bootstrap, candidate, stale],
    extra: [stale],
    current: candidate.digest,
    committed: candidate.digest,
  });
  assert.equal((await host.run()).decision, 'committed');
  assert.deepEqual(
    new Set(await host.entries()),
    new Set(['current', ...[bootstrap, candidate, next].map((r) => folder(r.digest))]),
  );
  assert.deepEqual(
    new Set(await host.images()),
    new Set([bootstrap, candidate, next].map((r) => image(r.digest))),
  );
});

scenario('--check reports a decision without modifying local state', async (t) => {
  const host = await fixture(t);
  const before = await host.fingerprint();
  const result = await host.run(0, '--check');
  assert.equal(result.decision, 'stage');
  assert.equal(result.running, bootstrap.digest);
  assert.equal(result.target, candidate.digest);
  assert.deepEqual(await host.fingerprint(), before);
  assert.deepEqual(await host.images(), [image(bootstrap.digest)]);
  assert.deepEqual(await host.calls(), [
    ['docker', 'ps', '-q', '--filter', 'label=hn-preview=local'],
  ]);
});

scenario('current without matching committed must run checks', async (t) => {
  const host = await fixture(t, { staged: [bootstrap, candidate], current: candidate.digest });
  assert.equal((await host.run()).decision, 'committed');
  assert.equal((await host.status()).committed, candidate.digest);
  const calls = await host.calls();
  assert.equal(restartCount(calls), 0);
  assert(calls.some(([name, call]) => name === 'docker' && call === 'container'));
});

scenario('production follows stage policy and retries a full session', async (t) => {
  const host = await fixture(t, { role: 'prod', fault: 'full' });
  assert.equal((await host.run()).decision, 'no-op');
  assert.deepEqual(await host.calls(), []);
  await host.setRecord(rollout(bootstrap, candidate, 'prod'));
  assert.equal((await host.run()).decision, 'committed');
  assert((await host.calls()).some(([name]) => name === 'session-full'));
});

scenario('validator timeout remains transient', async (t) => {
  const host = await fixture(t, { fault: 'snapshot-timeout', runTimeout: 80_000 });
  const result = await host.run(1);
  assert.equal(result.decision, 'error');
  assert.match(result.reason, /validation could not run/);
  assert.deepEqual(await host.status(), { committed: bootstrap.digest });
  assert.equal(await host.current(), bootstrap.digest);
  assert.equal(restartCount(await host.calls()), 0);
});

scenario('five minutes of full sessions defer the candidate without failing it', async (t) => {
  const host = await fixture(t, { fault: 'full-forever', runTimeout: 330_000 });
  const started = Date.now();
  const result = await host.run(1);
  assert(Date.now() - started >= 299_000);
  assert.equal(result.decision, 'rolled-back');
  let status = await host.status();
  assert.equal(status.committed, bootstrap.digest);
  assert.equal(status.failed, null);
  assert.match(status.reason, /session limit/);
  assert.equal(await host.current(), bootstrap.digest);
  assert.equal(await host.exists('attempt'), false);
  assert((await host.calls()).filter(([name]) => name === 'session-full').length > 1);
  assert((await host.calls()).some(([name]) => name === 'session-pending'));
  await host.setFault(null);
  assert.equal((await host.run()).decision, 'committed');
  status = await host.status();
  assert.equal(status.committed, candidate.digest);
  assert.equal(status.failed, null);
});

scenario('capacity and failed rollback recover good before retrying the candidate', async (t) => {
  const host = await fixture(t, { fault: 'full-forever-bad-good', runTimeout: 330_000 });
  assert.equal((await host.run(1)).decision, 'rollback-failed');
  let status = await host.status();
  assert.equal(status.committed, null);
  assert.equal(status.failed, null);
  assert.match(status.reason, /session limit.*known_good also failed/);
  assert.equal(await host.exists('attempt'), true);
  await host.setFault(null);
  assert.equal((await host.run(1)).decision, 'rolled-back');
  status = await host.status();
  assert.equal(status.committed, bootstrap.digest);
  assert.equal(status.failed, null);
  assert.equal(await host.exists('attempt'), false);
  assert.equal((await host.run()).decision, 'committed');
  assert.equal((await host.status()).committed, candidate.digest);
});

test('reader updater', { concurrency: 6 }, async (t) => {
  await Promise.all(scenarios.map(({ name, run }) => t.test(name, run)));
});
