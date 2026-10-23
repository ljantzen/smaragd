<script lang="ts">
  import { settingsStore, saveSettingsDebounced } from "./stores/settings.svelte";
  import { SHORTCUT_ACTIONS, SHORTCUT_CATEGORIES, effectiveCombo, setShortcut } from "./shortcuts";
  import type { KeyCombo } from "./stores/settings.svelte";

  // Settings dialog matching the real egui app's 9-category structure
  // (app/src/ui/settings_panel.rs) — General, Appearance, Editor,
  // SpellCheck, Templates, Versions, Sync, Pomodoro, Shortcuts, in that
  // order. Every field here is real (persists through core::settings::
  // Settings via settings_save) but several don't have a functional
  // consumer in this build yet — SpellCheck and Sync tabs are placeholders
  // for exactly that reason (no dictionary catalog, no real sync pairing —
  // Phase 6's Sync panel is demo-only), noted inline rather than silently
  // omitted, same as every other scope cut in this port.
  let { onClose }: { onClose: () => void } = $props();

  type Category =
    | "General"
    | "Appearance"
    | "Editor"
    | "SpellCheck"
    | "Templates"
    | "Versions"
    | "Sync"
    | "Pomodoro"
    | "Shortcuts";

  const categories: Category[] = [
    "General",
    "Appearance",
    "Editor",
    "SpellCheck",
    "Templates",
    "Versions",
    "Sync",
    "Pomodoro",
    "Shortcuts",
  ];

  let active = $state<Category>("General");

  function update() {
    settingsStore.value = { ...settingsStore.value };
    saveSettingsDebounced();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !recordingFor) onClose();
  }

  let recordingFor = $state<string | null>(null);

  function startRecording(actionId: string) {
    recordingFor = actionId;
  }

  function onRecordKeydown(e: KeyboardEvent) {
    if (!recordingFor) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recordingFor = null;
      return;
    }
    // Skip a bare modifier press — wait for the real key, same as the real
    // app's recording modal.
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    if (!e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
      // Real app rejects unmodified printable keys (function keys/Escape
      // exempt) — simplified here to: require at least one modifier, or a
      // function key.
      if (!/^F\d{1,2}$/.test(e.key)) return;
    }
    const combo: KeyCombo = {
      key: e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase(),
      ctrl_or_cmd: e.ctrlKey || e.metaKey,
      shift: e.shiftKey,
      alt: e.altKey,
    };
    setShortcut(recordingFor, combo);
    recordingFor = null;
  }

  function comboLabel(combo: KeyCombo | undefined): string {
    if (!combo || combo.key === "") return "Unbound";
    const mod = combo.ctrl_or_cmd ? "Ctrl+" : "";
    const alt = combo.alt ? "Alt+" : "";
    const shift = combo.shift ? "Shift+" : "";
    return `${mod}${alt}${shift}${combo.key.toUpperCase()}`;
  }
</script>

<svelte:window onkeydown={recordingFor ? onRecordKeydown : keydown} />

