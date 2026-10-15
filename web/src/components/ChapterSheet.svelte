<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { FeedPost } from '../lib/models';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import Timeline from './Timeline.svelte';

  let { title, headings, current, onjump, onclose }: {
    title: string;
    headings: { post: FeedPost; index: number }[];
    current: number;
    onjump: (index: number) => void;
    onclose: () => void;
  } = $props();
  let keys = $derived(headings.map(({ post }) => post.id));
  let scroller = $state<HTMLDivElement>();
  let timeline = $state<ReturnType<typeof Timeline>>();

  onMount(() => {
    let active = true;
    void tick().then(async () => {
      if (active && timeline) await timeline.jump(current);
    });
    return () => { active = false; };
  });
</script>

<Sheet label={`Chapters, ${title}`} {onclose}>
  <header class="sheet-bar">
    <h2>{title}</h2>
    <button class="icon-button" aria-label="Close chapters" onclick={onclose}>
      <Icon name="close" />
    </button>
  </header>
  <div class="chapter-list" bind:this={scroller}>
    {#if !headings.length}
      <p class="inline-status">No chapters</p>
    {:else if scroller}
      <Timeline bind:this={timeline} {keys} {scroller}>
        {#snippet row(index: number)}
          <button
            class="chapter-row"
            class:current={index === current}
            aria-current={index === current ? 'location' : undefined}
            onclick={() => onjump(headings[index].index)}
          >
            {headings[index].post.text}
          </button>
        {/snippet}
      </Timeline>
    {/if}
  </div>
</Sheet>

<style>
  .chapter-list {
    overflow-y: auto;
    max-height: calc(85dvh - 56px - env(safe-area-inset-bottom));
  }

  .chapter-row {
    width: 100%;
    text-align: left;
    font-size: 1.0625rem;
    line-height: 1.41;
    padding: 16px;
    border-bottom: .6px solid var(--line);
  }

  .chapter-row:hover, .chapter-row.current {
    background: var(--hover);
  }

  .chapter-row:not(.current) {
    color: var(--mute);
  }
</style>
