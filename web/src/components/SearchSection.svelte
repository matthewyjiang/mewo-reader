<script lang="ts">
  let { title, note }: { title: string; note?: string } = $props();
</script>

<header class="search-section" class:noted={!!note}>
  <h2>{title}</h2>
  {#if note}<p>{note}</p>{/if}
</header>

<style>
  .search-section {
    box-sizing: border-box;
    height: 44px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    padding: 0 16px;
    border-bottom: .6px solid var(--line);
  }

  .search-section.noted {
    height: 64px;
  }

  h2 {
    font-size: .9375rem;
    line-height: 1.25;
    font-weight: 700;
  }

  p {
    color: var(--mute);
    font-size: .8125rem;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
