export const MB = 1024 * 1024;

export function bytesToMb(bytes: number): number {
  return bytes / MB;
}

/** Whole or fractional megabytes. Empty, junk, and non-positive values are null. */
export function parseMb(input: string): number | null {
  const trimmed = input.trim();
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null;
  const mb = Number(trimmed);
  if (!Number.isFinite(mb) || mb <= 0) return null;
  const bytes = Math.round(mb * MB);
  if (!Number.isSafeInteger(bytes) || bytes <= 0) return null;
  return bytes;
}

/** `40500000` → `38.6 MB`. Exact units drop the decimal. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${Math.round(value)} B`;
  const rounded = Math.round(value * 10) / 10;
  const text = Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
  return `${text} ${units[unit]}`;
}
