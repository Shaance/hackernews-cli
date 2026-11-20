import { AwsClient } from 'aws4fetch';
import { promisify } from 'node:util';
import { execFile } from 'node:child_process';
import { get } from 'node:http';

export const registryDigest = /^sha256:[a-f0-9]{64}$/;
export const bootstrap = 'bootstrap-db03844';
export const releaseId = (id) =>
  typeof id === 'string' && (id === bootstrap || registryDigest.test(id));
export const imageRef = (digest) => 'ghcr.io/shaance/hn-preview@' + digest;
export class ReleaseError extends Error {}
export class SignatureMismatch extends ReleaseError {}

const exec = promisify(execFile);
const { R2_ACCOUNT_ID, R2_BUCKET, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, ...env } = process.env;

export const command = (file, args, timeout) => exec(file, args, { env, timeout });
const descriptor = (entry) =>
  releaseId(entry?.digest) &&
  /^[a-f0-9]{40}$/.test(entry.commit) &&
  /^[1-9]\d*$/.test(entry.commit_count);
export { descriptor };

export async function readRollout() {
  if (!R2_ACCOUNT_ID || !R2_BUCKET || !R2_ACCESS_KEY_ID || !R2_SECRET_ACCESS_KEY)
    throw new ReleaseError('Missing rollout storage configuration');
  const client = new AwsClient({
    accessKeyId: R2_ACCESS_KEY_ID,
    secretAccessKey: R2_SECRET_ACCESS_KEY,
    service: 's3',
    region: 'auto',
    retries: 0,
  });
  const base = `https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com/${R2_BUCKET}`;
  const response = await client
    .fetch(base + '/releases/rollout.json', {
      signal: AbortSignal.timeout(30_000),
    })
    .catch(() => {
      throw new ReleaseError('Rollout unavailable');
    });
  if (!response.ok) throw new ReleaseError(`Rollout read failed (HTTP ${response.status})`);
  const { good, candidate, stage, since, heartbeat, last_abort } = (await response.json()) ?? {};
  if (
    !descriptor(good) ||
    !['idle', 'dev', 'prod', 'writer'].includes(stage) ||
    (stage === 'idle'
      ? candidate !== null
      : !descriptor(candidate) || candidate.digest === bootstrap) ||
    !Number.isFinite(Date.parse(since)) ||
    !Number.isFinite(Date.parse(heartbeat)) ||
    (last_abort !== null &&
      (!releaseId(last_abort?.digest) || typeof last_abort.reason !== 'string'))
  )
    throw new ReleaseError('Invalid rollout record');
  return { good, candidate, stage, since, heartbeat, last_abort };
}

const identity =
  'https://github.com/Shaance/hn/.github/workflows/preview-server.yml@refs/heads/main';
const issuer = 'https://token.actions.githubusercontent.com';
const cosign = env.COSIGN || '/opt/hn-preview/tools/cosign';
const signatureErrors =
  /^(?:none of the expected identities matched|invalid signature|signature verification failed|digest did not match)/i;

export async function verifyRelease({ digest, commit, commit_count }) {
  if (!registryDigest.test(digest))
    throw new ReleaseError('Verification requires a registry digest');
  const args = [
    'verify',
    '--output',
    'json',
    '--certificate-identity',
    identity,
    '--certificate-oidc-issuer',
    issuer,
    imageRef(digest),
  ];
  const { stdout } = await command(cosign, args, 120_000).catch(({ stderr = '' }) => {
    // Inspect the underlying reasons: a verification error can also wrap a failed network call.
    const heading = /^(?:Error: |[^\n]*error during command execution: )?/;
    const lines = stderr.trimEnd().split('\n');
    const first = lines.findLastIndex((line) =>
      /^(?:no matching (?:signatures|attestations):|no signatures found$)/.test(
        line.replace(heading, ''),
      ),
    );
    const [summary, ...causes] = first < 0 ? [] : lines.slice(first);
    const diagnostic = summary?.replace(heading, '');
    const details = diagnostic?.match(/^no matching (?:signatures|attestations): *(.*)$/)?.[1];
    const reasons = [details, ...causes.map((line) => line.trim())].filter(Boolean);
    if (
      diagnostic === 'no signatures found' ||
      (details !== undefined &&
        reasons.length &&
        reasons.every((line) => signatureErrors.test(line)))
    )
      throw new SignatureMismatch('Release signature does not match');
    throw new ReleaseError('Cosign verification unavailable');
  });
  const signatures = JSON.parse(stdout);
  if (!Array.isArray(signatures)) throw new ReleaseError('Unreadable Cosign verification output');
  // Shape of `cosign verify --output json` (cosign v3): lowercase keys.
  const matches = ({ critical, optional }) =>
    critical?.image?.['docker-manifest-digest'] === digest &&
    (commit === undefined || optional?.commit === commit) &&
    (commit_count === undefined || optional?.commit_count === String(commit_count));
  if (!signatures.some(matches))
    throw new SignatureMismatch('Release signature annotations or digest do not match');
  return signatures;
}

export function gatewayAddress() {
  const port = Number(env.PORT || 8765);
  return {
    authority: `${env.PREVIEW_BIND || '127.0.0.1'}:${port}`,
    host: env.PREVIEW_HOST || `127.0.0.1:${port}`,
    origin: env.PREVIEW_ORIGIN || `http://127.0.0.1:${port}`,
  };
}

export async function readLocalHealth({ authority, host } = gatewayAddress()) {
  // node:http preserves Host when connecting to the bind address.
  const url = `http://${authority}/healthz`;
  const options = { headers: { Host: host }, signal: AbortSignal.timeout(2000) };
  const response = await new Promise((resolve, reject) =>
    get(url, options, resolve).on('error', reject),
  );
  if (response.statusCode !== 200) {
    response.resume();
    throw new ReleaseError(`Gateway health HTTP ${response.statusCode}`);
  }
  let body = '';
  for await (const chunk of response) body += chunk;
  return JSON.parse(body);
}
