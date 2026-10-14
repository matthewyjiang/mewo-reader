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
