import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { WebSocket } from 'ws';
import { docker } from './docker.mjs';
import { writeFile, mkdir } from 'node:fs/promises';
const base = 'http://127.0.0.1:8768',
  socketURL = base.replace('http', 'ws') + '/session';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(fn, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await fn()) return;
    await sleep(50);
  }
  throw Error('Condition timed out');
}
let child,
  log = '';
async function start(seconds) {
  child = spawn(process.execPath, ['server.mjs'], {
    cwd: new URL('.', import.meta.url),
    env: {
      ...process.env,
      PORT: '8768',
      SESSION_SECONDS: String(seconds),
      SNAPSHOT: new URL('./fixtures/snapshot.json', import.meta.url).pathname,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.on('data', (b) => (log += b));
  child.stderr.on('data', (b) => (log += b));
  await until(() => log.includes('Preview ready'));
  return child;
}
async function connect() {
  const ws = new WebSocket(socketURL, { origin: base }),
    session = { ws, events: [] };
  ws.on('error', () => {});
  ws.on('message', (data, binary) => {
    if (!binary) {
      const event = JSON.parse(data);
      session.events.push(event);
      if (event.type === 'ready') session.id = event.id;
    }
  });
  await until(() => session.id);
  return session;
}
const listed = () =>
  docker(
    'GET',
    '/containers/json?all=1&filters=' +
      encodeURIComponent(JSON.stringify({ label: ['hn-preview=local', 'hn-preview-port=8768'] })),
  );
const checks = [];
try {
  await start(2);
  const a = await connect();
  await until(() => a.events.some((e) => e.text === 'time limit reached'));
  await until(async () => !(await listed()).length);
  checks.push('Two-second session lifetime terminates and removes the container');
  child.kill('SIGTERM');
  await once(child, 'exit');
  log = '';
  await start(600);
  const b = await connect();
  child.kill('SIGKILL');
  await once(child, 'exit');
  assert.equal((await listed()).length, 1);
  checks.push('Forced gateway crash leaves a detectable labeled orphan');
  log = '';
  await start(600);
  assert.equal((await listed()).length, 0);
  checks.push('Restart reuses immutable snapshot and reaps its orphan');
  const c = await connect();
  child.kill('SIGTERM');
  await once(child, 'exit');
  assert.equal((await listed()).length, 0);
  checks.push('Graceful shutdown removes active sessions');
  a.ws.terminate();
  b.ws.terminate();
  c.ws.terminate();
  const rapid = [];
  log = '';
  await start(600);
  for (let i = 0; i < 3; i++) {
    const ws = new WebSocket(socketURL, { origin: base });
    ws.on('error', () => {});
    ws.on('open', () => ws.close());
    rapid.push(ws);
  }
  await sleep(1500);
  assert.equal((await listed()).length, 0);
  checks.push('Disconnect during startup does not leak a container');
  child.kill('SIGTERM');
  await once(child, 'exit');
  console.log(checks);
  await mkdir(new URL('./evidence/', import.meta.url), { recursive: true });
  await writeFile(
    new URL('./evidence/lifecycle-tests.json', import.meta.url),
    JSON.stringify({ at: new Date().toISOString(), checks }, null, 2),
  );
} finally {
  child?.kill('SIGTERM');
}
