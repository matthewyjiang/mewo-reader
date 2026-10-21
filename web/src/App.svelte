<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import AccountDrawer from './components/AccountDrawer.svelte';
  import AddBookMenu from './components/AddBookMenu.svelte';
  import CommentSheet from './components/CommentSheet.svelte';
  import FeedStatus from './components/FeedStatus.svelte';
  import Icon from './components/Icon.svelte';
  import Navigation from './components/Navigation.svelte';
  import RemoveBookSheet from './components/RemoveBookSheet.svelte';
  import SignIn from './components/SignIn.svelte';
  import TopBar from './components/TopBar.svelte';
  import UnderlineTabs from './components/UnderlineTabs.svelte';
  import Feed from './screens/Feed.svelte';
  import Home from './screens/Home.svelte';
  import Likes from './screens/Likes.svelte';
  import Profile from './screens/Profile.svelte';
  import Reader from './screens/Reader.svelte';
  import Search from './screens/Search.svelte';
  import Requests from './screens/Requests.svelte';
  import Server from './screens/Server.svelte';
  import Settings from './screens/Settings.svelte';
  import { chrome } from './lib/chrome.svelte';
  import type { BookRecord, FeedPost, PostHit } from './lib/models';
  import {
    display, bodySizes, bodyLines, headingSizes, headingLines,
  } from './lib/preferences.svelte';
  import { router, readerUrl, profileUrl, homeTab, homeUrl } from './lib/router.svelte';
  import { pullOffset, refreshGap, shouldRefresh } from './lib/pull';
  import { store, message } from './lib/store.svelte';
  import { writeStored } from './lib/storage';

  let main = $state<HTMLElement>();
  let fileInput = $state<HTMLInputElement>();
  let reader = $state<{ openChapters: () => void; capture: () => void }>();
  let search = $state<{ capture: () => void; showCatalog: () => void }>();
  let catalogPick = $state(0);
  let profile = $state<{ refresh: () => Promise<void> }>();
  let drawer = $state(false);
  let commentTarget = $state<PostHit | null>(null);
  let removeTarget = $state<BookRecord | null>(null);
  let uploading = $state(false);
  let uploadPercent = $state(0);
  let readerTitle = $state('Post');
  let readerBusy = $state(true);
  let profileCount = $state<number | null>(null);
  let query = $state('');
  let shelfToken = '';
  let shelfPromise: Promise<void> | undefined;
  let meToken = '';
  let touchStart: { x: number; y: number } | null = null;
  let pullStart: { x: number; y: number; active: boolean } | null = null;
  let pull = $state(0);
  let pulling = $state(false);
  let section = $derived(homeTab(router.url));
  let path = $derived(router.url.pathname);
  let shelfRefreshing = $derived(store.shelf.status === 'loaded' && store.shelf.refreshing);
  let feedRefreshing = $derived(store.home.status === 'loaded' && store.home.refreshing);
  let refreshing = $derived(path === '/' && (section === 'books' ? shelfRefreshing : feedRefreshing));
  let readerId = $derived(path.startsWith('/books/') ? decode(path.slice(7)) : null);
  let profileName = $derived(path.startsWith('/profiles/') ? decode(path.slice(10)) : null);
  let settings = $derived(path.startsWith('/settings'));
  let root = $derived(path === '/' || path === '/search' || path === '/likes');
  let pageTitle = $derived(readerId ? readerTitle : profileName ?? titleFor(path));
  let pullGap = $derived(path === '/' ? refreshGap(pull, refreshing, reducedMotion()) : 0);

  function decode(value: string): string {
    try { return decodeURIComponent(value); } catch { return value; }
  }

  function titleFor(path: string): string {
    const titles: Record<string, string> = {
      '/': 'Home', '/search': 'Search', '/likes': 'Likes', '/settings': 'Settings',
      '/settings/display': 'Display', '/settings/account': 'Account',
      '/settings/library': 'Library', '/settings/about': 'About',
      '/settings/server': 'Server', '/settings/server/users': 'Users',
      '/settings/requests': 'Requests',
    };
    return titles[path] ?? 'Mewo';
  }

  function openProfile(username: string): void {
    drawer = false;
    commentTarget = null;
    router.go(profileUrl(username));
  }

  function openComments(book: BookRecord, post: FeedPost): void {
    commentTarget = { book, post };
  }

  async function ensureShelf(token: string): Promise<void> {
    if (shelfToken === token && shelfPromise) {
      await shelfPromise;
      return;
    }
    shelfToken = token;
    shelfPromise = store.load();
    await shelfPromise;
  }

  function ensureMe(token: string): void {
    if (meToken === token) return;
    meToken = token;
    void store.loadMe().then((ok) => {
      if (!ok && meToken === token) meToken = '';
    });
  }

  function refreshShelf(): void {
    shelfPromise = undefined;
    void store.load();
  }

  // Only navigation/session changes mount destinations, never progress or social updates.
  $effect(() => {
    const token = store.session?.token;
    router.current.key;
    untrack(() => {
      drawer = false;
      commentTarget = null;
      removeTarget = null;
      readerTitle = 'Post';
      readerBusy = true;
      profileCount = null;
      chrome.reset();
      if (!token) {
        shelfToken = '';
        shelfPromise = undefined;
        meToken = '';
        return;
      }
      ensureMe(token);
      void ensureShelf(token);
    });
  });

  $effect(() => {
    const theme = display.theme;
    const step = display.step;
    document.documentElement.dataset.theme = theme;
    const style = document.documentElement.style;
    style.setProperty('--post-size', `${bodySizes[step] / 16}rem`);
    style.setProperty('--post-line', `${bodyLines[step] / 16}rem`);
    style.setProperty('--heading-size', `${headingSizes[step] / 16}rem`);
    style.setProperty('--heading-line', `${headingLines[step] / 16}rem`);
    const ground = theme === 'twitter_dim'
      ? '#15202B' : theme === 'x_lights_out' ? '#000000' : '#FFFFFF';
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', ground);
  });

  $effect(() => { document.title = `${pageTitle} · Mewo`; });

  onMount(() => {
    router.beforeChange = () => {
      if (readerId) reader?.capture();
      else if (path === '/search') search?.capture();
      else if (main) {
        router.scroll.set(router.current.key, main.scrollTop);
        if (root) router.tabScroll.set(router.current.tab, main.scrollTop);
      }
      store.flushProgress();
    };
    const save = (): void => {
      if (readerId) reader?.capture();
      store.flushProgress(undefined, true);
    };
    const visibility = (): void => {
      if (document.visibilityState === 'hidden') save();
    };
    const keydown = (event: KeyboardEvent): void => {
      if (event.key === 'Tab') { chrome.keyboardNavigation = true; chrome.show(); }
    };
    document.addEventListener('visibilitychange', visibility);
    window.addEventListener('pagehide', save);
    window.addEventListener('keydown', keydown);
    return () => {
      save();
      router.beforeChange = undefined;
      document.removeEventListener('visibilitychange', visibility);
      window.removeEventListener('pagehide', save);
      window.removeEventListener('keydown', keydown);
    };
  });

  $effect(() => {
    const scroller = main;
    if (!scroller) return;
    const move = (event: TouchEvent): void => onTouchMove(event);
    scroller.addEventListener('touchmove', move, { passive: false });
    return () => scroller.removeEventListener('touchmove', move);
  });

  $effect(() => {
    if (!refreshing && !pulling) pull = 0;
  });

  function pickFile(): void { fileInput?.click(); }

  function browseCatalog(): void {
    writeStored('mewo.search.kind', 'catalog');
    catalogPick += 1;
    if (path === '/search') search?.showCatalog();
    else router.go('/search');
  }

  async function upload(file: File, sample = false): Promise<void> {
    if (uploading) return;
    uploading = true;
    uploadPercent = 0;
    store.notice = '';
    const token = store.session?.token;
    try {
      const book = await store.upload(file, sample, (percent) => uploadPercent = percent);
      if (store.session?.token === token) {
        if (!book.isSample && !book.shared) {
          store.notice = `${book.title} is private.`;
        }
        router.go(readerUrl(book.id));
      }
    } catch (error) {
      store.notice = message(error);
    } finally {
      uploading = false;
      if (fileInput) fileInput.value = '';
    }
  }

  async function sample(): Promise<void> {
    const existing = store.books.find((book) => book.isSample);
    if (existing) { router.go(readerUrl(existing.id)); return; }
    if (uploading) return;
    uploading = true;
    store.notice = '';
    const token = store.session?.token;
    try {
      const response = await fetch('/sample.epub');
      if (!response.ok) {
        throw new Error('Could not load the sample. Try adding an EPUB instead.');
      }
      const file = new File([await response.blob()], 'sample.epub', {
        type: 'application/epub+zip',
      });
      uploading = false;
      if (token !== store.session?.token) return;
      await upload(file, true);
    } catch (error) {
      store.notice = message(error);
      uploading = false;
    }
  }

  async function signOut(): Promise<void> {
    drawer = false;
    if (readerId) reader?.capture();
    try { await store.signOut(); } catch (error) { store.notice = message(error); }
    router.go('/', true);
  }

  function retry(): void {
    refreshShelf();
  }

  function refreshProfile(): void {
    void profile?.refresh().catch((error: unknown) => store.notice = message(error));
  }

  function reducedMotion(): boolean {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  }

  function pullTransform(): string | undefined {
    if (pullGap <= 0) return undefined;
    return `translateY(${pullGap}px)`;
  }

  function onTouchStart(event: TouchEvent): void {
    const touch = event.touches[0];
    touchStart = root && touch ? { x: touch.clientX, y: touch.clientY } : null;
    pullStart = path === '/' && touch && main && main.scrollTop <= 0
      ? { x: touch.clientX, y: touch.clientY, active: false }
      : null;
  }

  function onTouchMove(event: TouchEvent): void {
    const touch = event.touches[0];
    if (!pullStart || !touch || !main || path !== '/') return;
    const dy = touch.clientY - pullStart.y;
    const dx = touch.clientX - pullStart.x;
    if (!pullStart.active) {
      if (dy < 8 || main.scrollTop > 0) return;
      if (Math.abs(dx) > dy) { pullStart = null; return; }
      pullStart.active = true;
      pulling = true;
    }
    if (main.scrollTop > 0) { pull = 0; pulling = false; return; }
    pull = pullOffset(dy, reducedMotion());
    if (pull > 0) event.preventDefault();
  }

  function onTouchEnd(event: TouchEvent): void {
    const touch = event.changedTouches[0];
    if (touchStart && touch) {
      const dx = touch.clientX - touchStart.x;
      const dy = touch.clientY - touchStart.y;
      // One avatar-width from the edge and one bar-width of travel avoid accidental drags.
      if (touchStart.x <= 40 && dx >= 56 && Math.abs(dy) < dx) drawer = true;
    }
    touchStart = null;
    const armed = pullStart?.active && shouldRefresh(pull);
    pulling = false;
    if (armed) {
      if (section === 'books') refreshShelf();
      else void store.loadFeed();
    }
    // Drop the finger offset. A running refresh keeps its gap, and the
    // sheet eases from wherever the finger was.
    pull = 0;
    pullStart = null;
  }
