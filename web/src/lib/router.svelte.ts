/** Tiny history router: real URLs, browser Back, and per-entry feed scroll positions. */
import type { TimelineAnchor } from './timeline';

export type Tab = 'home' | 'search' | 'likes';

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
  scroll = new Map<string, number | TimelineAnchor>();
  tabScroll = new Map<Tab, number | TimelineAnchor>();
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
    // A profile tab is the same screen. A new key would remount it and flash the header.
    const sameProfile = replace
      && url.pathname === this.url.pathname
      && url.pathname.startsWith('/profiles/');
    const next: Entry = {
      mewo: true,
      key: sameProfile ? this.current.key : newKey(),
      tab,
      previous: !replace || this.current.previous,
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

export type ProfileTab = 'posts' | 'reposts' | 'books';

export function profileTab(url: URL): ProfileTab {
  const tab = url.searchParams.get('tab');
  return tab === 'reposts' || tab === 'books' ? tab : 'posts';
}

export function profileUrl(username: string, tab: ProfileTab = 'posts'): string {
  const path = `/profiles/${encodeURIComponent(username)}`;
  return tab === 'posts' ? path : `${path}?tab=${tab}`;
}

export type HomeTab = 'feed' | 'books';

export function homeTab(url: URL): HomeTab {
  return url.pathname === '/' && url.searchParams.get('tab') === 'books' ? 'books' : 'feed';
}

export function homeUrl(tab: HomeTab): string {
  return tab === 'books' ? '/?tab=books' : '/';
}
