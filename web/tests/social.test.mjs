import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commitRepost, hasReposted, repostCount } from '../src/lib/repost.ts';
import { SocialGate, loadSocialIndex, startRepost } from '../src/lib/social.ts';

const index = (reposts, reposted) => ({ counts: {}, mine: [], reposts, reposted });

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

test('an in-flight index load cannot drop a repost the server accepted', async () => {
  const gate = new SocialGate();
  let current = index({}, []);
  let releaseLoad;
  const loadGate = new Promise((resolve) => { releaseLoad = resolve; });
  const load = loadSocialIndex(gate, async () => {
    await loadGate;
    return index({}, []);
  }).then((loaded) => {
    if (loaded) current = loaded;
    return loaded;
  });

  await flush();
  let releaseServer;
  const server = new Promise((resolve) => { releaseServer = resolve; });
  const repost = startRepost(gate, () => commitRepost(
    current,
    '1',
    async () => {
      await server;
      return { reposts: 1, reposted: true };
    },
    () => current,
    (next) => { current = next; },
  ));
  await flush();
  assert.equal(hasReposted(current, '1'), true);

  releaseLoad();
  assert.equal(await load, null);
  assert.equal(hasReposted(current, '1'), true);

  releaseServer();
  await repost;
  assert.deepEqual(current.reposted, ['1']);
  assert.equal(repostCount(current, '1'), 1);
});
