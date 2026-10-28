import assert from 'node:assert/strict';
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { readFile, writeFile, mkdtemp, rm, mkdir } from 'node:fs/promises';
import { WebSocket } from 'ws';
import { docker } from './docker.mjs';
const origin = 'https://hn.hashcode.dev',
  host = 'hn-session.hashcode.dev';
const cert = await readFile(process.env.TEST_TLS_CERT),
  key = await readFile(process.env.TEST_TLS_KEY);
await mkdir(new URL('./.runtime/', import.meta.url), { recursive: true });
const dir = await mkdtemp(new URL('./.runtime/public-check-', import.meta.url));
const snapshotPath = dir + '/snapshot.json',
  snapshot = JSON.parse(await readFile(new URL('./fixtures/snapshot.json', import.meta.url)));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(fn, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await fn()) return;
    await sleep(50);
  }
  throw Error('Condition timed out');
}
const listed = () =>
  docker(
    'GET',
    '/containers/json?all=1&filters=' +
      encodeURIComponent(JSON.stringify({ label: ['hn-preview=local', 'hn-preview-port=8770'] })),
  );
let child,
  log = '';
async function start() {
  log = '';
  child = spawn(process.execPath, ['server.mjs'], {
    cwd: new URL('.', import.meta.url),
    env: {
      ...process.env,
      PORT: '8770',
      PREVIEW_ORIGIN: origin,
      PREVIEW_HOST: host,
      SNAPSHOT: snapshotPath,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.on('data', (b) => (log += b));
  child.stderr.on('data', (b) => (log += b));
  await until(() => {
    if (child.exitCode !== null) throw Error(log);
    return log.includes('Preview ready');
  });
}
async function stop() {
  const done = once(child, 'exit');
  child.kill('SIGTERM');
  await done;
}
// Loopback TLS terminator preserves Host and Origin exactly as a real proxy must.
const proxy = https.createServer({ key, cert }, (req, res) => {
  const up = http.request(
    { host: '127.0.0.1', port: 8770, path: req.url, method: req.method, headers: req.headers },
    (r) => {
      res.writeHead(r.statusCode, r.headers);
      r.pipe(res);
    },
  );
  up.on('error', () => res.writeHead(502).end());
  req.pipe(up);
});
proxy.on('upgrade', (req, socket, head) => {
  const up = net.connect(8770, '127.0.0.1', () => {
    up.write(
      `${req.method} ${req.url} HTTP/1.1\r\n${req.rawHeaders.reduce((s, v, i) => s + v + (i % 2 ? '\r\n' : ': '), '')}\r\n`,
    );
    if (head.length) up.write(head);
    socket.pipe(up);
    up.pipe(socket);
  });
  up.on('error', () => socket.destroy());
  socket.on('error', () => up.destroy());
  socket.on('close', () => up.destroy());
});
await new Promise((resolve) => proxy.listen(8771, '127.0.0.1', resolve));
let snapshotRoute = '/healthz';
function get(headers = { Origin: origin, Host: host }) {
  return new Promise((resolve, reject) => {
    const req = https.get(
      {
        hostname: '127.0.0.1',
        port: 8771,
        path: snapshotRoute,
        ca: cert,
        servername: 'localhost',
        headers,
      },
      (res) => {
        let body = '';
        res.on('data', (b) => (body += b));
        res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body }));
      },
    );
    req.on('error', reject);
  });
}
function socket(from = origin, to = host) {
  return new WebSocket('wss://127.0.0.1:8771/session', {
    ca: cert,
    servername: 'localhost',
    origin: from,
    headers: { Host: to },
  });
}
async function denied(from, to) {
  const ws = socket(from, to);
  ws.on('error', () => {});
  return new Promise((resolve) =>
    ws.on('unexpected-response', (_req, res) => {
      res.resume();
      ws.terminate();
      resolve(res.statusCode);
    }),
  );
}
let ws;
const checks = [];
try {
  await writeFile(snapshotPath, JSON.stringify(snapshot));
  await start();
  snapshotRoute = `/snapshots/${JSON.parse((await get()).body).snapshot}.json`;
  const first = await get();
  assert.equal(first.status, 200);
  assert.equal(first.headers['access-control-allow-origin'], origin);
  assert.equal(
    (await get({ Host: host, Origin: 'https://untrusted.invalid' })).headers[
      'access-control-allow-origin'
    ],
    undefined,
  );
  assert.equal((await get({ Host: 'wrong.invalid', Origin: origin })).status, 403);
  assert.equal(await denied('https://untrusted.invalid', host), 403);
  assert.equal(await denied(origin, 'wrong.invalid'), 403);
  checks.push(
    'Verified private TLS; exact allowed frontend CORS; foreign origin and wrong Host denied',
  );
  ws = socket();
  let ready,
    output = '';
  ws.on('error', () => {});
  ws.on('message', (data, binary) => {
    if (binary) output += data;
    else {
      const v = JSON.parse(data);
      if (v.type === 'ready') ready = v;
    }
  });
  await until(() => ready);
  await until(() => output.includes('example.com'));
  ws.send(JSON.stringify({ type: 'input', data: '\r' }));
  await until(() => output.includes('hn-article=50225695'));
  checks.push('Real native CLI runs over WSS; Enter opens the selected snapshot story');
  const changed = {
    ...snapshot,
    generated_at: new Date(Date.parse(snapshot.generated_at) + 60000).toISOString(),
  };
  await writeFile(snapshotPath, JSON.stringify(changed));
  assert.equal((await get()).body, first.body);
  checks.push(
    'Active gateway and sessions keep their immutable snapshot after source file changes',
  );
  ws.close();
  await until(async () => !(await listed()).length);
  await stop();
  checks.push('HTTPS WebSocket disconnect removes the visitor container');
  await start();
  assert.equal((await get()).status, 404);
  snapshotRoute = '/healthz';
  snapshotRoute = `/snapshots/${JSON.parse((await get()).body).snapshot}.json`;
  assert.equal(JSON.parse((await get()).body).generated_at, changed.generated_at);
  await stop();
  checks.push('Explicit gateway restart adopts the new snapshot');
  await writeFile(snapshotPath, 'invalid-json');
  let rejected = false;
  try {
    await start();
  } catch {
    rejected = true;
  }
  assert(rejected);
  assert.notEqual(child.exitCode, 0);
  await writeFile(snapshotPath, JSON.stringify(changed));
  await start();
  assert.equal((await get()).status, 200);
  await stop();
  checks.push('Invalid snapshot refuses startup; restoring last validated file recovers service');
  console.log(
    JSON.stringify(
      {
        checks,
        limits:
          'Private test CA, loopback-only proxy; public DNS/certificate and Vercel routing not deployed. Snapshot updates currently require gateway restart.',
      },
      null,
      2,
    ),
  );
} finally {
  ws?.terminate();
  if (child?.exitCode === null) await stop();
  proxy.closeAllConnections();
  await new Promise((resolve) => proxy.close(resolve));
  await rm(dir, { recursive: true, force: true });
}
