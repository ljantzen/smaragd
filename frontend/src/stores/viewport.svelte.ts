// Phase 9: a width-based breakpoint, not a platform check — a Tauri mobile
// build is still the same web bundle as desktop, and a desktop window
// resized narrow deserves the same single-panel layout a phone does. 820px
// covers phones and portrait tablets while leaving landscape tablets/
// desktop on dockview's multi-pane layout.
const MOBILE_BREAKPOINT = 820;

export const viewport = $state({
  isMobile: typeof window !== "undefined" ? window.innerWidth < MOBILE_BREAKPOINT : false,
});

if (typeof window !== "undefined") {
  const query = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`);
  query.addEventListener("change", (event) => {
    viewport.isMobile = event.matches;
  });
}
