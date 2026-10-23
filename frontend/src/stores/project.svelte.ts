import { invoke } from "@tauri-apps/api/core";

// Phase 5: lifted out of ProjectView.svelte so every dock panel (Binder,
// Editor, Preview, Corkboard, Story Grid) can read/mutate the same project
// state regardless of which dockview group it's mounted in — a dockview
// panel's content is mounted into a plain detached <div> via Svelte's
// imperative mount(), outside the normal component tree, so passing state
// down through props/context from a single parent no longer works once
// panels can be freely split, floated, and closed/reopened independently.
//
// Fields live on one exported `$state` object rather than as separate
// top-level exports: Svelte 5 forbids reassigning an exported `$state`
// binding directly (`state_invalid_export`) since that can't be represented
// safely across a module boundary — only mutating an exported state value's
// *properties* is allowed, which this shape gives every field.
export type BinderNodeDto =
  | { kind: "Folder"; name: string; path: string; children: BinderNodeDto[] }
  | { kind: "Document"; name: string; path: string };

export type ProjectSummary = {
  root: string;
  book_title: string | null;
  tree: BinderNodeDto;
};

export const project = $state<{
  value: ProjectSummary | null;
  openError: string | null;
  selectedPath: string | null;
  documentText: string;
  documentError: string | null;
  saveStatus: "idle" | "saving" | "saved" | "error";
  previewHtml: string;
}>({
  value: null,
  openError: null,
  selectedPath: null,
  documentText: "",
  documentError: null,
  saveStatus: "idle",
  previewHtml: "",
});

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let previewTimer: ReturnType<typeof setTimeout> | undefined;
let editorRef: { setDocument: (text: string) => void } | undefined;

/** Called by EditorPanel.svelte on mount/unmount. A panel reopened from the
 * Window menu (or moved to a new group, which remounts its content) needs
 * today's already-open document echoed back into it immediately, since it
 * missed whatever `openDocument` call originally populated `documentText`. */
export function registerEditor(ref: typeof editorRef) {
  editorRef = ref;
  if (editorRef && project.selectedPath) editorRef.setDocument(project.documentText);
}

export async function openProject(root: string) {
  project.openError = null;
  project.value = null;
  project.selectedPath = null;
  project.documentText = "";
  project.previewHtml = "";
  try {
    project.value = await invoke<ProjectSummary>("project_open", { root });
  } catch (err) {
    project.openError = String(err);
  }
}

export async function closeProject() {
  await invoke("project_close");
  project.value = null;
  project.selectedPath = null;
  project.documentText = "";
  project.previewHtml = "";
}

export async function openDocument(path: string) {
  flushPendingSave();
  project.documentError = null;
  project.saveStatus = "idle";
  try {
    const text = await invoke<string>("document_read", { path });
    project.documentText = text;
    project.selectedPath = path;
    editorRef?.setDocument(text);
    schedulePreview();
  } catch (err) {
    project.documentError = String(err);
  }
}

export function onEditorChange(text: string) {
  project.documentText = text;
  project.saveStatus = "idle";
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveDocument, 800);
  schedulePreview();
}

function schedulePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(async () => {
    project.previewHtml = await invoke<string>("render_markdown", {
      markdown: project.documentText,
    });
  }, 300);
}

export async function saveDocument() {
  if (!project.selectedPath) return;
  project.saveStatus = "saving";
  try {
    await invoke("document_save", {
      path: project.selectedPath,
      contents: project.documentText,
    });
    project.saveStatus = "saved";
  } catch (err) {
    project.documentError = String(err);
    project.saveStatus = "error";
  }
}

function flushPendingSave() {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    saveDocument();
  }
}

export function onTreeChange(tree: BinderNodeDto) {
  if (project.value) project.value.tree = tree;
}
