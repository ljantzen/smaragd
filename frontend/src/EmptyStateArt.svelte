<script lang="ts">
  // A quiet line-art background for the "Open a project" empty state — an
  // emerald-cut gem and a quill, hand-drawn as plain stroked paths (no
  // fill), faint enough to sit behind the heading/button without
  // competing with them. Purely decorative: aria-hidden, no interaction.
</script>

<svg
  class="empty-state-art"
  viewBox="0 0 400 400"
  aria-hidden="true"
  xmlns="http://www.w3.org/2000/svg"
>
  <!-- Emerald-cut gem: outer facet outline, an inner "table" facet, and the
       short radiating lines between each cut corner that read as facet
       edges on a real emerald cut. -->
  <g class="gem">
    <path
      d="M152,75 L248,75 L270,97 L270,243 L248,265 L152,265 L130,243 L130,97 Z"
    />
    <path
      d="M171,113 L229,113 L242,126 L242,214 L229,227 L171,227 L158,214 L158,126 Z"
    />
    <line x1="152" y1="75" x2="171" y2="113" />
    <line x1="248" y1="75" x2="229" y2="113" />
    <line x1="270" y1="97" x2="242" y2="126" />
    <line x1="270" y1="243" x2="242" y2="214" />
    <line x1="248" y1="265" x2="229" y2="227" />
    <line x1="152" y1="265" x2="171" y2="227" />
    <line x1="130" y1="243" x2="158" y2="214" />
    <line x1="130" y1="97" x2="158" y2="126" />
  </g>

  <!-- Quill: a curved spine (rachis) from the nib up to the feather's
       tip, with barbs fanning to both sides and angled back toward the
       nib — the actual direction real feather barbs grow, which a first
       pass of this got backwards (angled toward the tip, reading as a
       twig rather than a feather). Barbs taper shortest at both ends and
       widest at the middle, the usual feather silhouette. -->
  <g class="quill">
    <path d="M180,310 C160,240 130,140 140,20" />
    <path d="M180,310 L175,320 M180,310 L185,318" />
    <line x1="173.8" y1="288.1" x2="165.6" y2="293.8" />
    <line x1="173.8" y1="288.1" x2="182.0" y2="293.8" />
    <line x1="167.2" y1="264.5" x2="150.8" y2="275.9" />
    <line x1="167.2" y1="264.5" x2="183.6" y2="275.9" />
    <line x1="160.7" y1="239.2" x2="137.7" y2="255.2" />
    <line x1="160.7" y1="239.2" x2="183.7" y2="255.2" />
    <line x1="154.4" y1="212.2" x2="126.5" y2="231.6" />
    <line x1="154.4" y1="212.2" x2="182.3" y2="231.6" />
    <line x1="148.8" y1="183.8" x2="117.6" y2="205.5" />
    <line x1="148.8" y1="183.8" x2="180.0" y2="205.5" />
    <line x1="144.0" y1="153.8" x2="116.1" y2="173.2" />
    <line x1="144.0" y1="153.8" x2="171.9" y2="173.2" />
    <line x1="140.5" y1="122.3" x2="117.5" y2="138.3" />
    <line x1="140.5" y1="122.3" x2="164.5" y2="138.3" />
    <line x1="138.4" y1="89.5" x2="122.0" y2="100.9" />
    <line x1="138.4" y1="89.5" x2="154.8" y2="100.9" />
    <line x1="138.2" y1="55.4" x2="130.0" y2="61.1" />
    <line x1="138.2" y1="55.4" x2="146.4" y2="61.1" />
  </g>
</svg>

<style>
  .empty-state-art {
    width: min(70vw, 28rem);
    height: auto;
    fill: none;
    stroke: #000;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.14;
  }
</style>
