import BinderTree from "./BinderTree.svelte";
import CorkboardView from "./CorkboardView.svelte";
import StoryGridView from "./StoryGridView.svelte";
import EditorPanel from "./panels/EditorPanel.svelte";
import PreviewPanel from "./panels/PreviewPanel.svelte";
import PomodoroPanel from "./panels/PomodoroPanel.svelte";
import CollabPanel from "./panels/CollabPanel.svelte";
import SyncPanel from "./panels/SyncPanel.svelte";
import BacklinksPanel from "./panels/BacklinksPanel.svelte";
import TagsPanel from "./panels/TagsPanel.svelte";
import MetadataPanel from "./panels/MetadataPanel.svelte";
import BeliefTimelinePanel from "./panels/BeliefTimelinePanel.svelte";
import WordCountPanel from "./panels/WordCountPanel.svelte";
import StreakPanel from "./panels/StreakPanel.svelte";
import BookmarksPanel from "./panels/BookmarksPanel.svelte";
import NotesPanel from "./panels/NotesPanel.svelte";
import DashboardPanel from "./panels/DashboardPanel.svelte";
import VersionActivityPanel from "./panels/VersionActivityPanel.svelte";

// Phase 9: pulled out of DockLayout.svelte so MobileShell.svelte (the
// single-panel-at-a-time mobile navigation DockLayout's dockview-based
// split/float UI has no business existing on — see the migration plan's
// Phase 9 row) can share the same panel set without duplicating this list
// and risking the two navigation modes drifting apart as panels are added.
//
// Every tab the real app's DockTab enum has (app/src/app/dock.rs), mapped
// to its title. All of them have real content as of Phase 7 (Settings and
// Daily Notes aren't DockTab entries in the real app either — see
// DockLayout.svelte's longer note on why both stay out of this registry).
export const PANEL_TITLES: Record<string, string> = {
  binder: "Binder",
  editor: "Editor",
  preview: "Preview",
  corkboard: "Corkboard",
  storygrid: "Story Grid",
  pomodoro: "Pomodoro",
  backlinks: "Backlinks",
  tags: "Tags",
  metadata: "Metadata",
  belieftimeline: "Belief Timeline",
  wordcount: "Word Count",
  collab: "Collab",
  sync: "Sync",
  streak: "Streak",
  bookmarks: "Bookmarks",
  notes: "Notes",
  dashboard: "Dashboard",
  versionactivity: "Version Activity",
};

export const PANEL_COMPONENTS: Record<string, any> = {
  binder: BinderTree,
  editor: EditorPanel,
  preview: PreviewPanel,
  corkboard: CorkboardView,
  storygrid: StoryGridView,
  pomodoro: PomodoroPanel,
  collab: CollabPanel,
  sync: SyncPanel,
  backlinks: BacklinksPanel,
  tags: TagsPanel,
  metadata: MetadataPanel,
  belieftimeline: BeliefTimelinePanel,
  wordcount: WordCountPanel,
  streak: StreakPanel,
  bookmarks: BookmarksPanel,
  notes: NotesPanel,
  dashboard: DashboardPanel,
  versionactivity: VersionActivityPanel,
};

// The subset shown in MobileShell's primary tab bar — the rest are one tap
// away behind "More" rather than all 18 crammed into a phone-width bar.
// Picked for what a mobile writing session actually needs first: find a
// document, write, check it rendered, see the story structure.
export const MOBILE_PRIMARY_PANELS = ["binder", "editor", "preview", "corkboard", "storygrid"];
