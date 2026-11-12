<script lang="ts">
  import Icon from './Icon.svelte';

  let { label, items, selected, testid, onselect }: {
    label: string;
    items: { id: string | null; name: string }[];
    selected: string | null;
    testid: string;
    onselect: (id: string | null) => void;
  } = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement>();
  let menuEl = $state<HTMLDivElement>();

  function choose(id: string | null): void {
    open = false;
    onselect(id);
  }

  function buttons(): HTMLButtonElement[] {
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
      const list = buttons();
      if (!list.length) return;
      event.preventDefault();
      const index = list.indexOf(document.activeElement as HTMLButtonElement);
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next = index < 0
        ? (delta > 0 ? 0 : list.length - 1)
        : (index + delta + list.length) % list.length;
      list[next]?.focus();
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
    (buttons().find((button) => button.getAttribute('aria-checked') === 'true') ?? buttons()[0])?.focus();
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
      node.removeEventListener('focusout', onFocusOut);
    };
  });
</script>

<div class="anchor" bind:this={root}>
  <button
    type="button"
    class="trigger"
    data-testid={testid}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => open = !open}
  >
    <span class="label">{label}</span>
    <span class="chevron"><Icon name="chevron" size={16} /></span>
  </button>
  {#if open}
    <div class="menu" role="menu" bind:this={menuEl}>
      {#each items as item (item.id)}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={item.id === selected}
          onclick={() => choose(item.id)}
        >
          <span class="name">{item.name}</span>
          {#if item.id === selected}
            <Icon name="check" size={20} />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .anchor {
    position: relative;
    display: inline-flex;
    min-width: 0;
  }

  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    height: 36px;
    padding: 0 12px 0 16px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: transparent;
    color: var(--ink);
    font-size: .9375rem;
    font-weight: 700;
    line-height: 20px;
  }

  .trigger:hover {
    background: var(--hover);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chevron {
    display: inline-flex;
    flex: none;
    transform: rotate(90deg);
  }

  .menu {
    position: absolute;
    top: calc(100% + 8px);
    left: 0;
    z-index: 3;
    min-width: max(100%, 200px);
    max-height: min(60vh, 432px);
    overflow-y: auto;
    background: var(--hover);
    border: 1px solid var(--line);
  }

  .menu button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
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

  .menu button:focus-visible {
    outline-offset: -2px;
  }
</style>
