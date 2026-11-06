import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  DEFAULT_INVITE_DAYS, checkHeading, inviteLink, inviteToken, inviteTrail,
} from '../src/lib/invites.ts';

const now = Date.UTC(2026, 10, 5, 12, 0, 0);

function invite(overrides) {
  return {
    id: '1',
    note: null,
    createdBy: 'ada',
    createdAt: now,
    expiresAt: Date.UTC(2026, 10, 12, 12, 0, 0),
    usedAt: null,
    usedBy: null,
    revokedAt: null,
    state: 'active',
    ...overrides,
  };
}

test('inviteToken only reads a non-blank invite on /signup', () => {
  assert.equal(inviteToken(new URL('https://mewo.example/signup?invite=abc')), 'abc');
  assert.equal(inviteToken(new URL('https://mewo.example/signup?invite=abc+def')), 'abc def');
  assert.equal(inviteToken(new URL('https://mewo.example/signup?invite=a%2Fb')), 'a/b');
  assert.equal(inviteToken(new URL('https://mewo.example/signup?invite=%20')), null);
  assert.equal(inviteToken(new URL('https://mewo.example/signup?invite=')), null);
  assert.equal(inviteToken(new URL('https://mewo.example/signup')), null);
  assert.equal(inviteToken(new URL('https://mewo.example/?invite=abc')), null);
  assert.equal(inviteToken(new URL('https://mewo.example/signup/extra?invite=abc')), null);
});

test('inviteLink encodes the token onto this origin', () => {
  assert.equal(DEFAULT_INVITE_DAYS, 7);
  const token = 'a/b c';
  const link = inviteLink('https://mewo.example', token);
  assert.equal(link, 'https://mewo.example/signup?invite=a%2Fb%20c');
  assert.equal(inviteToken(new URL(link)), token);
});

test('checkHeading names the three dead states', () => {
  assert.equal(checkHeading('used'), 'Invite already used');
  assert.equal(checkHeading('expired'), 'Invite expired');
  assert.equal(checkHeading('invalid'), 'Invite not valid');
});

test('inviteTrail is one line per state and uses UTC dates', () => {
  assert.equal(inviteTrail(invite(), now), 'Expires Nov 12');
  assert.equal(inviteTrail(invite({ expiresAt: now }), now), 'Expired');
  assert.equal(inviteTrail(invite({ expiresAt: now - 1 }), now), 'Expired');
  assert.equal(inviteTrail(invite({ state: 'expired' }), now), 'Expired');
  assert.equal(inviteTrail(invite({ state: 'revoked' }), now), 'Revoked');
  assert.equal(inviteTrail(invite({ state: 'used', usedBy: 'ada' }), now), '@ada');
  assert.equal(inviteTrail(invite({ state: 'used', usedBy: null }), now), 'Used');
});