</script>

{#if !store.session}
  <SignIn />
{:else}
  <div class="app-shell">
    <Navigation rail onaccount={() => drawer = true} />
    <div class="timeline-shell">
      <TopBar
        title={pageTitle}
        {root}
        reader={readerId !== null}
        profile={profileName !== null}
        busy={refreshing || (readerId !== null && readerBusy)}
        onrefresh={path === '/' ? () => {
          if (section === 'books') refreshShelf();
          else void store.loadFeed();
        } : undefined}
        count={profileCount}
        bind:query
        onaccount={() => drawer = true}
        onchapters={() => reader?.openChapters()}
      />
      {#if path === '/'}
        <UnderlineTabs
          tabs={[{ id: 'feed', label: 'Feed' }, { id: 'books', label: 'Books' }]}
          selected={section}
          fixed
          onselect={(id) => { if (id === 'feed' || id === 'books') router.go(homeUrl(id), true); }}
        />
      {/if}
      <main
        bind:this={main}
        class="timeline"
        class:has-tabs={path === '/' || path === '/search'}
        class:settings-timeline={settings}
        tabindex="-1"
        aria-label={pageTitle}
        onscroll={(event) => {
          if (!settings) chrome.scroll(event.currentTarget.scrollTop);
        }}
        ontouchstart={onTouchStart}
        ontouchend={onTouchEnd}
      >
        <div
          class="pull-sheet"
          class:pull-dragging={pulling}
          style:transform={pullTransform()}
        >
        {#if main}
          {#key router.current.key}
            {#if path === '/' && section === 'feed'}
              <Feed scroller={main} oncomment={openComments} onprofile={openProfile} />
            {:else if store.shelf.status === 'loading'}
              <FeedStatus loading={true} error="" label="your shelf" onretry={retry} />
            {:else if store.shelf.status === 'failed' && !store.shelf.books}
              <FeedStatus loading={false} error={store.shelf.error} label="your shelf" onretry={retry} />
            {:else if path === '/'}
              <Home
                scroller={main}
                {uploading}
                onadd={pickFile}
                onbrowse={browseCatalog}
                onsample={sample}
                onremove={(book) => removeTarget = book}
              />
            {:else if readerId}
              <Reader
                bind:this={reader}
                id={readerId}
                focusPostId={router.url.searchParams.get('post')}
                scroller={main}
                oncomment={openComments}
                onloaded={(book) => readerTitle = book.title}
                onbusy={(busy) => readerBusy = busy}
              />
            {:else if path === '/search'}
              <Search
                bind:this={search}
                scroller={main}
                {query}
                {catalogPick}
                oncomment={openComments}
                onprofile={openProfile}
                onadd={pickFile}
              />
            {:else if path === '/likes'}
              <Likes scroller={main} oncomment={openComments} />
            {:else if profileName}
              <Profile
                bind:this={profile}
                username={profileName}
                scroller={main}
                oncomment={openComments}
                onprofile={openProfile}
                oncount={(count) => profileCount = count}
              />
            {:else if path === '/settings/server' || path === '/settings/server/users'}
              <Server {path} scroller={main} onprofile={openProfile} />
            {:else if path === '/settings/requests'}
              <Requests scroller={main} />
            {:else if settings}
              <Settings
                {path}
                scroller={main}
                {uploading}
                onadd={pickFile}
                onsample={sample}
                onprofile={openProfile}
                onsignout={signOut}
              />
            {:else}
              <div class="empty-state">
                <h2>Not found</h2>
                <button class="pill" onclick={() => router.go('/')}>Back to Home</button>
              </div>
            {/if}
          {/key}
        {/if}
        </div>
      </main>
      {#if path === '/'}
        <div
          class="refresh-indicator below-tabs"
          class:pull-dragging={pulling}
          style:height="{pullGap}px"
          role="status"
          aria-hidden={pullGap === 0}
        >
          {#if pullGap > 0}<span class="spinner"></span>{/if}
          <span class="sr-only">{refreshing ? 'Refreshing' : 'Pull to refresh'}</span>
        </div>
      {/if}
      {#if path === '/' && section === 'books' && store.books.length}
        <AddBookMenu
          variant="fab"
          canRequest={store.me?.canRequest === true}
          {uploading}
          onupload={pickFile}
          onbrowse={browseCatalog}
        >
          <Icon name="plus" size={28} />
        </AddBookMenu>
      {/if}
      {#if !settings}<Navigation onaccount={() => drawer = true} />{/if}
      {#if store.notice}
        <div class="notice" role="status">
          <span>{store.notice}</span>
          <button
            class="icon-button"
            aria-label="Dismiss message"
            onclick={() => store.notice = ''}
          >
            <Icon name="close" size={18} />
          </button>
        </div>
      {/if}
      {#if uploading}
        <div class="upload-status" role="status">
          <strong>
            Uploading…
          </strong>
          <progress max="100" value={uploadPercent} aria-label="EPUB upload progress"></progress>
          <span class="meta">{uploadPercent}% uploaded</span>
        </div>
      {/if}
      <input
        class="sr-only"
        bind:this={fileInput}
        type="file"
        accept=".epub,application/epub+zip"
        tabindex="-1"
        aria-label="Choose an EPUB"
        onchange={(event) => {
          const file = event.currentTarget.files?.[0];
          if (file) void upload(file);
        }}
      />
    </div>
  </div>
{/if}

{#if drawer && store.session}
  <AccountDrawer
    onclose={() => drawer = false}
    onprofile={openProfile}
  />
{/if}
{#if commentTarget && store.session}
  {#key `${commentTarget.book.id}:${commentTarget.post.id}`}
    <CommentSheet
      target={commentTarget}
      onclose={() => commentTarget = null}
      onprofile={openProfile}
      onchanged={refreshProfile}
    />
  {/key}
{/if}
{#if removeTarget && store.session}
  <RemoveBookSheet book={removeTarget} onclose={() => removeTarget = null} />
{/if}

<style>
/* The timeline stays centered even when there is room for the navigation rail. */
.app-shell {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 600px) minmax(0, 1fr);
  width: 100%;
  height: 100dvh;
  padding-top: env(safe-area-inset-top);
  padding-bottom: env(safe-area-inset-bottom);
}

.timeline-shell {
  grid-column: 2;
  position: relative;
  min-width: 0;
  min-height: 0;
  border-inline: 1px solid var(--line);
  overflow: hidden;
}

.pull-sheet {
  transition: transform 200ms ease-out;
}

.pull-sheet.pull-dragging {
  transition: none;
}

.timeline.has-tabs {
  padding-top: 104px;
  scroll-padding-top: 104px;
}

.refresh-indicator {
  position: absolute;
  z-index: 4;
  top: 56px;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  pointer-events: none;
  transition: height 200ms ease-out;
}

.refresh-indicator.below-tabs {
  top: 104px;
}

.refresh-indicator.pull-dragging {
  transition: none;
}

.timeline {
  background: var(--ground);
  height: 100%;
  overflow-y: auto;
  overscroll-behavior-y: contain;
  overflow-anchor: none;
  padding: 56px 0 52px;
  scroll-padding-top: 56px;
}

.timeline.settings-timeline {
  padding-bottom: 0;
}


.notice {
  position: absolute;
  z-index: 10;
  bottom: 64px;
  left: 16px;
  right: 16px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 8px 8px 16px;
  border: 1px solid var(--line);
  background: var(--hover);
  font-size: .9375rem;
  line-height: 1.33;
  overflow-wrap: anywhere;
}

.notice > span {
  flex: 1;
}

.upload-status {
  position: absolute;
  z-index: 9;
  bottom: 64px;
  inset-inline: 16px;
  padding: 16px;
  background: var(--ground);
  border: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

progress {
  width: 100%;
  height: 4px;
  border: 0;
  border-radius: 0;
  background: var(--line);
  accent-color: var(--accent);
}

progress::-webkit-progress-bar {
  background: var(--line);
}

progress::-webkit-progress-value {
  background: var(--accent);
}

progress::-moz-progress-bar {
  background: var(--accent);
}

@media (prefers-reduced-motion: reduce) {
  .pull-sheet, .refresh-indicator { transition: none; }
}

@media (min-width: 1000px) {
  .timeline {
    padding-bottom: 0;
  }
  .notice, .upload-status {
    bottom: 16px;
  }
}

@media (max-width: 760px) {
  .app-shell {
    display: block;
    padding-inline: env(safe-area-inset-left) env(safe-area-inset-right);
  }
  .timeline-shell {
    height: 100%;
    border-inline: 0;
  }
}
</style>
