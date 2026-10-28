import http from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { WebSocketServer, WebSocket } from 'ws';
import { docker, attach } from './docker.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const port = Number(process.env.PORT || 8765);
const origin = process.env.PREVIEW_ORIGIN || `http://127.0.0.1:${port}`;
const publicHost = process.env.PREVIEW_HOST || `127.0.0.1:${port}`;
const maxSessions = 3;
const image = (
  await docker(
    'GET',
    `/images/${encodeURIComponent(process.env.PREVIEW_IMAGE || 'hn-preview:local')}/json`,
  )
).Id;
const lifetime = Number(process.env.SESSION_SECONDS || 600) * 1000;
if (!(lifetime >= 1000 && lifetime <= 600000))
  throw new Error('Session lifetime must be 1–600 seconds');
const bytes = await readFile(resolve(root, process.env.SNAPSHOT || 'snapshot.json'));
const snapshot = JSON.parse(bytes);
if (snapshot.version !== 1 || !snapshot.feeds?.top?.length) throw new Error('Invalid snapshot');
const hash = createHash('sha256').update(bytes).digest('hex');
const cache = resolve(root, '.runtime', hash);
await mkdir(cache, { recursive: true });
const snapshotPath = resolve(cache, 'snapshot.json');
try {
  await writeFile(snapshotPath, bytes, { mode: 0o444, flag: 'wx' });
} catch (error) {
  if (error.code !== 'EEXIST' || !(await readFile(snapshotPath)).equals(bytes)) throw error;
}
const assets = new Map([
  ['/', ['index.html', 'text/html']],
  ['/favicon.svg', ['favicon.svg', 'image/svg+xml']],
  ['/app.js', ['app.js', 'text/javascript']],
  ['/style.css', ['style.css', 'text/css']],
  ['/xterm.js', ['node_modules/@xterm/xterm/lib/xterm.js', 'text/javascript']],
  ['/fit.js', ['node_modules/@xterm/addon-fit/lib/addon-fit.js', 'text/javascript']],
  ['/xterm.css', ['node_modules/@xterm/xterm/css/xterm.css', 'text/css']],
]);
const sessions = new Set();
let stopping = false;
const server = http.createServer(async (req, res) => {
  res.setHeader(
    'Content-Security-Policy',
    "default-src 'self'; connect-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self'; frame-src https: http:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'",
  );
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('Cache-Control', 'no-store');
  if (req.headers.host !== publicHost || req.method !== 'GET') {
    res.writeHead(403).end();
    return;
  }
  res.setHeader('Vary', 'Origin');
  if (req.headers.origin === origin) res.setHeader('Access-Control-Allow-Origin', origin);
  if (req.url === '/healthz') {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ snapshot: hash }));
    return;
  }
  if (req.url === `/snapshots/${hash}.json`) {
    res.setHeader('Content-Type', 'application/json');
    res.end(bytes);
    return;
  }
  const asset = assets.get(req.url);
  if (!asset) {
    res.writeHead(404).end();
    return;
  }
  try {
    res.setHeader('Content-Type', asset[1]);
    res.end(await readFile(resolve(root, asset[0])));
  } catch {
    res.writeHead(500).end('Asset unavailable');
  }
});
const wss = new WebSocketServer({ noServer: true, maxPayload: 8192, perMessageDeflate: false });
server.on('upgrade', (req, socket, head) => {
  if (
    stopping ||
    req.url !== '/session' ||
    req.headers.origin !== origin ||
    req.headers.host !== publicHost
  ) {
    socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
    return;
  }
  if (sessions.size >= maxSessions) {
    socket.end('HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n');
    return;
  }
  wss.handleUpgrade(req, socket, head, (ws) => {
    const task = session(ws);
    sessions.add(task);
    task.finally(() => sessions.delete(task));
  });
});
function message(ws, value) {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(value));
}
async function session(ws) {
  const started = performance.now();
  let id, terminal, timer;
  let cols = 90,
    rows = 30;
  let endSession;
  const ended = new Promise((resolve) => {
    endSession = resolve;
  });
  ws.on('close', () => endSession('disconnected'));
  ws.on('error', () => endSession('connection error'));
  ws.on('message', (data) => {
    try {
      const event = JSON.parse(data.toString());
      if (event.type === 'input' && typeof event.data === 'string' && event.data.length <= 4096) {
        if (terminal && !terminal.write(event.data)) ws._socket.pause();
      } else if (
        event.type === 'resize' &&
        Number.isInteger(event.cols) &&
        Number.isInteger(event.rows)
      ) {
        cols = Math.max(30, Math.min(200, event.cols));
        rows = Math.max(10, Math.min(80, event.rows));
        if (id)
          docker('POST', `/containers/${id}/resize?w=${cols}&h=${rows}`).catch(() =>
            endSession('resize failed'),
          );
      } else endSession('invalid input');
    } catch {
      endSession('invalid input');
    }
  });
  try {
    message(ws, { type: 'status', text: 'Starting offline CLI…' });
    const result = await docker('POST', `/containers/create?name=hn-preview-${randomUUID()}`, {
      Image: image,
      User: '65534:65534',
      Tty: true,
      OpenStdin: true,
      AttachStdin: true,
      AttachStdout: true,
      AttachStderr: true,
      Env: [
        'HOME=/tmp',
        'TERM=xterm-256color',
        'COLORTERM=truecolor',
        'HN_THEME=dark',
        'HN_REDUCED_MOTION=1',
      ],
      Labels: { 'hn-preview': 'local', 'hn-preview-port': String(port) },
      HostConfig: {
        NetworkMode: 'none',
        ReadonlyRootfs: true,
        CapDrop: ['ALL'],
        SecurityOpt: ['no-new-privileges:true'],
        Memory: 268435456,
        MemorySwap: 268435456,
        NanoCpus: 500000000,
        PidsLimit: 64,
        Binds: [`${snapshotPath}:/data/snapshot.json:ro`],
        Tmpfs: { '/tmp': 'rw,noexec,nosuid,size=16m,mode=1777' },
        LogConfig: { Type: 'none' },
        ...(process.env.DOCKER_RUNTIME ? { Runtime: process.env.DOCKER_RUNTIME } : {}),
      },
    });
    id = result.Id;
    if (ws.readyState !== WebSocket.OPEN) return;
    terminal = await attach(id);
    terminal.on('error', () => endSession('terminal error'));
    terminal.on('drain', () => ws._socket.resume());
    terminal.on('data', (data) => {
      if (ws.readyState !== WebSocket.OPEN) return;
      if (ws.bufferedAmount > 1024 * 1024) {
        endSession('slow client');
        return;
      }
      ws.send(data, { binary: true });
    });
    terminal.on('end', () => endSession('CLI exited'));
    await docker('POST', `/containers/${id}/start`);
    await docker('POST', `/containers/${id}/resize?w=${cols}&h=${rows}`);
    timer = setTimeout(() => endSession('time limit reached'), lifetime);
    const ms = Math.round(performance.now() - started);
    console.log(JSON.stringify({ event: 'started', id, ms, snapshot: hash }));
    message(ws, { type: 'ready', id, ms, hash, generated_at: snapshot.generated_at });
    const reason = await ended;
    message(ws, { type: 'ended', text: reason });
  } catch (e) {
    console.error(e.message);
    message(ws, { type: 'ended', text: 'Could not start this session. Try again.' });
  } finally {
    clearTimeout(timer);
    terminal?.destroy();
    if (id) {
      try {
        await docker('DELETE', `/containers/${id}?force=1`);
        console.log(JSON.stringify({ event: 'removed', id }));
      } catch (e) {
        console.error('CLEANUP FAILED', id, e.message);
      }
    }
    ws.close();
  }
}
// Only this prototype's labeled containers are eligible for recovery cleanup.
for (const c of await docker(
  'GET',
  '/containers/json?all=1&filters=' +
    encodeURIComponent(JSON.stringify({ label: ['hn-preview=local', `hn-preview-port=${port}`] })),
)) {
  await docker('DELETE', `/containers/${c.Id}?force=1`);
}
server.listen(port, process.env.PREVIEW_BIND || '127.0.0.1', () =>
  console.log(
    `Preview ready at http://127.0.0.1:${port}; snapshot ${hash}; lifetime ${lifetime}ms`,
  ),
);
for (const signal of ['SIGINT', 'SIGTERM'])
  process.on(signal, async () => {
    if (stopping) return;
    stopping = true;
    server.close();
    for (const ws of wss.clients) ws.close(1001, 'Gateway stopping');
    await Promise.allSettled([...sessions]);
    process.exit(0);
  });
