<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { project } from "../stores/project.svelte";

  // Phase 7: read-only — core has no save-frontmatter command yet (the real
  // metadata_panel.rs writes the document's YAML frontmatter block in
  // place, which this phase doesn't add). Later refinement.
  type DocumentMeta = {
    section_type: string | null;
    status: string | null;
    pov: string | null;
    word_count_target: number | null;
    tags: string[];
  };

  let meta = $state<DocumentMeta | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const path = project.selectedPath;
    if (!path) {
      meta = null;
      return;
    }
    invoke<DocumentMeta>("document_metadata", { root: project.value!.root, path })
      .then((result) => (meta = result))
      .catch((err) => (error = String(err)));
  });
</script>

<div class="panel">
  {#if !project.selectedPath}
    <p class="empty">Select a document in the Binder.</p>
  {:else if error}
    <p class="error">{error}</p>
  {:else if meta}
    <dl>
      <dt>Type</dt>
      <dd>{meta.section_type ?? "—"}</dd>
      <dt>Status</dt>
      <dd>{meta.status ?? "—"}</dd>
      <dt>POV</dt>
      <dd>{meta.pov ?? "—"}</dd>
      <dt>Word count target</dt>
      <dd>{meta.word_count_target ?? "—"}</dd>
      <dt>Tags</dt>
      <dd>{meta.tags.length ? meta.tags.map((t) => `#${t}`).join(" ") : "—"}</dd>
    </dl>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow: auto;
    padding: 0.5rem;
    box-sizing: border-box;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.3rem 1rem;
    margin: 0;
  }

  dt {
    color: #888;
    font-size: 0.85rem;
  }

  dd {
    margin: 0;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
