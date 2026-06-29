# Rust Media Center App — Full Roadmap

This roadmap focuses ONLY on the Rust application (no hardware, no deployment).

Goal: build a local-first media center with filesystem browsing, navigation UI, persistence, and video playback abstraction.

---

# Phase 1 — Core Domain Model ✅

## 1.1 Types (`src/structs/`)

The domain model is a filesystem tree, not a separate “movie library” schema:

| Type                           | Role                                                                                |
| ------------------------------ | ----------------------------------------------------------------------------------- |
| `FolderNode`                   | Directory node: path, name, `subfolders`, `files`, and aggregated `reduced_*` stats |
| `FileNode`                     | Video file node: path, name, `format` (uppercase extension), optional `metadata`    |
| `FileMetadata`                 | Scan-time fields: `size`, `duration_ms`, `bitrate`, `codec`                         |
| `StorageDevice` / `DeviceType` | Defined for future hardware integration; **not wired yet**                          |

There is no `Movie`, `Library`, or `MediaFile` type — volumes and folders map directly to `FolderNode`.

## 1.2 Tree model

- Hierarchy: `FolderNode` → nested `FolderNode`s + `FileNode`s.
- The in-memory library root is a hidden `FolderNode` whose `subfolders` are volumes (`volumeD`, `volumeE`, …).
- Pure Rust structs with no UI or Slint dependency.

## 1.3 Metadata

Populated at scan time in `app/browser/`:

- Always: path, name, extension (`format`), file size.
- When `ffprobe` is available (`app/probe/`): duration, bitrate, codec.
- Folder nodes also carry rolled-up totals: file count, total size, total duration (`reduced_*`).

Not implemented yet: thumbnails, lazy/on-demand probing beyond scan.

## 1.4 Errors

**Not implemented.** Scanner panics on missing workspace root; probe and IO failures degrade gracefully (`None` metadata, skipped entries). A unified `AppError` is deferred to later phases.

---

# Phase 2 — Filesystem Scanner ✅

## 2.1 Module layout (`src/app/browser/`)

- `WORKSPACE` — compile-time project root (`env!("CARGO_MANIFEST_DIR")`).
- `build_tree(root)` — recursive scan of one directory tree via `walkdir`.
- `build_volume_library(workspace)` — discovers top-level `volume*` folders, builds each with `build_tree`, returns the hidden library root.

### Traversal rules

- **Videos only** — extensions: mkv, mp4, avi, mov, webm, m4v, wmv, flv, mpg, mpeg, ts, m2ts, vob, ogv, 3gp (case-insensitive).
- **All directories** under a volume become `FolderNode`s, including empty ones.
- Hidden/system folder filtering is **not** implemented yet.

### Scan algorithm (`build_tree`)

1. Register every directory in a flat `HashMap`.
2. Walk files, filter to videos, run `probe_video`, attach `FileNode` to parent.
3. Nest folders deepest-first into a tree.
4. `compute_reduced_stats` — post-order aggregation on every folder.

## 2.2 Live refresh (`src/watch/` + `app::wire_library_refresh`)

- `notify` watches the whole workspace recursively.
- Changes are debounced (800 ms quiet period), then `build_volume_library` runs on a **background thread**.
- Result is applied on the UI thread via `BrowsingState::reload_tree`.
- Debug logging: `[watch]` / `[refresh]` prefixes in debug builds (`src/debug/`).

## 2.3 Performance

- Initial scan runs at startup in `main` (blocking, before the window opens).
- Rescans never block the Slint event loop.
- Full-library rescan on every change batch (no incremental diff yet).

## 2.4 Debug output (`src/debug/`)

- `print_folder(&FolderNode)` — ASCII tree to stdout (optional in `main`).
- `refresh` / `watch` — stderr traces for the rescan pipeline.

Unit tests in `browser`, `probe`, `utils`, `browsing`, and `watch`.

---

# Phase 3 — Browsing State ✅

## 3.1 Scope (`src/app/browsing/`)

Implemented as **folder browsing only** — no global screen enum yet.

`BrowsingState` holds:

| Field      | Purpose                                                             |
| ---------- | ------------------------------------------------------------------- |
| `tree`     | Current `FolderNode` library snapshot                               |
| `stack`    | Open path (`Vec<PathBuf>`) — volumes and folders entered via “open” |
| `selected` | Index into the visible flat list                                    |
| `visible`  | Cached `Vec<TreeItem>` for the UI                                   |

Not implemented: Home / Detail / Player screens, file-open action, playback state.

## 3.2 Navigation

