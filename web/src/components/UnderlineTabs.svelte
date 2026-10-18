<script lang="ts">
  let { tabs, selected, fixed = false, onselect }: {
    tabs: { id: string; label: string }[];
    selected: string;
    fixed?: boolean;
    onselect: (id: string) => void;
  } = $props();
</script>

<div class="section-tabs" class:fixed role="tablist">
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      role="tab"
      aria-selected={selected === tab.id}
      onclick={() => onselect(tab.id)}
    >{tab.label}</button>
  {/each}
</div>

<style>
.section-tabs {
  display: flex;
  background: var(--ground);
  border-bottom: .6px solid var(--line);
}

.section-tabs.fixed {
  position: absolute;
  z-index: 4;
  top: 56px;
  left: 0;
  right: 0;
}

.section-tabs button {
  position: relative;
  flex: 1;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  font-size: .9375rem;
  color: var(--mute);
}

.section-tabs button[aria-selected='true'] {
  color: var(--ink);
}

.section-tabs button[aria-selected='true']::after {
  content: '';
  position: absolute;
  left: 50%;
  bottom: 0;
  width: 56px;
  height: 3px;
  transform: translateX(-50%);
  border-radius: 999px;
  background: var(--accent);
}

@media (forced-colors: active) {
  .section-tabs button[aria-selected='true']::after {
    background: CanvasText;
  }
}
</style>
