<script lang="ts">
  import { display, themes, sizeLabels } from '../lib/preferences.svelte';
  import Avatar from './Avatar.svelte';
</script>

<section class="display-settings" aria-label="Display preferences">
  <fieldset class="palette-fieldset">
    <legend>Choose your display</legend>
    <div class="palettes">
      {#each themes as theme}
        <button
          class={`palette ${theme.id}`}
          class:selected={display.theme === theme.id}
          aria-pressed={display.theme === theme.id}
          onclick={() => { display.theme = theme.id; display.save(); }}
        >
          <span class="palette-sample">
            <strong>Aa</strong>
            <span></span>
            <span></span>
          </span>
          <span>{theme.label}</span>
        </button>
      {/each}
    </div>
  </fieldset>
  <label class="setting-label" for="font-size">
    Font size <span class="meta">{sizeLabels[display.step]}</span>
  </label>
  <div class="size-control">
    <span>Aa</span>
    <input
      id="font-size"
      type="range"
      min="0"
      max="6"
      step="1"
      value={display.step}
      aria-valuetext={sizeLabels[display.step]}
      oninput={(event) => {
        display.step = event.currentTarget.valueAsNumber;
        display.save();
      }}
    />
    <span class="large-aa">Aa</span>
  </div>
  <div class="preview post">
    <Avatar name="Mewo" />
    <div class="post-column">
      <div class="post-meta">
        <strong>Mewo</strong>
        <span class="meta">@mewo · Preview</span>
      </div>
      <p class="post-body">
        The book is a timeline. One paragraph, one post. Make yourself comfortable.
      </p>
    </div>
  </div>
  <label class="switch-row">
    <span>
      <strong>Mewo mode</strong>
      <span class="meta">Teddy in the header. Same palette.</span>
    </span>
    <input
      type="checkbox"
      role="switch"
      checked={display.mewo}
      onchange={(event) => { display.mewo = event.currentTarget.checked; display.save(); }}
    />
  </label>
  <label class="switch-row">
    <span>
      <strong>Always show navigation</strong>
      <span class="meta">
        Keep bars visible while reading, including with a screen reader.
      </span>
    </span>
    <input
      type="checkbox"
      role="switch"
      checked={display.keepBars}
      onchange={(event) => { display.keepBars = event.currentTarget.checked; display.save(); }}
    />
  </label>
</section>

<style>
.display-settings {
  padding: 24px 16px;
}

.palette-fieldset {
  border: 0;
  padding: 0;
  margin: 0 0 24px;
}

.palette-fieldset legend {
  padding: 0;
  font-weight: 700;
  margin-bottom: 16px;
  font-size: 1.0625rem;
}

.palettes {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.palette {
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 12px;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 12px;
  text-align: left;
  background: var(--ground);
  color: var(--ink);
  font-size: .9375rem;
}

.palette.selected {
  outline: 2px solid var(--accent);
  outline-offset: 0;
}

.palette-sample {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
}

.palette-sample strong {
  font-size: 1.5rem;
  line-height: 1.18;
}

.palette-sample > span {
  display: block;
  height: 2px;
  width: 70%;
  background: var(--mute);
}

.palette-sample > span:last-child {
  width: 45%;
}

.setting-label {
  display: flex;
  justify-content: space-between;
  font-weight: 700;
  align-items: center;
  gap: 12px;
}

.size-control {
  display: flex;
  gap: 12px;
  align-items: center;
  padding-block: 16px;
}

.size-control input {
  flex: 1;
  min-width: 0;
  height: 40px;
  cursor: pointer;
}

.large-aa {
  font-size: 1.75rem;
}

.preview.post {
  border-block: .6px solid var(--line);
  padding-inline: 0;
  padding-bottom: 16px;
  margin-bottom: 8px;
}

.switch-row {
  display: flex;
  align-items: center;
  gap: 16px;
  justify-content: space-between;
  padding: 20px 0;
  border-bottom: .6px solid var(--line);
}

.switch-row > span {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
}

.switch-row input {
  width: 36px;
  height: 24px;
  flex: 0 0 36px;
  cursor: pointer;
}

@media (forced-colors: active) {
  .palette.selected {
    outline-color: Highlight;
  }
}
</style>
