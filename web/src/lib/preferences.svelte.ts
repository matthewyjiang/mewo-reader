import { readStored, writeStored } from './storage';

export type Theme = 'x_light' | 'x_lights_out' | 'twitter_light' | 'twitter_dim';
export const themes: { id: Theme; label: string }[] = [
  { id: 'twitter_light', label: 'Twitter light' },
  { id: 'x_light', label: 'X light' },
  { id: 'twitter_dim', label: 'Twitter dim' },
  { id: 'x_lights_out', label: 'Lights out' },
];

// These seven steps are the Compose ReaderTypeScale, not a scale for the chrome.
export const bodySizes = [14, 15, 17, 19, 21, 24, 28];
export const bodyLines = [20, 22, 24, 27, 30, 34, 39];
export const headingSizes = [14, 16, 17, 19, 21, 24, 28];
export const headingLines = [18, 21, 22, 25, 27, 31, 36];
export const sizeLabels = [
  'Extra small', 'Small', 'Default', 'Medium', 'Large', 'Extra large', 'Largest',
];

interface Preferences {
  theme: Theme;
  step: number;
  mewo: boolean;
  keepBars: boolean;
}

const defaultTheme = matchMedia('(prefers-color-scheme: dark)').matches
  ? 'x_lights_out' : 'x_light';
const saved = readStored<Partial<Preferences> | null>('mewo.display', null);

class DisplayPreferences {
  theme = $state<Theme>(
    themes.some((theme) => theme.id === saved?.theme) ? saved!.theme! : defaultTheme,
  );
  step = $state(
    typeof saved?.step === 'number' && Number.isInteger(saved.step)
      && saved.step >= 0 && saved.step <= 6 ? saved.step : 2,
  );
  mewo = $state(saved?.mewo === true);
  keepBars = $state(saved?.keepBars === true);

  save(): void {
    writeStored('mewo.display', {
      theme: this.theme, step: this.step, mewo: this.mewo, keepBars: this.keepBars,
    });
  }
}

export const display = new DisplayPreferences();