| Method              | Behavior                                                                                             |
| ------------------- | ---------------------------------------------------------------------------------------------------- |
| `go_up` / `go_down` | Move selection in the flat list                                                                      |
| `open_selected`     | Expand a volume/folder (push onto `stack`) or collapse if already open                               |
| `go_back`           | Pop `stack`, rebuild list, re-select the closed folder                                               |
| `reload_tree`       | Replace library after rescan; prune invalid stack entries; preserve selection when path still exists |

## 3.3 Visible list projection (`src/utils/`)

`flatten_along_path(library, stack, out)` converts the tree into a flat `TreeItem` list:

- Empty stack → volume rows only.
- Non-empty stack → expand along the open path; deepest folder shows its children (subfolders + files).
- `volume_to_tree_item` / `folder_to_tree_item` / `file_to_tree_item` + formatters for size, duration, bitrate.

There is **no Rust event enum** — navigation is direct method calls from the app wiring layer.

---

# Phase 4 — Input ✅ (UI-embedded)

## 4.1 Current approach

Input is handled in **Slint** (`src/ui/app.slint`), not a separate Rust `Action` module.

`MainWindow` callbacks (wired in `app::wire_up`):

| Callback                | Keys       | Rust handler                       |
| ----------------------- | ---------- | ---------------------------------- |
| `move_selection(delta)` | ↑ / ↓      | `BrowsingState::go_up` / `go_down` |
| `open_selected()`       | → / Return | `BrowsingState::open_selected`     |
| `navigate_back()`       | ← / Escape | `BrowsingState::go_back`           |
| `cycle_theme()`         | T          | `theme::apply_palette_by_index`    |
| `toggle_help()`         | H          | toggles help overlay               |

## 4.2 Not implemented

- Rust `Action` enum and input abstraction layer.
- Left/right beyond back/open, PlayPause, gamepad, or network input.
- Decoupled input pipeline (raw input → action → state).

Future work: extract Slint key bindings into `src/app/input/` and route through a shared action type (see Phase 9).

---

# Phase 5 — UI Layer ✅

## 5.1 Stack

- **Slint 1.17** — `src/ui/` (`.slint` markup + `build.rs` codegen).
- **Rust glue** — `src/app/mod.rs` (`wire_up`, `sync_window`, `with_browsing`).
- UI is a snapshot of `BrowsingState`; no filesystem or scan logic in `.slint` files.

## 5.2 Main window (`app.slint`)

Single `MainWindow` with:

- Scrollable virtualized tree (`ScrollView` + `TreeRow` per `TreeItem`).
- Selection highlight and auto-scroll-to-selection.
- Context panel for the selected item: `VolumeInfos`, `FolderInfos`, or `FileInfos`.
- Help overlay (`help/help.slint`), toggled with H.
- CRT-style theme via `theme/` palettes (T cycles).

Row components: `volume_row`, `folder_row`, `file_row` (via `tree_row` dispatcher), `space_row`.

## 5.3 Icons (`src/icons/` + `icon.slint`)

Font Awesome SVGs loaded on demand through the `IconLoader` global (`wire_icons`).

## 5.4 State binding

On every browsing mutation:

1. `with_browsing` runs the state change.
2. `sync_window` pushes `visible_items()` → `tree` property and `selected` → `selected_index`.
3. Slint re-renders from properties only.

`MainWindow` properties: `tree`, `selected_index`, `scroll_offset`, `help_visible`.

## 5.5 Not implemented

- Separate screens (home, player, media detail as full-page views).
- CLI / text-mode UI (debug tree print only).
- Opening / playing a file from the UI.

---

# Phase 6 — Persistence Layer (SQLite)

User-specific state lives in SQLite (`src/db/`). The filesystem tree from `build_volume_library` remains the **source of truth for what exists**; the database only stores per-path preferences and playback progress.

## 6.1 Data stored

| Table         | Purpose                                                      |
| ------------- | ------------------------------------------------------------ |
| `media_state` | Per-file user state: favorite, resume position, last watched |
| `settings`    | App preferences (e.g. last opened folder)                    |

`media_state` rows are keyed by **absolute file path** and apply only to video files present in the scanned tree.

## 6.2 Database schema (`src/db/sql/init.sql`)

