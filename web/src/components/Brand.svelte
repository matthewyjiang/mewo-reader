<script lang="ts">
  import { display } from '../lib/preferences.svelte';

  // Official marks, same path data as Android ic_brand_x / ic_brand_twitter and iOS BrandShapes.
  const X_PATH = 'M86.8918 28.25H99.0219L72.5243 58.5334L103.698 99.75H79.285L60.1647 74.7536'
    + 'L38.2929 99.75H26.1627L54.5069 67.3565L24.5938 28.25H49.6199L66.9004 51.0974L86.8918 28.25Z'
    + 'M82.6337 92.4904H89.3555L45.9716 35.1301H38.7584L82.6337 92.4904Z';
  const BIRD_PATH = 'M221.95 51.29c.15 2.17.15 4.34.15 6.53 0 66.73-50.8 143.69-143.69 143.69v-.04'
    + 'c-27.44.04-54.31-7.82-77.41-22.64 3.99.48 8 .72 12.02.73 22.74.02 44.83-7.61 62.72-21.66'
    + '-21.61-.41-40.56-14.5-47.18-35.07 7.57 1.46 15.37 1.16 22.8-.87-23.56-4.76-40.51-25.46-40.51-49.5'
    + 'v-.64c7.02 3.91 14.88 6.08 22.92 6.32C11.58 63.31 4.74 33.79 18.14 10.71'
    + 'c25.64 31.55 63.47 50.73 104.08 52.76-4.07-17.54 1.49-35.92 14.61-48.25'
    + ' 20.34-19.12 52.33-18.14 71.45 2.19 11.31-2.23 22.15-6.38 32.07-12.26'
    + '-3.77 11.69-11.66 21.62-22.2 27.93 10.01-1.18 19.79-3.86 29-7.95'
    + '-6.78 10.16-15.32 19.01-25.2 26.16Z';

  let twitter = $derived(display.theme.startsWith('twitter'));
</script>

<span class="brand" class:twitter>
  {#if display.mewo}
    <img src="/mewo-teddy.png" alt="Mewo" />
  {:else}
    {#if twitter}
      <svg width="28" height="23" viewBox="0 0 248 204" fill="currentColor" aria-hidden="true">
        <path d={BIRD_PATH} />
      </svg>
    {:else}
      <svg width="24" height="22" viewBox="24.6 28.25 80 72" fill="currentColor" aria-hidden="true">
        <path d={X_PATH} fill-rule="evenodd" />
      </svg>
    {/if}
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
