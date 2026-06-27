# Rust Media Center App — Full Roadmap

This roadmap focuses ONLY on the Rust application (no hardware, no deployment).

Goal: build a local-first media center with filesystem browsing, navigation UI, persistence, and video playback abstraction.

---

# Phase 1 — Core Domain Model (No UI)

## 1.1 Media types
- Define core structures:
  - Movie
  - Folder
  - MediaFile
  - Library

## 1.2 Tree model
- Define hierarchical model:
  - Node = File | Folder
- Support recursive structure
- Ensure no UI dependency

## 1.3 Metadata model
- File path
- Name
- Extension
- Size
- Optional future fields:
  - duration
  - thumbnail
  - codec info

## 1.4 Errors & Result system
- Define app-wide error type
- Wrap filesystem + IO errors

---

# Phase 2 — Filesystem Scanner

## 2.1 Directory traversal
- Implement recursive scan using `walkdir`
- Filter:
  - video files only (mp4, mkv, etc.)
- Ignore hidden/system folders

## 2.2 Library builder
- Convert filesystem → Media Tree
- Build:
  - Folder nodes
  - File nodes

## 2.3 Basic performance rules
- Avoid blocking recursion in UI layer
- Design scanner as independent module

## 2.4 Output (CLI/debug)
- Print structured tree
- Validate correctness of scan

---

# Phase 3 — Application Core State

## 3.1 App state machine
Define global states:
- Home
- Browsing
- Detail view
- Player (logical state only, no video yet)

## 3.2 Navigation stack
- Push/pop navigation
- Back behavior
- Current selection tracking

## 3.3 Event system
- Define internal events:
  - NavigateUp
  - NavigateDown
  - Select
  - Back
  - OpenFile

---

# Phase 4 — Input System

## 4.1 Input abstraction layer
- Map raw inputs → actions
- Do NOT expose key codes outside module

## 4.2 Action enum
```rust
enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    PlayPause,
}
## 4.3 Input pipeline
- Raw input → Action enum → Event system
- No OS-specific code in core logic
- Input is fully decoupled from UI + state

---

# Phase 5 — UI Layer (Minimal First)

## 5.1 UI principle
- UI is a pure function of state
- No business logic in UI layer
- UI only renders AppState

## 5.2 Screen system
- Home screen
- Folder browser
- Media detail view
- Player view (UI only, no decoding yet)

## 5.3 Rendering approach
- Start CLI-based or text UI
- Focus system (selected item highlight)
- Simple list rendering
- Scrollable view abstraction

## 5.4 UI state binding
- UI reads AppState snapshot
- Re-render on state change only
- No direct filesystem access

---

# Phase 6 — Persistence Layer (SQLite)

## 6.1 Data stored
- Favorites
- Watch history
- Resume position
- Cached metadata
- Last opened folder

## 6.2 Database schema
- movies
- folders
- history
- settings

## 6.3 Repository pattern
- MediaRepository trait
- SettingsRepository trait
- DB implementation hidden behind interfaces

## 6.4 Sync strategy
- Initial full scan → DB load
- Incremental updates later (optional optimization)

---

# Phase 7 — Library Indexing System

## 7.1 Index builder
- Convert filesystem tree → DB records
- Normalize file paths
- Deduplicate entries

## 7.2 Incremental updates
- Detect:
  - new files
  - removed files
  - renamed files (optional later)

## 7.3 In-memory cache
- Fast lookup index
- Avoid repeated disk scans
- DB as persistent source of truth

---

# Phase 8 — Playback Abstraction Layer

## 8.1 Player trait
```rust
trait Player {
    fn play(path: &str);
    fn pause();
    fn stop();
    fn seek(position: u64);
}

## 8.2 Playback state
- Track current media item
- Track playback position (seconds/ms)
- Track play/pause/stop state
- Optional: audio/subtitle selection state

## 8.3 Separation rule
- UI must NOT control decoding directly
- UI sends intent → Player handles execution
- Player is an isolated subsystem

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
- Control playback (play/pause/seek)
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