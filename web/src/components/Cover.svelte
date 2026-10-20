<script lang="ts">
  let { name, src = null }: {
    name: string;
    src?: string | null;
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

<span class="cover" style:background={color(name)} aria-hidden="true">
  {#if src && !failed}
    <img {src} alt="" onerror={() => failed = true} />
  {:else}
    {name.trim().charAt(0).toUpperCase() || 'M'}
  {/if}
</span>

<style>
.cover {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 60px;
  border-radius: 4px;
  color: #fff;
  font-weight: 700;
  overflow: hidden;
  line-height: 1;
}

.cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

@media (forced-colors: active) {
  .cover {
    border: 1px solid CanvasText;
  }
}
</style>
