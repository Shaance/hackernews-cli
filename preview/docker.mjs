import http from 'node:http';
import os from 'node:os';
export const socketPath = process.env.DOCKER_SOCKET || `${os.homedir()}/.docker/run/docker.sock`;
export function docker(method, path, body, timeout = 15000) {
  return new Promise((resolve, reject) => {
    const request = http.request(
      {
        socketPath,
        path: `/v1.51${path}`,
        method,
        headers: { 'Content-Type': 'application/json' },
        timeout,
      },
      (response) => {
        const chunks = [];
        response.on('data', (c) => chunks.push(c));
        response.on('end', () => {
          const raw = Buffer.concat(chunks).toString();
          if (response.statusCode >= 300)
            return reject(new Error(`Docker ${response.statusCode}: ${raw.slice(0, 200)}`));
          try {
            resolve(raw ? JSON.parse(raw) : null);
          } catch (e) {
            reject(e);
          }
        });
        response.on('error', reject);
      },
    );
    request.on('timeout', () => request.destroy(new Error('Docker request timed out')));
    request.on('error', reject);
    request.end(body === undefined ? undefined : JSON.stringify(body));
  });
}
export function attach(id) {
  return new Promise((resolve, reject) => {
    const request = http.request({
      socketPath,
      method: 'POST',
      path: `/v1.51/containers/${id}/attach?stream=1&stdin=1&stdout=1&stderr=1`,
      headers: { Connection: 'Upgrade', Upgrade: 'tcp' },
      timeout: 15000,
    });
    request.on('upgrade', (_response, socket, head) => {
      if (head.length) socket.unshift(head);
      resolve(socket);
    });
    request.on('response', (response) => {
      response.resume();
      reject(new Error(`Attach ${response.statusCode}`));
    });
    request.on('timeout', () => request.destroy(new Error('Attach timed out')));
    request.on('error', reject);
    request.end();
  });
}
