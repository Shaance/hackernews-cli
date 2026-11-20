import { AwsClient } from 'aws4fetch';
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import * as fs from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import {
  bootstrap,
  command,
  descriptor,
  imageRef,
  readLocalHealth,
  readRollout,
  registryDigest,
  releaseId,
  ReleaseError,
  SignatureMismatch,
  verifyRelease,
} from './release.mjs';

const releases = join(homedir(), 'releases');
const crane = process.env.CRANE || '/opt/hn-preview/tools/crane';
const folder = (id) => id.replace(':', '-');
const releasePath = (id) => join(releases, folder(id));
const log = (decision, fields) => console.log(JSON.stringify({ decision, ...fields }));

async function storage(key, options = {}, timeout = 30_000) {
  // readRollout checks these credentials before any caller reaches this boundary.
  const { R2_ACCOUNT_ID, R2_BUCKET, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY } = process.env;
  const client = new AwsClient({
    accessKeyId: R2_ACCESS_KEY_ID,
    secretAccessKey: R2_SECRET_ACCESS_KEY,
    service: 's3',
    region: 'auto',
    retries: 0,
  });
  const base = `https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com/${R2_BUCKET}`;
  const response = await client
    .fetch(base + '/' + key, { ...options, signal: AbortSignal.timeout(timeout) })
    .catch(() => {
      throw new ReleaseError('Publisher storage unavailable');
    });
  if (!response.ok) throw new ReleaseError(`Publisher storage HTTP ${response.status}`);
  return response;
}

async function saveRollout(record) {
  await storage('releases/rollout.json', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' },
    body: JSON.stringify(record),
  });
  if (!isDeepStrictEqual(await readRollout(), record))
    throw new ReleaseError('Rollout readback differs');
}

async function current() {
  try {
    return resolve(releases, await fs.readlink(join(releases, 'current')));
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    return null;
  }
}

async function installed(id) {
  try {
    return (await fs.stat(releasePath(id))).isDirectory();
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    return false;
  }
}

async function extract(id) {
  const target = releasePath(id);
  const staging = join(releases, '.stage');
  await fs.rm(staging, { recursive: true, force: true });
  await fs.mkdir(staging, { mode: 0o700 });
  try {
    await command(
      '/bin/bash',
      [
        '-o',
        'pipefail',
        '-c',
        '"$1" export "$2" - | tar -x -C "$3" opt/release usr/local/bin/hn',
        'extract-release',
        crane,
        imageRef(id),
        staging,
      ],
      240_000,
    ).catch(() => {
      throw new ReleaseError('Writer release extraction failed');
    });
    const extracted = join(staging, 'opt/release');
    await fs.rename(join(staging, 'usr/local/bin/hn'), join(extracted, 'hn'));
    await fs.access(join(extracted, 'hn'), constants.X_OK);
    await fs.access(join(extracted, 'publish.mjs'), constants.R_OK);
    await fs.rename(extracted, target);
  } finally {
    await fs.rm(staging, { recursive: true, force: true });
  }
  return target;
}

async function setLink(name, target) {
  const link = join(releases, name);
  const pending = link + '.pending';
  await fs.rm(pending, { force: true });
  await fs.symlink(target, pending);
  await fs.rename(pending, link);
}

async function activate(good) {
  const target = releasePath(good.digest);
  const old = await current();
  if (old !== target) {
    if (old) await setLink('previous', old);
    await setLink('current', folder(good.digest));
  }
  return old !== target;
}

async function reconcile(good) {
  if (!(await installed(good.digest))) {
    if (good.digest === bootstrap) throw new ReleaseError('Writer bootstrap is missing');
    await verifyRelease(good);
    await extract(good.digest);
  }
  const changed = await activate(good);
  const previous = await fs.readlink(join(releases, 'previous')).catch((error) => {
    if (error.code !== 'ENOENT') throw error;
    return '';
  });
  const keep = new Set([releasePath(good.digest), await current(), resolve(releases, previous)]);
  for (const entry of await fs.readdir(releases, { withFileTypes: true })) {
    const path = join(releases, entry.name);
    if (
      entry.isDirectory() &&
      releaseId(entry.name.replace(/^sha256-/, 'sha256:')) &&
      !keep.has(path)
    )
      await fs.rm(path, { recursive: true });
  }
  return changed;
}

class SnapshotRejected extends ReleaseError {}

async function validateLiveSnapshot(target) {
  const manifest = await (await storage('dev/latest.json')).json();
  if (
    manifest?.version !== 1 ||
    !/^[a-f0-9]{64}$/.test(manifest.sha256) ||
    manifest.key !== `dev/bundles/${manifest.sha256}.json`
  )
    throw new ReleaseError('Invalid live snapshot manifest');
  const bundle = await storage(manifest.key, {}, 60_000);
  const chunks = [];
  let size = 0;
  for await (const chunk of bundle.body) {
    size += chunk.length;
    if (size > 16 * 1024 * 1024) throw new ReleaseError('Live snapshot exceeds 16 MiB');
    chunks.push(chunk);
  }
  const bytes = Buffer.concat(chunks);
  if (createHash('sha256').update(bytes).digest('hex') !== manifest.sha256)
    throw new ReleaseError('Live snapshot digest differs');

  const scratch = await fs.mkdtemp(join(tmpdir(), 'hn-writer-'));
  try {
    const snapshot = join(scratch, 'snapshot.json');
    await fs.writeFile(snapshot, bytes, { mode: 0o600 });
    await command(join(target, 'hn'), ['--validate-snapshot', snapshot], 60_000).catch((error) => {
      if (error.killed || (typeof error.code !== 'number' && !error.signal))
        throw new ReleaseError('Writer snapshot validation could not run');
      throw new SnapshotRejected('Writer rejected the live snapshot');
    });
  } finally {
    await fs.rm(scratch, { recursive: true, force: true });
  }
}

