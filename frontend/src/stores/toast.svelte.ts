// A minimal status-line toast — the one piece of UI feedback the menu
// bar/shortcuts system needs for actions with no real implementation yet
// ("not implemented" rather than a silent dead click) and isn't worth a
// full toast-stack system for on its own.
export const toast = $state<{ message: string | null }>({ message: null });

let timer: ReturnType<typeof setTimeout> | undefined;

export function showToast(message: string, durationMs = 3000) {
  toast.message = message;
  clearTimeout(timer);
  timer = setTimeout(() => (toast.message = null), durationMs);
}

export function notImplemented(label: string) {
  showToast(`${label}: not implemented yet`);
}
