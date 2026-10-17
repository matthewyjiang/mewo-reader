<script lang="ts">
  let { loading, error, label, onretry }: {
    loading: boolean;
    error: string;
    label: string;
    onretry: () => void;
  } = $props();
</script>

{#if loading}
  <div class="empty-state" role="status">
    <span class="spinner"></span>
    <p>Loading {label}…</p>
  </div>
{:else if error}
  <div class="empty-state">
    <h2>Couldn't load</h2>
    <p class="error" role="alert">{error}</p>
    <button class="pill" onclick={onretry}>Try again</button>
  </div>
{/if}
