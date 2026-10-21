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
  let label = $derived(
    canRequest ? (variant === 'fab' ? 'Add a book' : undefined) : 'Add an EPUB',
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

  $effect(() => {
    if (!open) return;
    const node = root;
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      open = false;
    };
    const onPointer = (event: PointerEvent): void => {
      const target = event.target;
      if (node && target instanceof Node && node.contains(target)) return;
      open = false;
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', onPointer);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
    };
  });
</script>

<div class="anchor" class:fab={variant === 'fab'} bind:this={root}>
  {#if open}
    <div class="menu" role="menu">
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
    flex-direction: column;
    align-items: flex-start;
  }

  .anchor.fab {
    position: absolute;
    right: 16px;
    bottom: 68px;
    z-index: 4;
    display: flex;
    align-items: flex-end;
  }

  .menu {
    margin-bottom: 8px;
    min-width: max-content;
    background: var(--hover);
    border: 1px solid var(--line);
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

  .menu button:hover {
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
