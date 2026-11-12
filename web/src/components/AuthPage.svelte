<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let { children }: { children: Snippet } = $props();
</script>

<div class="auth-page">
  <header class="auth-bar">
    <Icon name="book" size={28} />
    <span class="sr-only">Mewo</span>
  </header>
  <div class="auth-form">{@render children()}</div>
</div>

<style>
.auth-page {
  max-width: 600px;
  margin-inline: auto;
  min-height: 100dvh;
  padding-top: env(safe-area-inset-top);
  padding-bottom: env(safe-area-inset-bottom);
  border-inline: 1px solid var(--line);
}

.auth-bar {
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-bottom: .6px solid var(--line);
}

.auth-form {
  padding: 24px 16px;
  max-width: 460px;
  margin-inline: auto;
}

.auth-form :global(form) {
  display: flex;
  flex-direction: column;
  align-items: stretch;
}

.auth-form :global(h1) {
  font-size: 1.375rem;
  line-height: 52px;
  margin-bottom: 8px;
}

.auth-form :global(label) {
  margin: 8px 0;
  min-height: 24px;
  font-weight: 700;
}

.auth-form :global(input) {
  color: var(--ink);
  background: var(--ground);
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 12px;
  min-height: 48px;
  width: 100%;
}

.auth-form :global(input:focus) {
  border-color: var(--accent);
}

.auth-form :global(.error) {
  margin-top: 6px;
}

.auth-form :global(.auth-submit) {
  margin-top: 20px;
  width: 100%;
}

@media (max-width: 760px) {
  .auth-page {
    border: 0;
  }
}
</style>
