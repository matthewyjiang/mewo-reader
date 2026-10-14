import { chrome } from './chrome.svelte';
import { router } from './router.svelte';
import type { BookRecord, FeedPost } from './models';

export interface FeedScreenProps {
  scroller: HTMLElement;
  oncomment: (book: BookRecord, post: FeedPost) => void;
}

/** Restore only after the destination has rendered its rows. */
export function restoreScroll(scroller: HTMLElement, key = router.current.key): void {
  const saved = router.scroll.get(key);
  scroller.scrollTop = typeof saved === 'number' ? saved : 0;
  chrome.reset(scroller.scrollTop);
  if (!(router.fromTab && router.url.pathname === '/search')) {
    scroller.focus({ preventScroll: true });
  }
}
