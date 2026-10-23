<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import MarkdownEditor from "./MarkdownEditor.svelte";
  import BinderTree, { type BinderNodeDto } from "./BinderTree.svelte";
  import CorkboardView from "./CorkboardView.svelte";
  import StoryGridView from "./StoryGridView.svelte";

  type ProjectSummary = {
    root: string;
    book_title: string | null;
    tree: BinderNodeDto;
  };

  type View = "binder" | "corkboard" | "storygrid";

  let rootInput = $state("");
  let project = $state<ProjectSummary | null>(null);
  let openError = $state<string | null>(null);
  let view = $state<View>("binder");

  let selectedPath = $state<string | null>(null);
  let documentText = $state("");
  let documentError = $state<string | null>(null);
  let saveStatus = $state<"idle" | "saving" | "saved" | "error">("idle");
  let previewHtml = $state("");

  let editor = $state<MarkdownEditor>();
  // Phase 3's "autosave" requirement: debounce document_save to 800ms after
  // the last keystroke rather than saving on every change (one IPC round
  // trip per keystroke would be wasteful and, on a slow disk, could start
  // queuing up out of order) or requiring a manual Save click.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;

  async function openProject() {
    openError = null;
    project = null;
    selectedPath = null;
    try {
      project = await invoke<ProjectSummary>("project_open", { root: rootInput });
    } catch (err) {
      openError = String(err);
    }
  }

  async function closeProject() {
    await invoke("project_close");
    project = null;
    selectedPath = null;
    documentText = "";
    previewHtml = "";
  }

  async function openDocument(path: string) {
    // Flush any pending autosave for the document being navigated away from
    // before switching, so the last few keystrokes aren't lost.
    flushPendingSave();

    documentError = null;
    saveStatus = "idle";
    try {
      const text = await invoke<string>("document_read", { path });
      documentText = text;
      // Fetched before `selectedPath` flips, so when the document pane first
      // mounts `MarkdownEditor` it reads the already-correct text via its
      // `value` prop rather than racing the mount against this await.
      selectedPath = path;
      editor?.setDocument(text);
      schedulePreview();
    } catch (err) {
      documentError = String(err);
    }
  }

  function onEditorChange(text: string) {
    documentText = text;
    saveStatus = "idle";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveDocument, 800);
    schedulePreview();
  }

  function schedulePreview() {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      previewHtml = await invoke<string>("render_markdown", { markdown: documentText });
    }, 300);
  }

  async function saveDocument() {
    if (!selectedPath) return;
    saveStatus = "saving";
    try {
      await invoke("document_save", { path: selectedPath, contents: documentText });
      saveStatus = "saved";
    } catch (err) {
      documentError = String(err);
      saveStatus = "error";
    }
  }

  function flushPendingSave() {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      saveDocument();
    }
  }

  function onTreeChange(newTree: BinderNodeDto) {
    if (project) project.tree = newTree;
  }
</script>

<section class="project">
  {#if !project}
    <h2>Open a project</h2>
    <form onsubmit={(e) => { e.preventDefault(); openProject(); }}>
      <input
        type="text"
        placeholder="/path/to/project"
        bind:value={rootInput}
      />
      <button type="submit">Open</button>
    </form>
    {#if openError}
      <p class="error">{openError}</p>
    {/if}
  {:else}
    <div class="toolbar">
      <strong>{project.book_title ?? project.root}</strong>
      <div class="view-tabs">
        <button class:active={view === "binder"} onclick={() => (view = "binder")}>Binder</button>
        <button class:active={view === "corkboard"} onclick={() => (view = "corkboard")}>Corkboard</button>
        <button class:active={view === "storygrid"} onclick={() => (view = "storygrid")}>Story Grid</button>
      </div>
      <button onclick={closeProject}>Close</button>
    </div>

    {#if view === "binder"}
      <div class="workspace">
        <BinderTree
          root={project.root}
          tree={project.tree}
          {selectedPath}
          onSelect={openDocument}
          {onTreeChange}
        />
        <div class="document">
          {#if selectedPath}
            <div class="editor-preview">
              <MarkdownEditor bind:this={editor} value={documentText} onChange={onEditorChange} />
              <div class="preview">{@html previewHtml}</div>
            </div>
            <div class="doc-toolbar">
              {#if saveStatus === "saving"}<span>Saving…</span>{/if}
              {#if saveStatus === "saved"}<span>Saved</span>{/if}
              {#if documentError}<span class="error">{documentError}</span>{/if}
            </div>
          {:else}
            <p>Select a document.</p>
          {/if}
        </div>
      </div>
    {:else if view === "corkboard"}
      <CorkboardView root={project.root} />
    {:else}
      <StoryGridView root={project.root} />
    {/if}
  {/if}
</section>

<style>
  .project {
    width: 100%;
    max-width: 60rem;
    text-align: left;
  }

  form {
    display: flex;
    gap: 0.5rem;
  }

  form input {
    flex: 1;
    padding: 0.4rem;
  }

  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
    margin-bottom: 0.75rem;
  }

  .view-tabs {
    display: flex;
    gap: 0.25rem;
  }

  .view-tabs button.active {
    background: #557;
    color: white;
  }

  .workspace {
    display: grid;
    grid-template-columns: 16rem 1fr;
    gap: 1rem;
    min-height: 20rem;
  }

  .document {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .editor-preview {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1rem;
    flex: 1;
    min-height: 16rem;
  }

  .preview {
    overflow: auto;
    border: 1px solid #ccc;
    padding: 0.5rem 0.75rem;
  }

  .doc-toolbar {
    display: flex;
    gap: 0.75rem;
    align-items: center;
  }

  .error {
    color: #c0392b;
  }
</style>
