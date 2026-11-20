import * as fs from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import * as release from './release.mjs';
import { ReleaseError, SignatureMismatch } from './release.mjs';
import { checkHealth, checkRelease, SessionCapacityError } from './session.mjs';

const lib = join(homedir(), '.local/lib/hn-preview');
const state = join(homedir(), '.local/state/hn-preview');
const role = process.env.PREVIEW_ROLE;
const snapshot = process.env.SNAPSHOT;
const check = process.argv[2] === '--check';
const currentLink = join(lib, 'current');
const folder = (id) => id.replace(':', '-');
const fromFolder = (name) => name.replace(/^sha256-/, 'sha256:');
const releasePath = (id) => join(lib, folder(id));
const path = (name) => join(state, name + '.json');
const log = (decision, fields = {}) => console.log(JSON.stringify({ decision, ...fields }));
const reasonOf = (error, fallback) => (error instanceof ReleaseError ? error.message : fallback);
class RecoveryPending extends ReleaseError {}

async function optional(operation) {
  try {
    return await operation;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    return null;
  }
}

const read = async (name) => JSON.parse(await optional(fs.readFile(path(name), 'utf8')));
async function save(name, value) {
  await fs.writeFile(path(name) + '.tmp', JSON.stringify(value) + '\n', { mode: 0o600 });
  await fs.rename(path(name) + '.tmp', path(name));
}

const desired = (record) =>
  record.stage === 'idle' || (role === 'prod' && record.stage === 'dev')
    ? record.good
    : record.candidate;
const retainedFailure = (record, status) =>
  status?.failed && (!record.candidate || record.candidate.digest === status.failed);

async function docker(args, timeout = 120_000) {
  try {
    return (await release.command(process.env.DOCKER || 'docker', args, timeout)).stdout.trim();
  } catch {
    throw new ReleaseError(`Docker ${args[0]} failed`);
  }
}

async function localImage(id) {
  const env = await optional(fs.readFile(join(releasePath(id), 'release.env'), 'utf8'));
  if (env === null) return null;
  const image = /^PREVIEW_IMAGE=(sha256:[a-f0-9]{64})$/m.exec(env)?.[1];
  if (!image || !env.split(/\r?\n/).includes(`RELEASE=${id}`))
    throw new ReleaseError('Invalid staged release environment');
  return image;
}

async function imagePresent(image) {
  try {
    const result = await release.command(
      process.env.DOCKER || 'docker',
      ['image', 'inspect', '--format', '{{.Id}}', image],
      120_000,
    );
    if (result.stdout.trim() !== image) throw new ReleaseError('Local image ID differs');
    return true;
  } catch (error) {
    if (/no such image/i.test(error.stderr ?? '')) return false;
    throw new ReleaseError('Docker image inspect failed');
  }
}

async function stage(descriptor) {
  const id = descriptor.digest;
  const existing = await localImage(id);
  if (existing && (await imagePresent(existing))) return;
  if (id === release.bootstrap) {
    throw new ReleaseError('Bootstrap release or image is missing');
  }
  await release.verifyRelease(descriptor);
  const ref = release.imageRef(id);
  await docker(['pull', ref]);
  const image = await docker(['image', 'inspect', '--format', '{{.Id}}', ref]);
  if (!release.registryDigest.test(image)) throw new ReleaseError('Invalid local image ID');
  if (existing === image) return;
  if (existing) throw new ReleaseError('Staged release image differs from the pulled image');
  const scratch = await fs.mkdtemp(join(lib, '.stage-'));
  try {
    const container = await docker(['create', image]);
    try {
      await docker(['cp', `${container}:/opt/release/.`, scratch]);
      await docker(['cp', `${container}:/usr/local/bin/hn`, join(scratch, 'hn')]);
      await fs.writeFile(join(scratch, 'release.env'), `PREVIEW_IMAGE=${image}\nRELEASE=${id}\n`, {
        mode: 0o600,
      });
    } finally {
      await docker(['rm', container]);
    }
    await fs.rename(scratch, releasePath(id));
  } finally {
    await fs.rm(scratch, { recursive: true, force: true });
  }
}

async function current() {
  const link = resolve(lib, await fs.readlink(currentLink));
  const id = fromFolder(basename(link));
  if (!release.releaseId(id) || link !== releasePath(id))
    throw new ReleaseError('Invalid current release link');
  return id;
}

