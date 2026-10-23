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

  <!-- Quill: a curved spine from the nib up to the feather's tip, with
       barbs fanning out, longest at the middle and tapering toward both
       ends — resting diagonally across the gem. -->
  <g class="quill">
    <path d="M180,310 C160,240 130,140 140,20" />
    <path d="M180,310 L175,320 M180,310 L185,318" />
    <line x1="176" y1="280" x2="164.1" y2="272.6" />
    <line x1="170" y1="245" x2="151.3" y2="233.3" />
    <line x1="162" y1="208" x2="136.5" y2="192.1" />
    <line x1="155" y1="170" x2="126.1" y2="152" />
    <line x1="150" y1="130" x2="124.5" y2="114.1" />
    <line x1="146" y1="90" x2="127.3" y2="78.3" />
    <line x1="143" y1="55" x2="131.1" y2="47.6" />
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
