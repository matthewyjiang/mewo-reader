/** Secure-context clipboard, otherwise the plain-HTTP `execCommand` fallback. */
export async function copyText(text: string, failure: string): Promise<void> {
  if (navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(text);
    return;
  }
  // Clipboard API is unavailable on plain-HTTP LAN deployments.
  const field = document.createElement('textarea');
  field.value = text;
  field.style.position = 'fixed';
  field.style.top = '-100%';
  const previous = document.activeElement;
  document.body.appendChild(field);
  field.select();
  const copied = document.execCommand('copy');
  field.remove();
  if (previous instanceof HTMLElement) previous.focus();
  if (!copied) throw new Error(failure);
}

/** Quote matches the phones: share the passage with author and book attribution. */
export async function shareText(text: string): Promise<boolean> {
  if (navigator.share) {
    try {
      await navigator.share({ text });
      return false;
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return false;
    }
  }
  await copyText(
    text,
    'Your browser could not copy this line. Select the passage and copy it manually.',
  );
  return true;
}
