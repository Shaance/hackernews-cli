import { AwsClient } from 'aws4fetch';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
const file = process.argv[2];
if (!file) throw Error('Usage: node publish.mjs SNAPSHOT');
const {
  R2_ACCOUNT_ID: account,
  R2_BUCKET: bucket,
  R2_ACCESS_KEY_ID: accessKeyId,
  R2_SECRET_ACCESS_KEY: secretAccessKey,
  PREVIEW_CHANNEL: channel,
} = process.env;
if (!account || !bucket || !accessKeyId || !secretAccessKey || !['dev', 'prod'].includes(channel))
  throw Error('Configure R2 credentials, bucket and PREVIEW_CHANNEL=dev|prod');
execFileSync(process.env.HN_BIN || 'hn', ['--validate-snapshot', file], { stdio: 'inherit' });
const bytes = await readFile(file),
  hash = createHash('sha256').update(bytes).digest('hex');
const snapshot = JSON.parse(bytes),
  key = `${channel}/bundles/${hash}.json`;
// The prompt asks for 60–90 words. Reject obvious fragments before any R2 write.
for (const [id, article] of Object.entries(snapshot.articles)) {
  if (article.summary != null && (article.summary.match(/\S+/g) || []).length < 40)
    throw Error(`Summary for ${id} is too short (minimum 40 words); published snapshot unchanged`);
}
const manifest = { version: 1, generated_at: snapshot.generated_at, key, sha256: hash };
const client = new AwsClient({
  accessKeyId,
  secretAccessKey,
  service: 's3',
  region: 'auto',
  retries: 0,
});
const base = `https://${account}.r2.cloudflarestorage.com/${bucket}/`;
let response = await client.fetch(base + key, {
  method: 'PUT',
  headers: { 'Content-Type': 'application/json' },
  body: bytes,
  signal: AbortSignal.timeout(60000),
});
if (!response.ok) throw Error(`Bundle upload failed: HTTP ${response.status}`);
response = await client.fetch(base + key, { signal: AbortSignal.timeout(60000) });
if (
  !response.ok ||
  createHash('sha256')
    .update(Buffer.from(await response.arrayBuffer()))
    .digest('hex') !== hash
)
  throw Error('Uploaded bundle readback failed; latest pointer unchanged');
response = await client.fetch(base + channel + '/latest.json', {
  method: 'PUT',
  headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' },
  body: JSON.stringify(manifest),
  signal: AbortSignal.timeout(30000),
});
if (!response.ok) throw Error(`Latest pointer upload failed: HTTP ${response.status}`);
response = await client.fetch(base + channel + '/latest.json', {
  signal: AbortSignal.timeout(30000),
});
if (!response.ok || (await response.json()).sha256 !== hash)
  throw Error('Latest pointer readback failed');
console.log(JSON.stringify({ published: hash, key, generated_at: snapshot.generated_at }));
