import type { AccountRemoval } from './models';

function books(n: number): string {
  return n === 1 ? '1 book' : `${n} books`;
}

/** One line per thing a delete removes or keeps. A zero count has no line. */
export function removalLines(r: AccountRemoval): string[] {
  const lines: string[] = [];
  if (r.booksDeleted > 0) {
    lines.push(r.sharedDeleted > 0
      ? `Deletes ${books(r.booksDeleted)}, ${r.sharedDeleted} that others can read.`
      : `Deletes ${books(r.booksDeleted)}.`);
  }
  if (r.booksPassed === 1) lines.push('1 book stays with the person who also has it.');
  if (r.booksPassed > 1) lines.push(`${r.booksPassed} books stay with the people who also have them.`);
  if (r.holdsReleased > 0) lines.push(`Gives back ${books(r.holdsReleased)} someone else added.`);
  if (r.requestsCanceled > 0) {
    lines.push(`Cancels ${r.requestsCanceled} ${r.requestsCanceled === 1 ? 'request' : 'requests'}.`);
  }
  lines.push('Removes the account, its replies, and its reposts.');
  return lines;
}
