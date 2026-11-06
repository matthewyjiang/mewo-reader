import type { AdminInvite } from './models';

export type InviteState = 'active' | 'used' | 'expired' | 'revoked';
export type CheckStatus = 'valid' | 'used' | 'expired' | 'invalid';

export const DEFAULT_INVITE_DAYS = 7;

const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** The signup token, only when this URL is the signup page and the query is non-blank. */
export function inviteToken(url: URL): string | null {
  if (url.pathname !== '/signup') return null;
  const token = url.searchParams.get('invite')?.trim() ?? '';
  return token.length > 0 ? token : null;
}

/** A link on this origin. The token is encoded so it survives the query string. */
export function inviteLink(origin: string, token: string): string {
  return `${origin}/signup?invite=${encodeURIComponent(token)}`;
}

export function checkHeading(status: Exclude<CheckStatus, 'valid'>): string {
  if (status === 'used') return 'Invite already used';
  if (status === 'expired') return 'Invite expired';
  return 'Invite not valid';
}

/** One trail per row. A deleted account has no username, so a used invite reads "Used". */
export function inviteTrail(invite: AdminInvite, now: number): string {
  if (invite.state === 'used') return invite.usedBy ? `@${invite.usedBy}` : 'Used';
  if (invite.state === 'revoked') return 'Revoked';
  if (invite.state === 'expired' || invite.expiresAt <= now) return 'Expired';
  const date = new Date(invite.expiresAt);
  return `Expires ${months[date.getUTCMonth()]} ${date.getUTCDate()}`;
}
