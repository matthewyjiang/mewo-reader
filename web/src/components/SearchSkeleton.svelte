<script lang="ts">
  import type { SearchKind } from '../lib/models';

  let { kind, rows = 6 }: { kind: SearchKind; rows?: number } = $props();
</script>

<div class="skeletons" role="list" aria-busy="true">
  <p class="sr-only" role="status">Loading results…</p>
  {#each Array.from({ length: rows }, (_, index) => index) as index (index)}
    {#if kind === 'accounts'}
      <div class="account-row" role="listitem">
        <span class="bone avatar"></span>
        <span class="lines">
          <span class="bone line wide"></span>
          <span class="bone line"></span>
        </span>
      </div>
    {:else if kind === 'books'}
      <div class="book" role="listitem">
        <span class="bone avatar"></span>
        <span class="lines">
          <span class="bone line wide"></span>
          <span class="bone line"></span>
          <span class="bone line short"></span>
        </span>
      </div>
    {:else if kind === 'catalog'}
      <div class="catalog" role="listitem">
        <span class="bone cover"></span>
        <span class="lines">
          <span class="bone line wide"></span>
          <span class="bone line"></span>
        </span>
        <span class="bone slot"></span>
      </div>
    {:else}
      <div class="post" role="listitem">
        <span class="bone avatar"></span>
        <span class="lines">
          <span class="bone line wide"></span>
          <span class="bone line body"></span>
          <span class="bone line"></span>
        </span>
      </div>
    {/if}
  {/each}
</div>

<style>
  .lines {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .book, .catalog {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    border-bottom: .6px solid var(--line);
    padding: 12px 16px 16px;
  }

  .book {
    min-height: 72px;
  }

  .catalog {
    min-height: 88px;
  }

  .bone {
    display: block;
    position: relative;
    overflow: hidden;
    background: color-mix(in srgb, var(--mute) 16%, var(--ground));
    border-radius: 4px;
  }

  .bone::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(
      90deg,
      transparent,
      color-mix(in srgb, var(--ground) 70%, transparent),
      transparent
    );
    transform: translateX(-100%);
    animation: shimmer 1.4s ease-in-out infinite;
  }

  .avatar {
    width: 40px;
    height: 40px;
    flex: 0 0 40px;
    border-radius: 50%;
  }

  .cover {
    width: 40px;
    height: 60px;
    flex: 0 0 40px;
    border-radius: 3px;
  }

  .slot {
    width: 40px;
    height: 40px;
    flex: 0 0 40px;
    border-radius: 50%;
    margin-top: 0;
  }

  .line {
    height: 12px;
    width: 46%;
  }

  .line.wide {
    width: 72%;
    height: 14px;
  }

  .line.short {
    width: 28%;
  }

  .line.body {
    width: 88%;
  }

  @keyframes shimmer {
    to { transform: translateX(100%); }
  }

  @media (prefers-reduced-motion: reduce) {
    .bone::after {
      animation: none;
      display: none;
    }
  }
</style>
