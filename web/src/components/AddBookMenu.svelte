<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    canRequest,
    uploading = false,
    variant = 'pill',
    onupload,
    onbrowse,
    children,
  }: {
    canRequest: boolean;
    uploading?: boolean;
    variant?: 'pill' | 'fab';
    onupload: () => void;
    onbrowse: () => void;
    children: Snippet;
  } = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement>();
  let menuEl = $state<HTMLDivElement>();
  let label = $derived(
    variant === 'fab' ? (canRequest ? 'Add a book' : 'Add an EPUB') : undefined,
  );

  function activate(): void {
    if (uploading) return;
    if (!canRequest) {
      onupload();
      return;
    }
    open = !open;
  }

  function choose(run: () => void): void {
    open = false;
    run();
  }

  function items(): HTMLButtonElement[] {
    return [...(menuEl?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
  }

  $effect(() => {
    if (!open) return;
    const node = root;
    const menu = menuEl;
    if (!node || !menu) return;
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.preventDefault();
        open = false;
        return;
      }
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      const buttons = items();
      if (!buttons.length) return;
      event.preventDefault();
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next = index < 0
        ? (delta > 0 ? 0 : buttons.length - 1)
        : (index + delta + buttons.length) % buttons.length;
      buttons[next]?.focus();
    };
    const onPointer = (event: PointerEvent): void => {
      const target = event.target;
      if (target instanceof Node && node.contains(target)) return;
      open = false;
    };
    const onFocusOut = (event: FocusEvent): void => {
      // Removing the menu drops focus and fires this during that update.
      if (!open) return;
      const next = event.relatedTarget;
      if (next instanceof Node && node.contains(next)) return;
      open = false;
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', onPointer);
    node.addEventListener('focusout', onFocusOut);
    items()[0]?.focus();
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
      node.removeEventListener('focusout', onFocusOut);
    };
  });
</script>

<div class="anchor" class:fab={variant === 'fab'} bind:this={root}>
  {#if open}
    <div class="menu" role="menu" bind:this={menuEl}>
      <button type="button" role="menuitem" onclick={() => choose(onupload)}>Upload EPUB</button>
      <button type="button" role="menuitem" onclick={() => choose(onbrowse)}>Browse catalog</button>
    </div>
  {/if}
  <button
    type="button"
    class={variant}
    aria-label={label}
    aria-haspopup={canRequest ? 'menu' : undefined}
    aria-expanded={canRequest ? open : undefined}
    disabled={uploading}
    onclick={activate}
  >
    {@render children()}
  </button>
</div>

<style>
  .anchor {
    position: relative;
    display: inline-flex;
  }

  .anchor.fab {
    position: absolute;
    right: 16px;
    bottom: 68px;
    z-index: 4;
  }

  .menu {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 1;
    min-width: max-content;
    background: var(--hover);
    border: 1px solid var(--line);
  }

  .anchor.fab .menu {
    left: auto;
    right: 0;
  }

  .menu button {
    display: flex;
    align-items: center;
    width: 100%;
    height: 48px;
    padding: 0 16px;
    background: transparent;
    color: var(--ink);
    font-size: .9375rem;
    line-height: 20px;
    text-align: left;
    white-space: nowrap;
  }

  .menu button:hover,
  .menu button:focus-visible {
    background: color-mix(in srgb, var(--ink) 8%, var(--hover));
  }

  button.fab {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
  }

  button.fab:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 85%, var(--ink));
  }

  @media (min-width: 1000px) {
    .anchor.fab {
      bottom: 24px;
    }
  }
</style>
