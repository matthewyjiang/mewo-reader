<script lang="ts">
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  let { label, kind = 'bottom', onclose, children }: {
    label: string;
    kind?: 'bottom' | 'full' | 'drawer';
    onclose: () => void;
    children: Snippet;
  } = $props();
  let dialog = $state<HTMLDialogElement>();

  function outside(event: PointerEvent): void {
    if (!dialog || event.target !== dialog) return;
    const box = dialog.getBoundingClientRect();
    if (event.clientX < box.left || event.clientX > box.right
      || event.clientY < box.top || event.clientY > box.bottom) onclose();
  }

  onMount(() => {
    const node = dialog!;
    const previous = document.activeElement;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    node.showModal();
    const frame = requestAnimationFrame(() => {
      node.querySelector<HTMLElement>('[aria-current="location"]')
        ?.scrollIntoView({ block: 'center', behavior: 'instant' });
    });
    return () => {
      cancelAnimationFrame(frame);
      node.close();
      document.body.style.overflow = overflow;
      if (previous instanceof HTMLElement && previous.isConnected) {
        previous.focus({ preventScroll: true });
      }
    };
  });
</script>

<dialog
  bind:this={dialog}
  class={`sheet ${kind}`}
  aria-label={label}
  oncancel={(event) => { event.preventDefault(); onclose(); }}
  onpointerdown={outside}
>
  <div class="sheet-content">{@render children()}</div>
</dialog>
