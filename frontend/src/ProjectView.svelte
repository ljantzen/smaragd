<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // Phase 2 proof-of-concept: project::open + document::read/save round
  // tripping through IPC. Deliberately not the real Binder UI (that's
  // Phase 4) — a flat recursive file-tree list is enough to prove the
  // command layer and DTOs before building the real widgets on top of them.
  type BinderNodeDto =
    | { kind: "Folder"; name: string; path: string; children: BinderNodeDto[] }
    | { kind: "Document"; name: string; path: string };

  type ProjectSummary = {
    root: string;
    book_title: string | null;
    tree: BinderNodeDto;
  };

  let rootInput = $state("");
  let project = $state<ProjectSummary | null>(null);
  let openError = $state<string | null>(null);

  let selectedPath = $state<string | null>(null);
  let documentText = $state("");
  let documentError = $state<string | null>(null);
  let saveStatus = $state<"idle" | "saving" | "saved" | "error">("idle");

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
  }

  async function openDocument(path: string) {
    documentError = null;
    saveStatus = "idle";
    selectedPath = path;
    try {
      documentText = await invoke<string>("document_read", { path });
    } catch (err) {
      documentError = String(err);
    }
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
      <button onclick={closeProject}>Close</button>
    </div>
    <div class="workspace">
      <nav class="tree">
        {#snippet node(n: BinderNodeDto)}
          {#if n.kind === "Folder"}
            <details open>
              <summary>{n.name}</summary>
              <ul>
                {#each n.children as child}
                  <li>{@render node(child)}</li>
                {/each}
              </ul>
            </details>
          {:else}
            <button
              class="doc"
              class:selected={selectedPath === n.path}
              onclick={() => openDocument(n.path)}
            >
              {n.name}
            </button>
          {/if}
        {/snippet}
        {@render node(project.tree)}
      </nav>
      <div class="document">
        {#if selectedPath}
          <textarea bind:value={documentText}></textarea>
          <div class="doc-toolbar">
            <button onclick={saveDocument} disabled={saveStatus === "saving"}>Save</button>
            {#if saveStatus === "saved"}<span>Saved</span>{/if}
            {#if documentError}<span class="error">{documentError}</span>{/if}
          </div>
        {:else}
          <p>Select a document.</p>
        {/if}
      </div>
    </div>
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
    margin-bottom: 0.75rem;
  }

  .workspace {
    display: grid;
    grid-template-columns: 16rem 1fr;
    gap: 1rem;
    min-height: 20rem;
  }

  .tree {
    border: 1px solid #ccc;
    padding: 0.5rem;
    overflow: auto;
  }

  .tree ul {
    list-style: none;
    margin: 0;
    padding-left: 1rem;
  }

  .tree .doc {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: 0.2rem 0.3rem;
    cursor: pointer;
  }

  .tree .doc.selected {
    background: #eef;
  }

  .document {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .document textarea {
    flex: 1;
    min-height: 16rem;
    font-family: ui-monospace, monospace;
    padding: 0.5rem;
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
