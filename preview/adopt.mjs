import { AwsClient } from 'aws4fetch';
import { readFile, writeFile, rename, rm, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { get } from 'node:http';
const {
  R2_ACCOUNT_ID: account,
  R2_BUCKET: bucket,
  R2_ACCESS_KEY_ID: accessKeyId,
  R2_SECRET_ACCESS_KEY: secretAccessKey,
  PREVIEW_CHANNEL: channel,
  SNAPSHOT,
} = process.env;
if (
  !account ||
  !bucket ||
  !accessKeyId ||
  !secretAccessKey ||
  !SNAPSHOT ||
  !['dev', 'prod'].includes(channel)
)
  throw Error('Configure read-only R2 credentials, channel and SNAPSHOT');
const target = resolve(SNAPSHOT),
  pending = target + '.pending',
  previous = target + '.previous';
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const client = new AwsClient({
  accessKeyId,
  secretAccessKey,
  service: 's3',
  region: 'auto',
  retries: 0,
});
const base = `https://${account}.r2.cloudflarestorage.com/${bucket}/`;
const response = await client.fetch(base + channel + '/latest.json', {
  signal: AbortSignal.timeout(30000),
});
if (!response.ok) throw Error(`Manifest unavailable: HTTP ${response.status}`);
const manifest = await response.json();
if (
  manifest.version !== 1 ||
  !/^[a-f0-9]{64}$/.test(manifest.sha256) ||
  manifest.key !== `${channel}/bundles/${manifest.sha256}.json`
)
  throw Error('Invalid manifest');
let old;
try {
  old = await readFile(target);
} catch (e) {
  if (e.code !== 'ENOENT') throw e;
}
if (old && digest(old) === manifest.sha256) {
  console.log('Snapshot unchanged');
  process.exit(0);
}
const bundle = await client.fetch(base + manifest.key, { signal: AbortSignal.timeout(60000) });
if (!bundle.ok) throw Error(`Bundle unavailable: HTTP ${bundle.status}`);
// Bound streamed storage input before parsing or handing it to the native validator.
let size = 0;
const chunks = [];
for await (const chunk of bundle.body) {
  size += chunk.length;
  if (size > 16 * 1024 * 1024) throw Error('Snapshot exceeds 16 MiB');
  chunks.push(chunk);
}
const bytes = Buffer.concat(chunks);
if (digest(bytes) !== manifest.sha256) throw Error('Bundle digest mismatch');
await mkdir(dirname(target), { recursive: true });
try {
  await writeFile(pending, bytes, { mode: 0o644 });
  execFileSync(process.env.HN_BIN || 'hn', ['--validate-snapshot', pending], { stdio: 'inherit' });
  if (old) await writeFile(previous, old, { mode: 0o644 });
  await rename(pending, target);
  try {
    execFileSync('systemctl', ['--user', 'restart', 'hn-preview.service'], { stdio: 'inherit' });
    const url = `http://${process.env.PREVIEW_BIND || '127.0.0.1'}:${process.env.PORT || 8765}/healthz`;
    let ready = false,
      failure = 'No response';
    for (let attempt = 0; attempt < 10; attempt++) {
      try {
        // fetch ignores a custom Host; the gateway binds an IP and checks its public hostname.
        const health = await new Promise((resolve, reject) =>
          get(
            url,
            {
              headers: { Host: process.env.PREVIEW_HOST || '127.0.0.1:8765' },
              signal: AbortSignal.timeout(1000),
            },
            resolve,
          ).on('error', reject),
        );
        let body = '';
        for await (const chunk of health) body += chunk;
        if (health.statusCode !== 200) throw Error(`Health check HTTP ${health.statusCode}`);
        ready = JSON.parse(body).snapshot === manifest.sha256;
        if (!ready) failure = 'Gateway snapshot digest differs from published version';
      } catch (error) {
        failure = error.message;
      }
      if (ready) break;
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
    if (!ready) throw Error(`Gateway did not adopt the published version: ${failure}`);
  } catch (error) {
    if (old) {
      await writeFile(pending, old, { mode: 0o644 });
      await rename(pending, target);
      execFileSync('systemctl', ['--user', 'restart', 'hn-preview.service'], { stdio: 'inherit' });
    }
    throw error;
  }
  console.log(JSON.stringify({ adopted: manifest.sha256, generated_at: manifest.generated_at }));
} finally {
  await rm(pending, { force: true });
}
