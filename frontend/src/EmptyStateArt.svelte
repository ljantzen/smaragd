<script lang="ts">
  // The "Open a project" empty state's illustration — a provided pen-and-
  // ink style drawing of an emerald-cut gem and a quill (with inkwell and
  // scattered feathers), replacing the earlier hand-authored SVG attempt.
  // Imported as a Vite asset (not inlined/absolute-positioned behind text
  // like the SVG was) so Vite hashes and bundles it properly; shown as its
  // own illustration above the open-project controls rather than as a
  // low-opacity watermark behind them — it has its own cream paper
  // background and isn't meant to read as transparent line art.
  import backgroundUrl from "./assets/empty-state-background.png";
</script>

<img class="empty-state-art" src={backgroundUrl} alt="" />

<style>
  .empty-state-art {
    width: min(90vw, 40rem);
    height: auto;
    border-radius: 4px;
  }
</style>
