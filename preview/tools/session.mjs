import WebSocket from 'ws';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { StringDecoder } from 'node:string_decoder';
import { setTimeout as wait } from 'node:timers/promises';
import { gatewayAddress, readLocalHealth, ReleaseError } from './release.mjs';

const full = Symbol('session limit');
const timedOut = Symbol('session timeout');
export class SessionCapacityError extends ReleaseError {}

function render(socket, timeoutMs, title, ready) {
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
      story = false,
      id;
    const decoder = new StringDecoder('utf8');
    socket.on('message', (bytes, binary) => {
      if (binary) {
        const text = tail + decoder.write(bytes);
        top ||= text.includes('Top');
        fresh ||= text.includes('New');
        story ||= text.includes(title);
        tail = text.slice(-Math.max(2, title.length - 1));
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
      if (id && top && fresh && story) resolve();
    });
    const timer = setTimeout(() => reject(timedOut), timeoutMs);
    socket.once('close', () => clearTimeout(timer));
  });
}

export async function checkHealth(id, snapshot) {
  const bytes = await readFile(snapshot);
  const hash = createHash('sha256').update(bytes).digest('hex');
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
  return bytes;
}

export async function checkRelease(id, image, snapshot, docker) {
  const bytes = await checkHealth(id, snapshot);
  const data = JSON.parse(bytes);
  const title = (data.feeds?.best ?? [])
    .map((id) => Array.from(data.items?.[id]?.title?.trim() ?? '').slice(0, 20))
    .find((prefix) => prefix.length >= 12)
    ?.join('');
  if (!title) throw new ReleaseError('Snapshot Best feed has no title with at least 12 characters');
  const { authority, host, origin } = gatewayAddress();
  const deadline = performance.now() + 300_000;
  let capacitySeen = false;
  while (performance.now() < deadline) {
    const socket = new WebSocket(`ws://${authority}/session`, { headers: { Host: host }, origin });
    const remaining = deadline - performance.now();
    let container;
    try {
      await render(socket, Math.min(30_000, remaining), title, (id) => {
        container = id;
      });
      const inspect = ['container', 'inspect', '--format', '{{json .}}', container];
      const { Image, HostConfig: config } = JSON.parse(await docker(inspect, 10_000));
      if (
        socket.readyState !== WebSocket.OPEN ||
        Image !== image ||
        config.Runtime !== 'runsc-rootless' ||
        config.NetworkMode !== 'none' ||
        config.ReadonlyRootfs !== true ||
        config.Memory !== 268435456
      )
        throw new ReleaseError('Synthetic session container configuration differs');
      return;
    } catch (error) {
      if (error === full) capacitySeen = true;
      else if (error === timedOut && capacitySeen && remaining <= 30_000) break;
      else if (error === timedOut)
        throw new ReleaseError('Synthetic session did not render the feed');
      else throw error;
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
  throw new SessionCapacityError('Synthetic session limit persisted for five minutes');
}
