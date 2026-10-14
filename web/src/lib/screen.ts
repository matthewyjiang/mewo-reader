import { chrome } from './chrome.svelte';
import { router } from './router.svelte';
import type { BookRecord, FeedPost } from './models';

export interface FeedScreenProps {
  scroller: HTMLElement;
  oncomment: (book: BookRecord, post: FeedPost) => void;
}

/** Restore only after the destination has rendered its rows. */
export function restoreScroll(scroller: HTMLElement, key = router.current.key): void {
  scroller.scrollTop = router.scroll.get(key) ?? 0;
  chrome.reset(scroller.scrollTop);
  scroller.focus({ preventScroll: true });
}