async function prodHealth() {
  const response = await fetch('https://hn-session.hashcode.dev/healthz', {
    signal: AbortSignal.timeout(2000),
  });
  return response.ok ? response.json() : null;
}

async function nextRelease(record) {
  const { stdout } = await command(
    crane,
    ['digest', 'ghcr.io/shaance/hn-preview:main'],
    30_000,
  ).catch(() => {
    throw new ReleaseError('Could not resolve the main image');
  });
  const digest = stdout.trim();
  if (!registryDigest.test(digest)) throw new ReleaseError('Invalid main image digest');
  if (digest === record.last_abort?.digest) return null;
  const signatures = await verifyRelease({ digest });
  // Only use annotations attached to the signature for this manifest.
  const candidates = signatures
    .filter(({ critical }) => critical?.image?.['docker-manifest-digest'] === digest)
    .map(({ optional }) => ({
      digest,
      commit: optional?.commit,
      commit_count: optional?.commit_count,
    }))
    .filter(
      (entry) =>
        typeof entry.commit === 'string' &&
        typeof entry.commit_count === 'string' &&
        descriptor(entry),
    );
  if (!candidates.length) throw new ReleaseError('Missing signed release annotations');
  return candidates.find((entry) => BigInt(entry.commit_count) > BigInt(record.good.commit_count));
}

async function decide(record, dev, prod, check) {
  const { good, candidate, stage, last_abort } = record;
  const onRelease = (health, digest) => health?.running === digest && health.committed === digest;
  if (stage === 'idle') {
    if (!onRelease(dev, good.digest) || !onRelease(prod, good.digest))
      return { decision: 'waiting' };
    const next = await nextRelease(record);
    return next
      ? { decision: 'admit', changes: { candidate: next, stage: 'dev' } }
      : { decision: 'waiting' };
  }

  const abort = (reason) => ({
    decision: 'abort',
    changes: { candidate: null, stage: 'idle', last_abort: { digest: candidate.digest, reason } },
  });
  if (dev?.failed === candidate.digest) return abort('Development reader rejected the candidate');
  if (prod?.failed === candidate.digest) return abort('Production reader rejected the candidate');

  const onCandidate = (health) => onRelease(health, candidate.digest);
  if (stage === 'dev' && prod && onCandidate(dev))
    return { decision: 'advance', changes: { stage: 'prod' } };
  if (stage === 'prod' && onCandidate(prod))
    return { decision: 'advance', changes: { stage: 'writer' } };
  if (stage !== 'writer' || !onCandidate(dev) || !onCandidate(prod)) return { decision: 'waiting' };
  try {
    await verifyRelease(candidate);
    if (!check) {
      const target = (await installed(candidate.digest))
        ? releasePath(candidate.digest)
        : await extract(candidate.digest);
      await validateLiveSnapshot(target);
    }
  } catch (error) {
    if (error instanceof SignatureMismatch || error instanceof SnapshotRejected)
      return abort(error.message);
    throw error;
  }
  if (check) return { decision: 'validate-writer' };
  return {
    decision: 'promote',
    changes: {
      good: candidate,
      candidate: null,
      stage: 'idle',
      last_abort: last_abort?.digest === candidate.digest ? null : last_abort,
    },
  };
}

async function rollout() {
  const args = process.argv.slice(2);
  const check = args[0] === '--check';
  const reconcileOnly = args[0] === '--reconcile-writer';
  if (args.length && (args.length !== 1 || (!check && !reconcileOnly)))
    throw new ReleaseError('Use --check or --reconcile-writer');
  const record = await readRollout();
  const changed = check
    ? (await current()) !== releasePath(record.good.digest)
    : await reconcile(record.good);
  if (reconcileOnly)
    return log(changed ? 'reconcile-writer' : 'writer-current', { good: record.good.digest });

  const [dev, prod] = await Promise.all([
    readLocalHealth().catch(() => null),
    prodHealth().catch(() => null),
  ]);
  const { decision, changes } = await decide(record, dev, prod, check);
  const now = new Date().toISOString();
  const next = { ...record, ...changes, since: changes ? now : record.since, heartbeat: now };
  if (!check) {
    await saveRollout(next);
    // The record remains authoritative if the service stops between readback and activation.
    if (decision === 'promote') await activate(next.good);
  }
  log(decision, {
    check,
    stage: next.stage,
    good: next.good.digest,
    candidate: next.candidate?.digest ?? null,
    reconcile_writer: changed,
    ...(decision === 'abort' && { reason: next.last_abort.reason }),
  });
}

try {
  await rollout();
} catch (error) {
  log('error', {
    reason: error instanceof ReleaseError ? error.message : 'Coordinator could not complete',
  });
  process.exitCode = 1;
}
