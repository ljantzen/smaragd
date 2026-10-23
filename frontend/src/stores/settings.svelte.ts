import { invoke } from "@tauri-apps/api/core";

// Mirrors core::settings::Settings field-for-field. Persisted to its own
// smaragd-tauri.toml, not the real egui app's smaragd.toml — see that
// module's doc comment for why the two apps never share a settings file.
export type KeyCombo = { key: string; ctrl_or_cmd: boolean; shift: boolean; alt: boolean };
export type ThemePreference = "system" | "dark" | "light";
export type EditorFontId =
  | "Proportional"
  | "Monospace"
  | "LibertinusSerif"
  | "DejaVuSansMono"
  | "AtkinsonHyperlegible";

export type Settings = {
  reopen_last_project: boolean;
  last_project_path: string | null;
  recent_project_paths: string[];
  create_starter_folders: boolean;
  shortcuts: Record<string, KeyCombo>;
  theme_preference: ThemePreference;
  color_theme: string | null;
  template_date_format: string;
  template_time_format: string;
  editor_font: EditorFontId;
  editor_font_size: number;
  ui_font: EditorFontId;
  git_integration_disabled: boolean;
  backup_enabled: boolean;
  backup_on_open: boolean;
  backup_on_close: boolean;
  backup_on_manual_save: boolean;
  backup_keep_count: number;
  backup_dir: string | null;
  pomodoro_work_minutes: number;
  pomodoro_short_break_minutes: number;
  pomodoro_long_break_minutes: number;
  pomodoro_cycles_before_long_break: number;
  pomodoro_notifications_enabled: boolean;
  typewriter_quotes: boolean;
  show_editor_gutter: boolean;
  multi_tab_editor: boolean;
  ui_scale: number;
  double_shift_search_disabled: boolean;
};

export type ColorTheme = {
  id: string;
  label: string;
  dark: boolean;
  background: string;
  foreground: string;
  accent: string;
  broken_wikilink: string;
};

function defaults(): Settings {
  return {
    reopen_last_project: false,
    last_project_path: null,
    recent_project_paths: [],
    create_starter_folders: false,
    shortcuts: {},
    theme_preference: "system",
    color_theme: null,
    template_date_format: "",
    template_time_format: "",
    editor_font: "Proportional",
    editor_font_size: 0,
    ui_font: "Proportional",
    git_integration_disabled: false,
    backup_enabled: false,
    backup_on_open: false,
    backup_on_close: false,
    backup_on_manual_save: false,
    backup_keep_count: 0,
    backup_dir: null,
    pomodoro_work_minutes: 0,
    pomodoro_short_break_minutes: 0,
    pomodoro_long_break_minutes: 0,
    pomodoro_cycles_before_long_break: 0,
    pomodoro_notifications_enabled: false,
    typewriter_quotes: false,
    show_editor_gutter: false,
    multi_tab_editor: false,
    ui_scale: 0,
    double_shift_search_disabled: false,
  };
}

export const settingsStore = $state<{ value: Settings; loaded: boolean; colorThemes: ColorTheme[] }>({
  value: defaults(),
  loaded: false,
  colorThemes: [],
});

let saveTimer: ReturnType<typeof setTimeout> | undefined;

export async function loadSettings() {
  try {
    settingsStore.value = await invoke<Settings>("settings_load");
  } catch {
    settingsStore.value = defaults();
  }
  try {
    settingsStore.colorThemes = await invoke<ColorTheme[]>("color_themes_list");
  } catch {
    settingsStore.colorThemes = [];
  }
  settingsStore.loaded = true;
}

/** Debounced save, called after any in-place edit to settingsStore.value. */
export function saveSettingsDebounced() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    invoke("settings_save", { settings: settingsStore.value }).catch(() => {});
  }, 400);
}
