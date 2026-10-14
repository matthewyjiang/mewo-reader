<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import AccountDrawer from './components/AccountDrawer.svelte';
  import CommentSheet from './components/CommentSheet.svelte';
  import FeedStatus from './components/FeedStatus.svelte';
  import Icon from './components/Icon.svelte';
  import Navigation from './components/Navigation.svelte';
  import RemoveBookSheet from './components/RemoveBookSheet.svelte';
  import SignIn from './components/SignIn.svelte';
  import TopBar from './components/TopBar.svelte';
  import Home from './screens/Home.svelte';
  import Likes from './screens/Likes.svelte';
  import Profile from './screens/Profile.svelte';
  import Reader from './screens/Reader.svelte';
  import Search from './screens/Search.svelte';
  import Settings from './screens/Settings.svelte';
  import { chrome } from './lib/chrome.svelte';
  import type { BookRecord, FeedPost, PostHit } from './lib/models';
  import {
    display, bodySizes, bodyLines, headingSizes, headingLines,
  } from './lib/preferences.svelte';
  import { router, readerUrl, profileUrl } from './lib/router.svelte';
  import { store, message } from './lib/store.svelte';

  let main = $state<HTMLElement>();
  let fileInput = $state<HTMLInputElement>();
  let reader = $state<{ openChapters: () => void }>();
  let profile = $state<{ refresh: () => Promise<void> }>();
  let drawer = $state(false);
  let commentTarget = $state<PostHit | null>(null);
  let removeTarget = $state<BookRecord | null>(null);
  let uploading = $state(false);
  let uploadPercent = $state(0);
  let loading = $state(false);
  let pageError = $state('');
  let readerTitle = $state('Post');
  let readerBusy = $state(true);
  let profileCount = $state(0);
  let query = $state('');
  let shelfToken = '';
  let shelfPromise: Promise<void> | undefined;
  let touchStart: { x: number; y: number } | null = null;

  let path = $derived(router.url.pathname);
  let readerId = $derived(path.startsWith('/books/') ? decode(path.slice(7)) : null);
  let profileName = $derived(path.startsWith('/profiles/') ? decode(path.slice(10)) : null);
  let settings = $derived(path.startsWith('/settings'));
  let root = $derived(path === '/' || path === '/search' || path === '/likes');
  let pageTitle = $derived(readerId ? readerTitle : profileName ?? titleFor(path));

  function decode(value: string): string {
    try { return decodeURIComponent(value); } catch { return value; }
  }

  function titleFor(path: string): string {
    const titles: Record<string, string> = {
      '/': 'Home', '/search': 'Search', '/likes': 'Likes', '/settings': 'Settings',
      '/settings/display': 'Display', '/settings/account': 'Account',
      '/settings/library': 'Library',
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
    if (shelfToken !== token || !shelfPromise) {
      shelfToken = token;
      shelfPromise = store.load().catch((error: unknown) => {
        shelfPromise = undefined;
        throw error;
      });
    }
    await shelfPromise;
  }

  // Only navigation/session changes mount destinations, never progress or social updates.
  $effect(() => {
    const token = store.session?.token;
    router.current.key;
    let active = true;
    untrack(() => {
      drawer = false;
      commentTarget = null;
      removeTarget = null;
      pageError = '';
      readerTitle = 'Post';
      readerBusy = true;
      profileCount = 0;
      chrome.reset();
      if (!token) {
        loading = false;
        shelfToken = '';
        shelfPromise = undefined;
        return;
      }
      loading = true;
      ensureShelf(token).then(() => {
        if (active && token === store.session?.token) loading = false;
      }).catch((error: unknown) => {
        if (active) { pageError = message(error); loading = false; }
      });
    });
    return () => { active = false; };
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
      if (main) router.scroll.set(router.current.key, main.scrollTop);
      store.flushProgress();
    };
    const save = (): void => { store.flushProgress(); };
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

  function pickFile(): void { fileInput?.click(); }

  async function upload(file: File, sample = false): Promise<void> {
    if (uploading) return;
    uploading = true;
    uploadPercent = 0;
    store.notice = '';
    const token = store.session?.token;
    try {
      const book = await store.upload(file, sample, (percent) => uploadPercent = percent);
      if (store.session?.token === token) router.go(readerUrl(book.id));
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
    try { await store.signOut(); } catch (error) { store.notice = message(error); }
    router.go('/', true);
  }

  function retry(): void {
    shelfPromise = undefined;
    router.go(router.url.pathname + router.url.search, true);
  }

  function refreshProfile(): void {
    void profile?.refresh().catch((error: unknown) => store.notice = message(error));
  }

  function onTouchStart(event: TouchEvent): void {
    const touch = event.touches[0];
    touchStart = root && touch ? { x: touch.clientX, y: touch.clientY } : null;
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
        busy={loading || (readerId !== null && readerBusy)}
        count={profileCount}
        bind:query
        onaccount={() => drawer = true}
        onchapters={() => reader?.openChapters()}
      />
      <main
        bind:this={main}
        class="timeline"
        class:settings-timeline={settings}
        tabindex="-1"
        aria-label={pageTitle}
        onscroll={(event) => {
          if (!settings) chrome.scroll(event.currentTarget.scrollTop);
        }}
        ontouchstart={onTouchStart}
        ontouchend={onTouchEnd}
      >
        {#if loading || pageError}
          <FeedStatus loading={loading} error={pageError} label="your shelf" onretry={retry} />
        {:else if main}
          {#key router.current.key}
            {#if path === '/'}
              <Home
                scroller={main}
                {uploading}
                onadd={pickFile}
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
              <Search scroller={main} {query} oncomment={openComments} />
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
                <h2>That page isn't here</h2>
                <button class="pill" onclick={() => router.go('/')}>Back to Home</button>
              </div>
            {/if}
          {/key}
        {/if}
      </main>
      {#if path === '/' && store.books.length && !loading}
        <button class="fab" aria-label="Add an EPUB" disabled={uploading} onclick={pickFile}>
          <Icon name="plus" size={28} />
        </button>
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
            {uploadPercent === 100 ? 'Making your timeline…' : 'Adding your EPUB…'}
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
    onsignout={signOut}
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
