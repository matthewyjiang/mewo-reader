<script lang="ts">
  let { name, src = null, size = 40 }: {
    name: string;
    src?: string | null;
    size?: number;
  } = $props();

  const palette = ['#1D9BF0', '#F91880', '#00BA7C', '#FF7A00', '#7856FF', '#FFAD1F'];
  let failed = $state(false);

  function color(value: string): string {
    let hash = 0;
    for (let i = 0; i < value.length; i++) {
      hash = (Math.imul(hash, 31) + value.charCodeAt(i)) | 0;
    }
    return palette[(hash & 0x7fffffff) % palette.length];
  }

  $effect(() => { src; failed = false; });
</script>

<span
  class="avatar"
  style:width={`${size}px`}
  style:height={`${size}px`}
  style:font-size={`${size * 0.42}px`}
  style:background={color(name)}
  aria-hidden="true"
>
  {#if src && !failed}
    <img {src} alt="" onerror={() => failed = true} />
  {:else}
    {name.trim().charAt(0).toUpperCase() || 'M'}
  {/if}
</span>
