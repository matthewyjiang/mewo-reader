<script lang="ts">
  let { title, src = null, width = 40, height = 60 }: {
    title: string;
    src?: string | null;
    width?: number;
    height?: number;
  } = $props();

  let failed = $state(false);
  const letter = $derived(title.trim().charAt(0).toUpperCase() || '?');

  $effect(() => { src; failed = false; });
</script>

<span class="book-cover" style:width="{width}px" style:height="{height}px" aria-hidden="true">
  {#if src && !failed}
    <img {src} alt="" loading="lazy" onerror={() => failed = true} />
  {:else}
    <span class="letter">{letter}</span>
  {/if}
</span>

<style>
.book-cover {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 60px;
  border: .6px solid var(--line);
  border-radius: 3px;
  background: var(--hover);
  color: var(--mute);
  overflow: hidden;
  line-height: 1;
}

.book-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.letter {
  font-weight: 700;
  font-size: 1.0625rem;
}

@media (forced-colors: active) {
  .book-cover {
    border: 1px solid CanvasText;
  }
}
</style>