async function activate(id) {
  const old = await current();
  if (old !== id) {
    await save('previous', old);
    const pending = currentLink + '.pending';
    await fs.rm(pending, { force: true });
    await fs.symlink(folder(id), pending);
    await fs.rename(pending, currentLink);
  }
  try {
    await release.command(
      process.env.SYSTEMCTL || 'systemctl',
      ['--user', 'restart', 'hn-preview.service'],
      60_000,
    );
  } catch {
    throw new ReleaseError('Gateway restart failed');
  }
}

async function prune(good) {
  const keep = new Set([good, await current(), await read('previous')]);
  const releases = (await fs.readdir(lib, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory() && release.releaseId(fromFolder(entry.name)))
    .map((entry) => fromFolder(entry.name));
  const stale = releases.filter((id) => !keep.has(id));
  if (!stale.length) return;
  const protectedImages = new Set(
    await Promise.all(releases.filter((id) => keep.has(id)).map(localImage)),
  );
  const images = new Set((await docker(['image', 'ls', '-q', '--no-trunc'])).split('\n'));
  for (const id of stale) {
    const image = await localImage(id);
    if (image && images.has(image) && !protectedImages.has(image)) {
      await docker(['image', 'rm', '--no-prune', image]);
      images.delete(image);
    }
    await fs.rm(releasePath(id), { recursive: true });
  }
}

const busy = async () => Boolean(await docker(['ps', '-q', '--filter', 'label=hn-preview=local']));

async function attemptRelease(id, restart) {
  try {
    if (restart) await activate(id);
    await checkRelease(id, await localImage(id), snapshot, docker);
    return null;
  } catch (error) {
    return error instanceof ReleaseError ? error : new ReleaseError('Release checks failed');
  }
}

async function update() {
  if (!['dev', 'prod'].includes(role) || !snapshot || process.argv.length !== (check ? 3 : 2))
    throw new ReleaseError('Configure PREVIEW_ROLE and SNAPSHOT; optional argument: --check');
  const tools = (await fs.readFile(new URL('./VERSION', import.meta.url), 'utf8')).trim();
  const attempt = await read('attempt');
  if (attempt && (!release.releaseId(attempt.target) || !release.releaseId(attempt.known_good)))
    throw new ReleaseError('Invalid interrupted attempt');
  let record;
  try {
    record = await release.readRollout();
  } catch (error) {
    if (!attempt || check) throw error;
    try {
      await checkHealth(attempt.target, snapshot);
      log('recovery-pending', { target: attempt.target, reason: 'Rollout unavailable' });
    } catch {
      const status = await read('committed');
      const rollback = await attemptRelease(attempt.known_good, true);
      const reason =
        'Rollout unavailable' + (rollback ? '; known_good also failed: ' + rollback.message : '');
      await save('committed', {
        ...status,
        committed: rollback ? null : attempt.known_good,
        failed: status?.failed === attempt.known_good ? null : (status?.failed ?? null),
        reason: status?.reason ? status.reason + '; ' + reason : reason,
        tools,
      });
      log('recovery-pending', {
        target: attempt.target,
        committed: rollback ? null : attempt.known_good,
        reason,
      });
    }
    process.exitCode = 1;
    return;
  }
  const target = desired(record).digest;
  const installed = await current();
  const status = await read('committed');
  const deferred = await read('deferred');
  const running =
    target === installed || check
      ? ((await release.readLocalHealth().catch(() => null))?.running ?? null)
      : null;
  const failed =
    retainedFailure(record, status) && status.failed !== record.good.digest ? status.failed : null;
  let action = 'check-current';
  if (attempt) action = 'recover';
  else if (failed === target && target !== record.good.digest) action = 'skip-failed';
  else if (target !== installed) action = 'stage';
  else if (running === installed && status?.committed === installed) action = 'no-op';
  const writeStatus = (rollout, committed, failed = null, reason = null) =>
    save('committed', { committed, failed, reason, tools, rollout });
  const deferring = () => deferred?.target !== target || Date.now() - deferred.since <= 7_200_000;
  if (check) {
    const occupied = action === 'stage' && (await busy());
    log(occupied && deferring() ? 'deferred' : action, {
      check: true,
      target,
      current: installed,
      running,
      committed: status?.committed ?? null,
      known_good: record.good.digest,
      attempt: attempt && { target: attempt.target, known_good: attempt.known_good },
      deferred: deferred && { target: deferred.target, since: deferred.since },
      busy: occupied,
    });
    return;
  }
  if (action === 'skip-failed') return log(action, { target });
  if (action === 'no-op') {
    await writeStatus(record, target, failed, failed ? status.reason : null);
    if (deferred) await fs.rm(path('deferred'), { force: true });
    await prune(record.good.digest);
    return log(action, { target });
  }
  if (attempt) {
    await stage(record.good);
    let reason = null;
    if (attempt.target !== record.good.digest) {
      if (status?.failed === attempt.target)
        reason = new ReleaseError(status.reason || 'Interrupted rollback already recorded');
      else if (status?.committed === null)
        reason = new RecoveryPending(status.reason || 'Interrupted rollback is unverified');
    }
    const superseded =
      attempt.target !== target
        ? new ReleaseError('Interrupted target is no longer desired')
        : null;
    return finish(record, attempt.target, true, reason || superseded);
  }

  if (action === 'stage') {
    try {
      await stage(desired(record));
    } catch (error) {
      if (!(error instanceof SignatureMismatch)) throw error;
      return rejectTarget(record, error.message);
    }
    if ((await busy()) && deferring()) {
      if (deferred?.target !== target) await save('deferred', { target, since: Date.now() });
      return log('deferred', { target });
    }
    if (target !== record.good.digest) await stage(record.good);
    try {
      await fs.access(snapshot);
      await release.command(
        join(releasePath(target), 'hn'),
        ['--validate-snapshot', snapshot],
        60_000,
      );
    } catch (error) {
      if (error.killed || (typeof error.code !== 'number' && !error.signal))
        throw new ReleaseError('Snapshot validation could not run');
      return rejectTarget(record, 'Release rejected the live snapshot');
    }
  }

  const latest = await release.readRollout();
  const next = desired(latest),
    original = desired(record);
  if (
    next.digest !== target ||
    next.commit !== original.commit ||
    String(next.commit_count) !== String(original.commit_count) ||
    !(await localImage(latest.good.digest))
  )
    return log('record-changed', { target, desired: next.digest });
  await save('attempt', { target, known_good: latest.good.digest });
  return finish(latest, target, action === 'stage' || running !== installed, null);

  async function rejectTarget(rollout, reason) {
    const rejected = target === rollout.good.digest ? failed : target;
    await writeStatus(rollout, status?.committed ?? null, rejected, reason);
    log(target === rollout.good.digest ? 'check-failed' : 'failed', { target, reason });
    process.exitCode = 1;
  }

  async function finish(rollout, id, restart, failure) {
    const good = rollout.good.digest;
    failure ||= await attemptRelease(id, restart);
    const transient = failure instanceof SessionCapacityError || failure instanceof RecoveryPending;
    const rollbackNeeded = failure && id !== good && (!transient || restart);
    const rollback = rollbackNeeded ? await attemptRelease(good, true) : null;
    let committed = id;
    if (failure) {
      if (id === good || rollback) committed = null;
      else if (rollbackNeeded) committed = good;
      else committed = status?.committed ?? null;
    }
    const previousFailure =
      retainedFailure(rollout, status) && status.failed !== good ? status.failed : null;
    const rejected = failure && !transient && id !== good ? id : previousFailure;
    let reason = failure?.message ?? (rejected ? status.reason : null);
    if (rollback) reason += '; known_good also failed: ' + rollback.message;
    await writeStatus(rollout, committed, rejected, reason);
    if (!failure || (id !== good && !rollback)) await fs.rm(path('attempt'));
    await fs.rm(path('deferred'), { force: true });
    if (!failure || (id !== good && !rollback)) await prune(good);
    const decision = !failure
      ? 'committed'
      : rollback
        ? 'rollback-failed'
        : rollbackNeeded
          ? 'rolled-back'
          : 'check-failed';
    log(decision, { target: id, committed, reason });
    if (failure) process.exitCode = 1;
  }
}

try {
  await update();
} catch (error) {
  log('error', { reason: reasonOf(error, 'Updater could not complete') });
  process.exitCode = 1;
}