<div class="backdrop" onclick={onClose} role="presentation">
  <div
    class="modal"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="layout">
      <nav class="categories">
        {#each categories as category (category)}
          <button class:active={active === category} onclick={() => (active = category)}>
            {category}
          </button>
        {/each}
      </nav>
      <div class="content">
        {#if active === "General"}
          <label class="row">
            <input type="checkbox" bind:checked={settingsStore.value.reopen_last_project} onchange={update} />
            Reopen project on launch
          </label>
          <label class="row">
            <input
              type="checkbox"
              bind:checked={settingsStore.value.create_starter_folders}
              onchange={update}
            />
            Ensure Research and Trash folders exist in every project
          </label>
        {:else if active === "Appearance"}
          <label class="row">
            Theme
            <select bind:value={settingsStore.value.theme_preference} onchange={update}>
              <option value="system">System</option>
              <option value="dark">Dark</option>
              <option value="light">Light</option>
            </select>
          </label>
          <label class="row">
            Color theme
            <select bind:value={settingsStore.value.color_theme} onchange={update}>
              <option value={null}>Default</option>
              {#each settingsStore.colorThemes as theme (theme.id)}
                <option value={theme.id}>{theme.label}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            UI scale (%)
            <input
              type="number"
              min="50"
              max="300"
              placeholder="auto"
              value={settingsStore.value.ui_scale ? Math.round(settingsStore.value.ui_scale * 100) : ""}
              oninput={(e) => {
                const v = Number((e.target as HTMLInputElement).value);
                settingsStore.value.ui_scale = v ? v / 100 : 0;
                update();
              }}
            />
          </label>
        {:else if active === "Editor"}
          <label class="row">
            Editor font
            <select bind:value={settingsStore.value.editor_font} onchange={update}>
              <option value="Proportional">Proportional</option>
              <option value="Monospace">Monospace</option>
              <option value="LibertinusSerif">Libertinus Serif</option>
              <option value="DejaVuSansMono">DejaVu Sans Mono</option>
              <option value="AtkinsonHyperlegible">Atkinson Hyperlegible</option>
            </select>
          </label>
          <label class="row">
            <input
              type="checkbox"
              bind:checked={settingsStore.value.typewriter_quotes}
              onchange={update}
            />
            Typewriter quotes in Preview and export
          </label>
          <label class="row">
            <input
              type="checkbox"
              bind:checked={settingsStore.value.show_editor_gutter}
              onchange={update}
            />
            Show line numbers
          </label>
          <label class="row">
            <input type="checkbox" bind:checked={settingsStore.value.multi_tab_editor} onchange={update} />
            Allow multiple documents open at once
          </label>
          <p class="note">
            Not yet wired to the Editor panel itself in this build — these persist but don't
            change its behavior yet.
          </p>
        {:else if active === "SpellCheck"}
          <p class="note">
            Spell checking isn't built in this Tauri port yet (no dictionary catalog, no
            in-editor flagging). Reserved tab, matching the real app's structure.
          </p>
        {:else if active === "Templates"}
          <label class="row">
            Date format for {"${{date}}"}
            <input
              type="text"
              placeholder="%Y-%m-%d"
              bind:value={settingsStore.value.template_date_format}
              onchange={update}
            />
          </label>
          <label class="row">
            Time format for {"${{time}}"}
            <input
              type="text"
              placeholder="%H:%M"
              bind:value={settingsStore.value.template_time_format}
              onchange={update}
            />
          </label>
        {:else if active === "Versions"}
          <label class="row">
            <input
              type="checkbox"
              checked={!settingsStore.value.git_integration_disabled}
              onchange={(e) => {
                settingsStore.value.git_integration_disabled = !(e.target as HTMLInputElement).checked;
                update();
              }}
            />
            Enable Git integration
          </label>
          <h4>Backups</h4>
          <label class="row">
            <input type="checkbox" bind:checked={settingsStore.value.backup_enabled} onchange={update} />
            Enable automatic backups
          </label>
          <label class="row">
            <input type="checkbox" bind:checked={settingsStore.value.backup_on_open} onchange={update} />
            Back up when opening a project
          </label>
          <label class="row">
            <input type="checkbox" bind:checked={settingsStore.value.backup_on_close} onchange={update} />
            Back up when closing a project
          </label>
          <label class="row">
            <input
              type="checkbox"
              bind:checked={settingsStore.value.backup_on_manual_save}
              onchange={update}
            />
            Back up on every manual save
          </label>
          <label class="row">
            Backups to keep
            <input
              type="number"
              min="1"
              max="100"
              placeholder="10"
              value={settingsStore.value.backup_keep_count || ""}
              oninput={(e) => {
                settingsStore.value.backup_keep_count = Number((e.target as HTMLInputElement).value);
                update();
              }}
            />
          </label>
        {:else if active === "Sync"}
          <p class="note">
            Sync has no real device pairing in this build yet — the Sync dock panel runs a
            local demo against an in-memory vault instead (see Phase 6). Server/passphrase
            settings aren't modeled here until real pairing exists.
          </p>
        {:else if active === "Pomodoro"}
          <label class="row">
            Work session (min)
            <input
              type="number"
              min="1"
              max="180"
              placeholder="25"
              value={settingsStore.value.pomodoro_work_minutes || ""}
              oninput={(e) => {
                settingsStore.value.pomodoro_work_minutes = Number((e.target as HTMLInputElement).value);
                update();
              }}
            />
          </label>
          <label class="row">
            Short break (min)
            <input
              type="number"
              min="1"
              max="180"
              placeholder="5"
              value={settingsStore.value.pomodoro_short_break_minutes || ""}
              oninput={(e) => {
                settingsStore.value.pomodoro_short_break_minutes = Number(
                  (e.target as HTMLInputElement).value,
                );
                update();
              }}
            />
          </label>
          <label class="row">
            Long break (min)
            <input
              type="number"
              min="1"
              max="180"
              placeholder="15"
              value={settingsStore.value.pomodoro_long_break_minutes || ""}
              oninput={(e) => {
                settingsStore.value.pomodoro_long_break_minutes = Number(
                  (e.target as HTMLInputElement).value,
                );
                update();
              }}
            />
          </label>
          <label class="row">
            <input
              type="checkbox"
              bind:checked={settingsStore.value.pomodoro_notifications_enabled}
              onchange={update}
            />
            Show a desktop notification when a phase completes
          </label>
          <p class="note">
            Not yet read by the Pomodoro panel itself (still Phase 1's hardcoded 25/5/15/4) —
            persists here, wiring it in is a later refinement.
          </p>
        {:else if active === "Shortcuts"}
          <table>
            <thead>
              <tr>
                <th>Category</th>
                <th>Action</th>
                <th>Shortcut</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each SHORTCUT_CATEGORIES as category (category)}
                {#each SHORTCUT_ACTIONS.filter((a) => a.category === category) as action (action.id)}
                  <tr>
                    <td class="muted">{category}</td>
                    <td>{action.label}</td>
                    <td>
                      {#if recordingFor === action.id}
                        <em>Press a new shortcut… (Esc to cancel)</em>
                      {:else}
                        {comboLabel(effectiveCombo(action))}
                      {/if}
                    </td>
                    <td class="actions">
                      <button onclick={() => startRecording(action.id)}>Change</button>
                      <button onclick={() => setShortcut(action.id, null)}>Clear</button>
                    </td>
                  </tr>
                {/each}
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    </div>
    <div class="footer">
      <button onclick={onClose}>OK</button>
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
    width: 720px;
    max-width: 95vw;
    height: 520px;
    max-height: 90vh;
    background: white;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
  }

  .layout {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .categories {
    width: 160px;
    border-right: 1px solid #ddd;
    display: flex;
    flex-direction: column;
    padding: 0.5rem 0;
    overflow: auto;
  }

  .categories button {
    text-align: left;
    background: none;
    border: none;
    padding: 0.4rem 0.75rem;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .categories button.active {
    background: #557;
    color: white;
  }

  .content {
    flex: 1;
    overflow: auto;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    font-size: 0.85rem;
  }

  .row input[type="text"],
  .row input[type="number"],
  .row select {
    max-width: 12rem;
  }

  h4 {
    margin: 0.5rem 0 0;
    font-size: 0.8rem;
    color: #888;
  }

  .note {
    font-size: 0.8rem;
    color: #888;
    font-style: italic;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }

  th,
  td {
    text-align: left;
    padding: 0.2rem 0.4rem;
    border-bottom: 1px solid #eee;
  }

  .muted {
    color: #999;
  }

  .actions {
    white-space: nowrap;
  }

  .actions button {
    font-size: 0.7rem;
    padding: 0.1rem 0.3rem;
  }

  .footer {
    border-top: 1px solid #ddd;
    padding: 0.5rem 1rem;
    display: flex;
    justify-content: flex-end;
  }
</style>