```sql
CREATE TABLE media_state (
    path TEXT PRIMARY KEY,
    favorite INTEGER NOT NULL DEFAULT 0,
    resume_position_ms INTEGER,
    last_watched_at INTEGER
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

- `media_state.path` — canonical absolute path of a `FileNode` in the library tree.
- `favorite` — `0` / `1`.
- `resume_position_ms` / `last_watched_at` — optional; `NULL` when unset.
- `settings` — key/value store (e.g. `last_opened_folder` → JSON path string).

No separate `movies`, `folders`, or `history` tables — library structure is not duplicated in SQL.

## 6.3 Repository pattern (`src/db/mod.rs`)

- `MediaState` / `Settings` — Rust structs mirroring persisted fields.
- `MediaStateRepository` — `get`, `save` per path.
- `SettingsRepository` — `get_last_opened_folder`, `set_last_opened_folder`.

SQLite access stays behind these traits; callers in `app/` do not run raw SQL.

## 6.4 Sync strategy — tree rebuild reconciliation

On **every** library tree rebuild (startup scan and each debounced rescan from `watch`):

1. **Build tree** — `build_volume_library` produces the current `FolderNode` snapshot (filesystem is authoritative).
2. **Collect live paths** — walk the tree and gather the set of all `FileNode.path` values (absolute, normalized).
3. **Prune stale `media_state` rows** — delete any row whose `path` is **not** in that set:

   ```sql
   DELETE FROM media_state
   WHERE path NOT IN (...paths from current tree...);
   ```

   Removed, moved, or renamed files must not leave orphan favorites or resume positions. Renames are treated as delete + new file (old path pruned; new path gets a fresh row in step 4).

4. **Seed missing `media_state` rows** — for every path in the current tree that has **no** row yet, insert a default record:

   ```sql
   INSERT INTO media_state (path, favorite, resume_position_ms, last_watched_at)
   VALUES (?, 0, NULL, NULL);
   ```

   Run for each `FileNode.path` not already present (batch `INSERT` or `INSERT OR IGNORE` after path collection). New files discovered on rescan therefore always have a `media_state` row before the UI or player needs one.

5. **Read / update on use** — load row by path for display (favorite icon, resume) and on user or player events (`save` on favorite toggle, periodic resume writes, etc.). Rebuild reconciliation only prunes and seeds; it does not overwrite existing `favorite`, `resume_position_ms`, or `last_watched_at` for paths that remain in the tree.

`settings` is **not** pruned or seeded by tree rebuild (app-level, not per-file).

### Integration point

Hook steps 3–4 into the same pipeline as `BrowsingState::reload_tree` — immediately after a successful `build_volume_library` on the background thread (before or when applying the new tree to UI state), so DB rows match the current file set: **no orphans, no gaps**.

### Not in scope yet

- Incremental diff scan (full tree walk for path set is enough initially).
- Migrating `media_state` across renames (old row deleted, new row seeded with defaults).
- Caching the full library in SQL (Phase 7).

---

# Phase 7 — Library Indexing System

## 7.1 Role

The in-memory `FolderNode` tree built by `app/browser/` is the library index. SQLite (`media_state`) holds **user overlay** only, reconciled on each rebuild: prune rows for missing files, seed rows for new files (Phase 6.4).

## 7.2 Path normalization

- Store and compare paths in one canonical form (absolute, consistent separators) before prune `NOT IN` checks.
- Deduplicate by using `path` as the primary key in `media_state`.

## 7.3 Incremental updates (optional later)

- Detect new / removed / renamed files without a full rescan.
- Until then, full rescan + `media_state` prune on each rebuild is sufficient.

## 7.4 In-memory cache

- `BrowsingState.tree` is the hot path for navigation.
- Load `media_state` rows on demand for visible or selected files; no need to mirror the whole tree in RAM.

---

# Phase 8 — Playback Abstraction Layer

Isolated video playback subsystem. The UI and browsing layer send **intents**; the player owns decoding, output, and track selection. No Slint or filesystem code inside the player module.

## 8.1 Required capabilities

| Capability | Description |
|------------|-------------|
| **Play** | Start playback of a file path (from cold or from `resume_position_ms` in `media_state`). |
| **Pause** | Pause decoding/output; position is retained. |
| **Resume** | Continue from paused position (toggle with pause, or explicit `play` while paused). |
| **Seek relative** | Jump **forward or backward by N milliseconds** from the current position (`seek_delta_ms(±N)`). Used for skip buttons and scrub shortcuts. |
| **Cycle audio track** | Switch to the next audio stream in the file; wrap from last → first. No-op or disabled when only one track exists. |
| **Cycle subtitle track** | Switch to the next subtitle stream; include an **off** state (no subtitles) in the cycle. Wrap from last → off → first. No-op when no subtitle streams exist. |

Absolute seek (`seek_to_ms(position)`) is also required for resume and progress bar use, but relative jumps are a first-class API surface.

## 8.2 Player interface (sketch)

```rust
enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

struct TrackInfo {
    index: u32,
    label: String,   // e.g. "eng", "fra 5.1"
    language: Option<String>,
}

