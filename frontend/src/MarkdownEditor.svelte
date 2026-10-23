<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView, keymap, highlightActiveLine, lineNumbers } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown } from "@codemirror/lang-markdown";
  import { languages } from "@codemirror/language-data";

  // Phase 3 proof-of-concept: CodeMirror 6 in place of the plain <textarea>
  // from Phase 2. This is the highest-risk phase in the migration plan — the
  // whole motivation for leaving egui is that its text widgets have weak
  // IME/screen-reader/text-shaping support, and CodeMirror is where that bet
  // either pays off or doesn't. Keep this component plain (no custom theme,
  // no wikilink/tag decorations yet) so IME/a11y behavior can be judged
  // against a near-default CodeMirror setup first.
  let {
    value,
    onChange,
  }: {
    value: string;
    onChange: (text: string) => void;
  } = $props();

  let container: HTMLDivElement;
  let view: EditorView | undefined;

  onMount(() => {
    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        history(),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        markdown({ codeLanguages: languages }),
        EditorView.lineWrapping,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChange(update.state.doc.toString());
          }
        }),
      ],
    });

    view = new EditorView({ state, parent: container });
  });

  onDestroy(() => {
    view?.destroy();
  });

  // Replace the document wholesale when a different file is opened (not on
  // every local keystroke — `onChange` already owns that direction, and
  // resetting on every update would fight the editor's own cursor/undo
  // history each time the caller echoes `value` back via a prop update).
  export function setDocument(text: string) {
    if (!view) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
  }
</script>

<div class="editor" bind:this={container}></div>

<style>
  .editor {
    flex: 1;
    min-height: 16rem;
    overflow: auto;
    border: 1px solid #ccc;
    text-align: left;
  }

  .editor :global(.cm-editor) {
    height: 100%;
    font-size: 0.95rem;
  }
</style>
