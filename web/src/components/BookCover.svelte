<script lang="ts">
  let { title, src = null }: {
    title: string;
    src?: string | null;
  } = $props();

  let failed = $state(false);
  const letter = $derived(title.trim().charAt(0).toUpperCase() || '?');

  $effect(() => { src; failed = false; });
</script>

<span class="book-cover" aria-hidden="true">
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
  background: color-mix(in srgb, var(--mute) 14%, var(--ground));
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
