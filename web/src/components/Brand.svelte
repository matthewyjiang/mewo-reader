<script lang="ts">
  import { display } from '../lib/preferences.svelte';
  import Icon from './Icon.svelte';
</script>

<span class="brand" class:twitter={display.theme.startsWith('twitter')}>
  {#if display.mewo}
    <img src="/mewo-teddy.png" alt="Mewo" />
  {:else}
    <Icon name={display.theme.startsWith('twitter') ? 'bird' : 'x'} size={26} />
    <span class="sr-only">Mewo</span>
  {/if}
</span>

<style>
.brand {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 40px;
}

.brand img {
  object-fit: contain;
  height: 36px;
  width: 36px;
}

.brand.twitter {
  color: var(--accent);
}

:global(:root[data-theme='twitter_dim']) .brand.twitter {
  color: var(--ink);
}



</style>
