<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import { project } from "./stores/project.svelte";
  import { showToast } from "./stores/toast.svelte";

  // Export Manuscript: real docx/epub/pdf rendering via core::export
  // (ported in Phase 0, never used by a command until now). Folder choice
  // mirrors the real File menu's three-way branch: zero Manuscript-role
  // folders exports the whole project root, one exports it directly, two
  // or more asks which — here, as a plain select rather than a submenu.
  let { onClose }: { onClose: () => void } = $props();

  type ManuscriptFolder = { path: string; name: string };

  let folders = $state<ManuscriptFolder[]>([]);
  let folder = $state("");
  let format = $state<"docx" | "epub" | "pdf">("docx");
  let title = $state("");
  let subtitle = $state("");
  let author = $state("");
  let error = $state<string | null>(null);
  let exporting = $state(false);

  onMount(async () => {
    try {
      folders = await invoke<ManuscriptFolder[]>("manuscript_folders", {
        root: project.value!.root,
      });
      folder = folders[0]?.path ?? project.value!.root;
      title = project.value!.book_title ?? "";
    } catch (err) {
      error = String(err);
    }
  });

  async function runExport() {
    const extensions: Record<string, string> = { docx: "docx", epub: "epub", pdf: "pdf" };
    const outPath = await save({
      defaultPath: `${title || "manuscript"}.${extensions[format]}`,
      filters: [{ name: format.toUpperCase(), extensions: [extensions[format]] }],
    });
    if (!outPath) return;
    exporting = true;
    error = null;
    try {
      await invoke("export_manuscript", {
        root: project.value!.root,
        folder,
        format,
        title,
        subtitle,
        author,
        outPath,
      });
      showToast(`Exported to ${outPath}`);
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      exporting = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onClose()} />

<div
  class="backdrop"
  onclick={onClose}
  onkeydown={(e) => e.key === "Escape" && onClose()}
  role="presentation"
>
  <div
    class="modal"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <h2>Export Manuscript</h2>

    {#if folders.length > 1}
      <label class="row">
        Folder
        <select bind:value={folder}>
          {#each folders as f (f.path)}
            <option value={f.path}>{f.name}</option>
          {/each}
        </select>
      </label>
    {/if}

    <label class="row">
      Format
      <select bind:value={format}>
        <option value="docx">Word (.docx)</option>
        <option value="epub">EPUB (.epub)</option>
        <option value="pdf">PDF (.pdf)</option>
      </select>
    </label>

    <label class="row">
      Title
      <input type="text" bind:value={title} />
    </label>
    <label class="row">
      Subtitle
      <input type="text" bind:value={subtitle} />
    </label>
    <label class="row">
      Author
      <input type="text" bind:value={author} />
    </label>

    {#if error}<p class="error">{error}</p>{/if}

    <div class="footer">
      <button onclick={onClose}>Cancel</button>
      <button onclick={runExport} disabled={exporting}>
        {exporting ? "Exporting…" : "Export…"}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    width: 24rem;
    background: white;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  h2 {
    margin: 0 0 0.25rem;
    font-size: 1.1rem;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    font-size: 0.85rem;
  }

  .row input,
  .row select {
    flex: 1;
    max-width: 14rem;
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .error {
    color: #c0392b;
    font-size: 0.8rem;
  }
</style>
