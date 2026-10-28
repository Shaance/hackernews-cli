import assert from 'node:assert/strict';
import { WebSocket } from 'ws';
import { docker } from './docker.mjs';
import { execFileSync } from 'node:child_process';
import { writeFile, mkdir } from 'node:fs/promises';
const base = process.env.PREVIEW_URL || 'http://127.0.0.1:8765';
const socketURL = base.replace('http', 'ws') + '/session';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(check, ms = 5000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await sleep(50);
  }
  throw Error('Condition timed out');
}
async function connect() {
  const ws = new WebSocket(socketURL, { origin: base });
  const s = { ws, output: '', events: [], connectedAt: performance.now() };
  ws.on('message', (data, binary) => {
    if (binary) s.output += data.toString();
    else {
      s.events.push(JSON.parse(data));
      const ready = s.events.find((e) => e.type === 'ready');
      if (ready) Object.assign(s, ready);
    }
  });
  ws.on('error', () => {});
  await until(() => s.id, 15000);
  return s;
}
const input = (s, data) => s.ws.send(JSON.stringify({ type: 'input', data }));
async function removed(id) {
  await until(
    async () => !(await docker('GET', '/containers/json?all=1')).some((c) => c.Id === id),
  );
}
function exec(id, cmd) {
  return execFileSync(
    'docker',
    [
      '--host',
      `unix://${process.env.DOCKER_SOCKET || process.env.HOME + '/.docker/run/docker.sock'}`,
      'exec',
      id,
      'sh',
      '-c',
      cmd,
    ],
    { encoding: 'utf8', timeout: 12000 },
  );
}
const checks = [];
const pass = (name, detail) => {
  checks.push({ name, detail });
  console.log('PASS', name, detail || '');
};
const opened = [];
try {
  const active = await fetch(base + '/healthz').then((r) => r.json());
  const snap = await fetch(base + `/snapshots/${active.snapshot}.json`).then((r) => r.json());
  assert.equal(snap.version, 1);
  for (const ids of Object.values(snap.feeds)) {
    assert(ids.length > 0 && ids.length <= 20);
    for (const id of ids) assert(snap.items[id]);
  }
  for (const item of Object.values(snap.items))
    for (const child of item.kids || []) assert(snap.items[child]);
  pass('Snapshot references are complete', {
    stories: Object.keys(snap.articles).length,
    items: Object.keys(snap.items).length,
  });
  const bad = new WebSocket(socketURL, { origin: 'https://untrusted.invalid' });
  const rejected = await new Promise((resolve) => {
    bad.on('unexpected-response', (_req, res) => {
      res.resume();
      bad.terminate();
      resolve(res.statusCode);
    });
    bad.on('error', () => {});
  });
  assert.equal(rejected, 403);
  pass('Cross-origin WebSocket denied');
  const a = await connect();
  opened.push(a);
  await until(() => a.output.includes(snap.items[snap.feeds.best[0]].title));
  pass('Real CLI renders captured Best feed', {
    startup_ms: a.ms,
    first_feed_ms: Math.round(performance.now() - a.connectedAt),
  });
  const usage = await docker('GET', `/containers/${a.id}/stats?stream=false&one-shot=true`);
  pass('Idle resource sample', {
    memory_bytes: usage.memory_stats?.usage,
    pids: usage.pids_stats?.current,
  });
  input(a, '2');
  await until(() => a.output.includes(snap.items[snap.feeds.new[0]].title));
  input(a, '1');
  await until(() => a.output.includes(snap.items[snap.feeds.top[0]].title));
  pass('Native category switching works offline');
  input(a, '\r');
  await until(() => a.output.includes('hn-article=' + snap.feeds.top[0]));
  pass('CLI emits captured article selection');
  const config = await docker('GET', `/containers/${a.id}/json`);
  assert.equal(config.HostConfig.NetworkMode, 'none');
  assert.equal(config.HostConfig.ReadonlyRootfs, true);
  assert.deepEqual(config.HostConfig.CapDrop, ['ALL']);
  assert.equal(config.HostConfig.Memory, 268435456);
  assert.equal(config.HostConfig.PidsLimit, 64);
  assert.equal(config.HostConfig.NanoCpus, 500000000);
  assert.equal(config.Mounts.filter((m) => m.Type === 'bind').length, 1);
  assert.equal(config.Mounts.find((m) => m.Type === 'bind').RW, false);
  const isolation = exec(
    a.id,
    'set -e; id; cat /proc/net/dev; cat /proc/self/status | grep -E "^(CapEff|NoNewPrivs|Seccomp):"; test ! -S /var/run/docker.sock; test ! -e /etc/tldw; if touch /etc/hn-probe 2>/dev/null; then exit 90; fi; touch /tmp/hn-probe',
  );
  assert.match(isolation, /uid=65534/);
  assert.match(isolation, /CapEff:\s+0000000000000000/);
  assert.match(isolation, /NoNewPrivs:\s+1/);
  pass('Non-root, no capabilities, readonly root, writable temporary storage', isolation.trim());
  const network = exec(
    a.id,
    'for host in 1.1.1.1 169.254.169.254 172.17.0.1 192.168.65.254; do if timeout 2 bash -c "echo > /dev/tcp/$host/80" 2>/dev/null; then exit 91; fi; done; if timeout 2 getent hosts example.com >/dev/null; then exit 92; fi; cat /proc/net/route',
  );
  assert.equal(network.trim().split('\n').length, 1);
  pass('Public, metadata, host-bridge TCP and DNS blocked');
  const b = await connect();
  opened.push(b);
  assert(!exec(b.id, 'ls /tmp').includes('hn-probe'));
  pass('Sessions do not share temporary state');
  const c = await connect();
  opened.push(c);
  const fourth = new WebSocket(socketURL, { origin: base });
  fourth.on('error', () => {});
  const limited = await new Promise((resolve) =>
    fourth.on('unexpected-response', (_req, res) => {
      res.resume();
      fourth.terminate();
      resolve(res.statusCode);
    }),
  );
  assert.equal(limited, 503);
  pass('Fourth simultaneous session rejected');
  b.ws.close();
  await removed(b.id);
  pass('Disconnect removes container');
  input(c, 'q');
  await removed(c.id);
  pass('Quit ends session without a shell');
  a.ws.close();
  await removed(a.id);
  const final = await connect();
  opened.push(final);
  pass('Capacity becomes available after cleanup', { startup_ms: final.ms });
  input(final, 'q');
  await removed(final.id);
  const invalid = await connect();
  opened.push(invalid);
  invalid.ws.send(JSON.stringify({ type: 'exec', command: 'id' }));
  await removed(invalid.id);
  pass('Arbitrary command messages rejected and session removed');
  await mkdir(new URL('./evidence/', import.meta.url), { recursive: true });
  await writeFile(
    new URL('./evidence/boundary-tests.json', import.meta.url),
    JSON.stringify({ at: new Date().toISOString(), checks }, null, 2),
  );
} finally {
  for (const s of opened) s.ws.close();
}
