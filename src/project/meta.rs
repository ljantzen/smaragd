use super::*;

/// Manual ordering and per-node metadata that the filesystem itself can't express.
/// Keyed by a `/`-separated path relative to the project root ("" for the root folder
/// itself) rather than `PathBuf`, so the file stays portable across platforms and
/// serializes to plain JSON without ambiguity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectMeta {
    pub version: u32,
    pub node_order: HashMap<String, Vec<String>>,
    /// `#[serde(default)]` is required, not cosmetic: project.json files written
    /// before this field existed have no `folder_roles`/`trashed_origins` keys at
    /// all — without a default, deserializing them would fail outright and silently
    /// discard their real, already-persisted `node_order` data.
    #[serde(default)]
    pub folder_roles: HashMap<String, FolderRole>,
    /// A trashed item's *current* relative key (its path inside the Trash folder,
    /// post-move) → its *original* relative key (where it lived pre-delete).
    /// Disambiguates same-named items trashed from different folders and is what a
    /// future "restore from trash" action needs to put something back where it came
    /// from — the on-disk name alone (deduplicated with a " (2)" suffix on collision)
    /// doesn't carry that.
    #[serde(default)]
    pub trashed_origins: HashMap<String, String>,
    /// Lisa Cron-style story/plotting cards, deliberately *not* tied to the binder
    /// tree or `node_order`: a card may exist with no linked document at all (a pure
    /// plotting artifact, drafted before any scene exists) and its position in this
    /// list — the corkboard order — is independent of manuscript order. Storing them
    /// here rather than as document frontmatter sidesteps frontmatter write-back
    /// entirely (see `frontmatter.rs`'s doc comment on why that isn't implemented).
    #[serde(default)]
    pub story_cards: Vec<StoryCard>,
    /// The protagonist's driving external/internal want — half of Lisa Cron's
    /// "Third Rail" (the other half is `protagonist_misbelief`): the throughline
    /// every scene's `StoryCard::why_it_matters` should ultimately test or advance.
    /// Project-wide rather than per-scene, since it's meant to anchor the whole
    /// manuscript's arc, not vary scene to scene. Edited from the Corkboard view.
    #[serde(default)]
    pub protagonist_desire: String,
    /// The flawed, usually childhood-formed belief standing between the protagonist
    /// and `protagonist_desire` — see that field's doc comment.
    #[serde(default)]
    pub protagonist_misbelief: String,
    /// Whether git version control (commit/push/pull from the Versions menu, modeled
    /// after the Obsidian Git plugin) is turned on for this project. Deliberately a
    /// per-project setting, not a global one in `Settings`/`settings.rs`: one project
    /// folder might be a git repo (or want to be) while another isn't, and there's no
    /// single "on for every project" answer that would make sense.
    #[serde(default)]
    pub git_enabled: bool,
    /// Whether the user has already been asked (via the one-time "enable git
    /// support?" dialog) whether to turn `git_enabled` on — regardless of their
    /// answer, prevents nagging them again every time the project is opened. Doesn't
    /// block a later manual "Enable Git Support" from the Versions menu.
    #[serde(default)]
    pub git_prompted: bool,
    /// Whether this project's own `.smaragd/plugins/*.rhai` scripts are loaded,
    /// in addition to the always-loaded global plugin directory. Off by default and
    /// requires an explicit action to turn on (see `Project::set_plugins_enabled`)
    /// — unlike a global plugin (which the user deliberately placed themselves), a
    /// project's own plugin folder could arrive via a shared/pulled git repo, so
    /// loading it without consent would be silent code execution from someone
    /// else's content, not just a convenience default.
    #[serde(default)]
    pub plugins_enabled: bool,
    /// Book-level title/author, entered once in the Export dialog and reused on
    /// every later export rather than retyped each time. `None` (not an empty
    /// string) until the user has actually set one, so a never-exported project's
    /// `project.json` doesn't grow two empty-string keys for no reason.
    #[serde(default)]
    pub book_title: Option<String>,
    /// Optional subtitle — not every book has one, so this stays `None` rather
    /// than defaulting to an empty string shown alongside `book_title`.
    #[serde(default)]
    pub book_subtitle: Option<String>,
    #[serde(default)]
    pub book_author: Option<String>,
    /// The chosen `export::style::TypesetStyle` id, same reuse-across-exports
    /// rationale as `book_title`/`book_author`. `None` means "use the export
    /// dialog's own default" rather than a project.json key with a specific
    /// style id baked in, so the default can change later without a migration.
    #[serde(default)]
    pub book_style: Option<String>,
    /// The relative key (same `/`-joined, root-is-`""` encoding `folder_roles` uses)
    /// of the folder whose direct child documents' titles populate the Type field's
    /// dropdown in the Metadata panel — see [`PicklistField::Type`] and
    /// [`Project::picklist_documents`]. `None` (the default) keeps that field plain
    /// free text, unchanged from before this existed.
    #[serde(default)]
    pub type_picklist_folder: Option<String>,
    /// Same as `type_picklist_folder`, for the POV field.
    #[serde(default)]
    pub pov_picklist_folder: Option<String>,
    /// Same as `type_picklist_folder`, for the Status field.
    #[serde(default)]
    pub status_picklist_folder: Option<String>,
    /// Overall manuscript word-count goal (Scrivener's "Draft Target"), edited in
    /// the Word Count panel. `None` until set — no target means no progress bar
    /// to show, not a target of 0. Deliberately named differently from
    /// `frontmatter::DocumentMeta::word_count_target` (a per-scene target shown
    /// in the Metadata panel) — the two are unrelated and never aggregated
    /// together.
    #[serde(default)]
    pub draft_target_words: Option<u32>,
    /// Today's writing-session goal (Scrivener's "Session Target"), independent
    /// of `draft_target_words` — see `Project::word_count`'s doc comment for how
    /// "words written this session" is derived from it.
    #[serde(default)]
    pub session_target_words: Option<u32>,
    /// Which documents count toward the Word Count panel's live total — see
    /// [`WordCountScope`].
    #[serde(default)]
    pub word_count_scope: WordCountScope,
    /// The project's total word count (per `word_count_scope`) as of the start of
    /// the current writing session — `session_target_words`'s progress is the
    /// live total minus this baseline, never the raw total. Rolled forward
    /// automatically on a new calendar day (see
    /// [`Project::maybe_roll_over_session`]) or manually via "Reset Session" (see
    /// [`Project::reset_session`]).
    #[serde(default)]
    pub session_baseline_words: u32,
    /// The date (`YYYY-MM-DD`, `chrono::Local`, same convention as
    /// `templates.rs`'s `${{date}}`) `session_baseline_words` was captured on —
    /// `None` before the session mechanism has ever run once.
    #[serde(default)]
    pub session_baseline_date: Option<String>,
    /// Each tracked calendar day's total words written (a delta, not a
    /// cumulative running total), keyed `YYYY-MM-DD` — same string
    /// convention as `session_baseline_date`. Populated exclusively by
    /// [`Project::maybe_roll_over_session`] when a new day rolls over, and
    /// pruned to `streak::DAILY_HISTORY_RETENTION_DAYS` on every write.
    /// Feeds `streak::evaluate_streak` for the Writing Streak feature.
    ///
    /// Accepted precision limitation: a day's logged value is captured
    /// whenever the *first* word-count recompute of the *next* day happens
    /// to fire, not at a precise midnight boundary — the same approximation
    /// `session_baseline_date`'s rollover already makes. If the app is
    /// opened and typed in before that recompute runs, a few of the new
    /// day's words can bleed into the previous day's logged figure.
    #[serde(default)]
    pub daily_word_counts: BTreeMap<String, u32>,
    /// Master on/off switch for Writing Streak tracking on this project. Off
    /// by default (same as `git_enabled`/`plugins_enabled` above) — a
    /// deliberately per-project setting, not a global one in `Settings`:
    /// different projects can reasonably want different writing paces (or
    /// none at all), same rationale as `git_enabled`.
    #[serde(default)]
    pub streak_enabled: bool,
    /// The weekly word-count schedule the streak is measured against — see
    /// [`crate::streak::WeeklySchedule`].
    #[serde(default)]
    pub streak_schedule: crate::streak::WeeklySchedule,
    /// How strictly a week counts as "met" — see
    /// [`crate::streak::StreakEvaluationMode`].
    #[serde(default)]
    pub streak_evaluation_mode: crate::streak::StreakEvaluationMode,
    /// How many consecutive missed weeks turn the streak light red. `0`
    /// means "not yet configured," resolved to a real default at the point
    /// of use (`streak::resolve_streak_config`) — same blank-means-unset
    /// convention `pomodoro_work_minutes` (`settings.rs`) uses.
    #[serde(default)]
    pub streak_red_threshold_weeks: u32,
    /// One-line pitch/premise (Save the Cat-style logline), edited in the
    /// Metadata dock when the binder's root project row is selected — see
    /// `ui::metadata_panel::show_project`. Independent of `book_title`, which
    /// is reused as-is for the project's display name rather than duplicated
    /// here under a different key.
    #[serde(default)]
    pub logline: String,
    /// A single-line note on the story's thematic point/takeaway, same editing
    /// location as `logline` — unlike `logline`/`what_if`/`synopsis`, always a
    /// one-line field rather than a multiline box.
    #[serde(default)]
    pub point: String,
    /// Longer project-wide synopsis, same editing location as `logline`.
    #[serde(default)]
    pub synopsis: String,
    /// The story's inciting "what if" premise question (Save the Cat/Story
    /// Genius vocabulary), same editing location as `logline`. Deliberately
    /// separate from `protagonist_desire`/`protagonist_misbelief` (the
    /// Corkboard's scene-level Third Rail) — this is the project pitch, not
    /// the protagonist's arc.
    #[serde(default)]
    pub what_if: String,
}