struct PlaybackState {
    path: Option<PathBuf>,
    status: PlaybackStatus,
    position_ms: u64,
    duration_ms: Option<u64>,
    audio_tracks: Vec<TrackInfo>,
    subtitle_tracks: Vec<TrackInfo>,
    selected_audio: u32,
    selected_subtitle: Option<u32>,  // None = subtitles off
}

trait Player {
    fn open(&mut self, path: &Path) -> Result<()>;
    fn play(&mut self) -> Result<()>;
    fn pause(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;

    fn seek_to_ms(&mut self, position_ms: u64) -> Result<()>;
    fn seek_delta_ms(&mut self, delta_ms: i64) -> Result<()>;  // +N forward, −N back

    fn cycle_audio_track(&mut self) -> Result<()>;
    fn cycle_subtitle_track(&mut self) -> Result<()>;

    fn state(&self) -> &PlaybackState;
}
```

Concrete backend (e.g. libmpv, GStreamer, ffplay child process) lives behind this trait in `src/app/player/` or similar.

## 8.3 Track discovery

- On `open`, query the container for available audio and subtitle streams.
- Populate `audio_tracks` / `subtitle_tracks` before or at start of playback.
- Persist **selected** audio/subtitle indices in memory for the session; optional future extension to `media_state` or `settings` per path.

## 8.4 Input mapping (future)

Playback actions are routed through the same pipeline as browsing (Phase 4), e.g.:

| Action | Typical binding |
|--------|-----------------|
| Play / Pause | Space or dedicated key |
| Seek +10 s / −10 s | configurable `seek_delta_ms(±10_000)` |
| Cycle audio | A |
| Cycle subtitle | S |

Exact keys TBD; bindings must call `Player` methods only, never the backend directly.

## 8.5 Persistence integration

- On pause / stop / periodic tick: write `resume_position_ms` and `last_watched_at` to `media_state` for the current path.
- On `open`: read `resume_position_ms` and call `seek_to_ms` when present.
- Pruned paths (Phase 6.4) drop stored resume data automatically.

## 8.6 Separation rules

- UI renders `PlaybackState` (position, track labels, play/pause icon); it does not invoke ffmpeg/mpv APIs.
- Player runs off the UI thread; position updates pushed to Slint via the event loop.
- Browsing and player are separate modes or layered UI: entering player from a selected `FileNode` calls `Player::open`; back returns to `BrowsingState` without tearing down the library tree.

## 8.7 Not in scope yet

- Video output surface embedded in Slint (may use separate window or platform view initially).
- Subtitle styling / ASS rendering beyond backend defaults.
- Remembering last audio/subtitle choice per file in SQLite.

---

# Phase 9 — Network Layer (Optional)

## 9.1 Local API server

- Embedded HTTP or WebSocket server
- Runs inside the same Rust process
- No dependency on external services

## 9.2 Remote control pipeline

- Network request → Action enum
- Same pipeline as keyboard input
- Fully unified input system

## 9.3 API capabilities (future)

- List library
- Get metadata
- Control playback: play, pause, seek, seek ±N ms, cycle audio, cycle subtitles
- Navigate folders remotely

---

# Phase 10 — Full System Integration

## 10.1 Module integration map

- Input → Core → State → UI → Player → Storage
- Strict unidirectional data flow

## 10.2 Main event loop

- Event-driven architecture
- No blocking operations in UI thread
- All IO moved to background tasks

## 10.3 Concurrency model

- UI thread (render + state view)
- IO thread (filesystem + DB)
- Player thread (optional isolation)
- Communication via message passing

---

# Phase 11 — Performance & UX Layer

## 11.1 Performance goals

- Instant navigation (<50ms perception)
- No UI freeze during scanning
- Cached library access by default

## 11.2 UX consistency rules

- Stable focus navigation rules
- Predictable back behavior
- No state desync between screens

## 11.3 Error handling strategy

- Graceful fallback screens:
  - Empty library
  - Missing file
  - Scan failure
- Never crash UI loop

---

# Final Architecture Summary

```text id="final_arch"
Input System
    ↓
App Core (State Machine)
    ↓
UI Renderer
    ↓
Library + SQLite Layer
    ↓
Filesystem Scanner
    ↓
Playback Engine (Abstracted)

## Final Product Definition

# A single Rust application that:

- scans and indexes media files
- builds a structured library
- provides smooth navigation UI
- stores user state persistently
- plays media via a backend player abstraction
- supports optional remote control via network
- runs as a fully self-contained media center system

If you want next step, I can :contentReference[oaicite:0]{index=0} so you can literally just execute it like a
```
