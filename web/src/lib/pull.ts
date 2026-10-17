/** Distance the finger travels before a pull counts as a refresh. */
export const PULL_THRESHOLD = 56;
const PULL_MAX = 80;

/**
 * Rubber-band the pull. Reduced motion skips the drag and arms at the threshold,
 * so the gesture still refreshes without a moving sheet.
 */
export function pullOffset(dy: number, reducedMotion: boolean): number {
  if (dy <= 0) return 0;
  if (reducedMotion) return dy >= PULL_THRESHOLD ? PULL_THRESHOLD : 0;
  return Math.min(PULL_MAX, dy * 0.45);
}

export function shouldRefresh(offset: number): boolean {
  return offset >= PULL_THRESHOLD;
}
