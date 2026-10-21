<script lang="ts">
  let { title }: { title: string } = $props();
</script>

<header class="search-section">
  <h2>{title}</h2>
</header>

<style>
  .search-section {
    box-sizing: border-box;
    height: 44px;
    display: flex;
    align-items: center;
    padding: 0 16px;
    border-bottom: .6px solid var(--line);
  }

  h2 {
    font-size: .9375rem;
    line-height: 1.25;
    font-weight: 700;
  }
</style>
