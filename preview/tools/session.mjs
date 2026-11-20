import WebSocket from 'ws';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { setTimeout as wait } from 'node:timers/promises';
import { gatewayAddress, readLocalHealth, ReleaseError } from './release.mjs';

const full = Symbol('session limit');

function render(socket, timeout, ready) {
  return new Promise((resolve, reject) => {
    const fail = () => reject(new ReleaseError('Synthetic session did not render the feed'));
    socket.once('error', fail);
    socket.once('close', fail);
    socket.on('unexpected-response', (_, response) => {
      const { statusCode } = response;
      response.destroy();
      reject(statusCode === 503 ? full : new ReleaseError(`Session HTTP ${statusCode}`));
    });
    let tail = '',
      top = false,
      fresh = false,
      id;
    socket.on('message', (bytes, binary) => {
      if (binary) {
        const text = tail + bytes.toString();
        top ||= text.includes('Top');
        fresh ||= text.includes('New');
        tail = text.slice(-2);
      } else {
        try {
          const message = JSON.parse(bytes);
          if (message.type === 'ended') return fail();
          if (message.type === 'ready') {
            if (!/^[a-f0-9]{64}$/.test(message.id)) return fail();
            id = message.id;
            ready(id);
          }
        } catch {
          return fail();
        }
      }
      // Docker attach can deliver terminal bytes before ready.
      if (id && top && fresh) resolve();
    });
    const timer = setTimeout(fail, timeout);
    socket.once('close', () => clearTimeout(timer));
  });
}

export async function checkRelease(id, image, snapshot, docker) {
  const hash = createHash('sha256')
    .update(await readFile(snapshot))
    .digest('hex');
  const startup = performance.now() + 10_000;
  let health;
  // Type=simple reports a restart before the process opens its listening socket.
  while (!health) {
    try {
      health = await readLocalHealth();
      if (health?.snapshot !== hash || health?.running !== id)
        throw new ReleaseError('Gateway health snapshot or running release differs');
    } catch (error) {
      if (error.code !== 'ECONNREFUSED' || performance.now() >= startup)
        throw error instanceof ReleaseError
          ? error
          : new ReleaseError('Gateway health unavailable');
      await wait(200);
    }
  }
  const { authority, host, origin } = gatewayAddress();
  const deadline = performance.now() + 300_000;
  while (performance.now() < deadline) {
    const socket = new WebSocket(`ws://${authority}/session`, { headers: { Host: host }, origin });
    let container;
    try {
      await render(socket, Math.min(30_000, deadline - performance.now()), (id) => {
        container = id;
      });
      const inspect = ['container', 'inspect', '--format', '{{json .}}', container];
      const { Image, HostConfig: config } = JSON.parse(await docker(inspect, 10_000));
      const runtime = process.env.DOCKER_RUNTIME;
      if (
        socket.readyState !== WebSocket.OPEN ||
        Image !== image ||
        (runtime && config.Runtime !== runtime) ||
        config.NetworkMode !== 'none' ||
        config.ReadonlyRootfs !== true ||
        config.Memory !== 268435456
      )
        throw new ReleaseError('Synthetic session container configuration differs');
      return;
    } catch (error) {
      if (error !== full) throw error;
    } finally {
      socket.close();
      try {
        if (container) {
          const until = performance.now() + 10_000;
          const filter = ['ps', '-aq', '--no-trunc', '--filter', `id=${container}`];
          while (
            performance.now() < until &&
            (await docker(filter, Math.max(1, Math.ceil(until - performance.now()))))
          )
            await wait(100);
          if (performance.now() >= until)
            throw new ReleaseError('Synthetic session container was not removed');
        }
      } finally {
        socket.terminate();
      }
    }
    await wait(Math.min(1000, Math.max(0, deadline - performance.now())));
  }
  throw new ReleaseError('Synthetic session limit persisted for five minutes');
}
