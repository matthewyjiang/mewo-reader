/** Tiny history router: real URLs, browser Back, and per-entry feed scroll positions. */
export type Tab = 'home' | 'search' | 'likes';
export interface ReaderAnchor { index: number; offset: number }

interface Entry {
  mewo: true;
  key: string;
  tab: Tab;
  previous: boolean;
}

let sequence = 0;
// Unlike crypto.randomUUID, this also works on a plain-HTTP LAN origin.
const newKey = (): string => `${Date.now()}:${++sequence}`;

function entry(): Entry {
  const saved: unknown = history.state;
  if (saved && typeof saved === 'object'
    && 'mewo' in saved && saved.mewo === true
    && 'key' in saved && typeof saved.key === 'string'
    && 'tab' in saved && (saved.tab === 'home' || saved.tab === 'search' || saved.tab === 'likes')
    && 'previous' in saved && typeof saved.previous === 'boolean') {
    return { mewo: true, key: saved.key, tab: saved.tab, previous: saved.previous };
  }
  const tab: Tab = location.pathname === '/search'
    ? 'search' : location.pathname === '/likes' ? 'likes' : 'home';
  const value: Entry = { mewo: true, key: newKey(), tab, previous: false };
  history.replaceState(value, '');
  return value;
}

class Router {
  url = $state(new URL(location.href));
  current = $state<Entry>(entry());
  scroll = new Map<string, number | ReaderAnchor>();
  tabScroll = new Map<Tab, number>();
  fromTab = $state(false);
  beforeChange: (() => void) | undefined;

  constructor() {
    history.scrollRestoration = 'manual';
    window.addEventListener('popstate', () => {
      this.beforeChange?.();
      this.fromTab = false;
      this.current = entry();
      this.url = new URL(location.href);
    });
  }

  go(path: string, replace = false, fromTab = false): void {
    this.beforeChange?.();
    const url = new URL(path, location.origin);
    const tab: Tab = url.pathname === '/search' ? 'search'
      : url.pathname === '/likes' ? 'likes'
        : url.pathname === '/' ? 'home' : this.current.tab;
    const next: Entry = {
      mewo: true, key: newKey(), tab, previous: !replace || this.current.previous,
    };
    if (fromTab) this.scroll.set(next.key, this.tabScroll.get(tab) ?? 0);
    if (replace) history.replaceState(next, '', url);
    else history.pushState(next, '', url);
    this.current = next;
    this.fromTab = fromTab;
    this.url = url;
  }

  selectTab(path: string): void { this.go(path, false, true); }

  back(): void {
    if (this.current.previous) history.back();
    else this.go('/', true);
  }
}

export const router = new Router();

export function readerUrl(id: string, postId?: string): string {
  const focus = postId === undefined ? '' : `?post=${encodeURIComponent(postId)}`;
  return `/books/${encodeURIComponent(id)}${focus}`;
}

export function profileUrl(username: string): string {
  return `/profiles/${encodeURIComponent(username)}`;
}
