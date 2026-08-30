# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Canvas Desktop — a native (no Electron, no webview, no JS) Canva-like design
editor written in Rust, on `winit` + `wgpu` + `egui` + `vello`. Windows is the
primary target; macOS/Linux shell integration are real implementations (Linux
installs a `.desktop` entry; macOS generates an `Info.plist` and registers via
`lsregister`). Docs and code comments are in Spanish; keep new comments/UI
strings consistent with the surrounding language (UI strings are English,
comments are Spanish).

## Commands

```sh
# Run
cargo run -p canvas-app                              # welcome screen
cargo run -p canvas-app -- C:\path\to\photo.png       # open an image
cargo run -p canvas-app -- C:\path\to\folder          # open a gallery

# Verify (must be clean before considering a phase/task done)
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Cross-platform shell compile-check (from any OS; no linker needed)
cargo check -p canvas-shell --target x86_64-unknown-linux-gnu
cargo check -p canvas-shell --target x86_64-apple-darwin

# Headless GPU examples (real render, no window) — used to verify GPU-dependent
# behavior that unit tests can't cover
cargo run -p canvas-render --example bake_blur -- in.png out.png 20
cargo run -p canvas-render --example bake_filters -- ...
cargo run -p canvas-render --example save_roundtrip -- in.png out.png
cargo run -p canvas-render --example text_probe -- ...
cargo run -p canvas-render --example export_probe -- ...
cargo run -p canvas-shell --example instance_probe
cargo run -p canvas-shell --example registry_probe
```

There is no single-test shorthand beyond normal cargo filtering, e.g.
`cargo test -p canvas-core snap`. `crates/canvas-io/tests/kill_during_save.rs`
spawns a real child process and kills it mid-write to verify atomic save
survives a crash — don't "fix" it by mocking the filesystem.
`crates/canvas-render/tests/gpu_bake.rs` has 11 GPU integration tests
(marked `#[ignore]` — run with `cargo test -p canvas-render --test gpu_bake
-- --ignored`); they replicate `bake_blur` and `save_roundtrip` with synthetic
images and assertions on the baked pixels, catching GPU shader regressions
that CPU tests can't — including the texture-swap crash (a layer whose source
image changes size between bakes, e.g. pasting a photo over a blurred layer:
the whole fx texture set must be recreated, never re-uploaded in place).
`crates/canvas-shell/tests/integration.rs` tests cross-platform path
normalization (argv parsing, hidden-file detection, `ShellEvent`).
`crates/canvas-shell/src/single_instance.rs` has unit tests that use a
unique-per-PID socket name to avoid colliding with a real instance.

**Golden rule: every phase is verified by running the app, not
just by compiling.** For UI/interaction changes, actually run `cargo run -p
canvas-app` and exercise the feature — clippy/fmt/tests catch correctness,
not feature behavior.

CI (`.github/workflows/ci.yml`) runs fmt, clippy `-D warnings` and the test
suite on Windows/Linux/macOS, cross-compiles `canvas-shell` for
linux/darwin (no linker needed), and enforces the test-count floor.

## Opening a pull request

Iterative branches use `gh` (install: `brew install gh`; auth: `gh auth login`
— the git credential-helper token lacks the `read:org` scope `gh` requires,
so `gh auth login` is the one-time setup). DO NOT open a PR from `main`:

```sh
# From a feature branch (already pushed):
git branch --show-current          # verify this is NOT main
git push -u origin HEAD
gh pr create --title "type(scope): short summary" --body-file /tmp/body.md
```

Write the body to a file and pass `--body-file` (never multi-line markdown
inline — escaping bugs). Run `cargo fmt/clippy/test` first and fix failures
before opening.

## Architecture

Cargo workspace, five crates, dependencies flow one direction only
(`canvas-core` knows nothing about the others):

```
crates/
├─ canvas-core/     # Document/Page/Layer model + undo history. No UI, no OS. Has unit tests.
├─ canvas-render/   # Scene → vello; GPU blur/color-filter shaders (WGSL); offscreen bake + readback
├─ canvas-io/       # Load (EXIF-aware), atomic save (ReplaceFileW), sidecar, thumbnails, export
├─ canvas-shell/    # OS-open normalization → OpenPath; per-OS integration (all three real)
└─ canvas-app/      # `canvas-desktop` binary: eframe/egui + vello, Welcome/Gallery/Editor states
```

### Module layout conventions

Every crate is split by responsibility (the phase log lives in git history;
module doc comments carry the reasoning). Three conventions hold everywhere — follow them when
adding code:

- **≤ 400 lines of code per file, ≤ 80 per function.** Tests don't count
  toward it. Only two production files are over, on purpose, and say so in
  their doc comment: `app_icons.rs` (flat library of sibling icon draw
  functions) and `editor/properties_panel/layer_common.rs` (one cohesive
  form). The orchestrators (`app/views/editor/mod.rs`, `editor/canvas/mod.rs`,
  `scene/mod.rs`) stay under the target.
- **Zero `#[allow(clippy::too_many_arguments)]` in production code.** When a
  function accumulates too many parameters, group them into a struct
  (`CanvasContext`, `SaveContext`, `PaintGeometry`, `SyncLayerRequest`,
  `PassInput`, `RenderDims`, `PressGeometry`, `RenderRefs`, `SaveInput`,
  `GalleryOpOutcome`, `PaintPass`, etc.) instead of suppressing the lint.
  Zero `too_many_arguments` exists anywhere today; the few remaining
  `#[allow]`s are cfg-gates for Windows-only code plus one in
  `examples/verify_live_blur_update.rs`.
- **Tests live in a `tests.rs` next to the code they cover**, one per folder
  (`command/tests.rs`, `deck/tests.rs`, …), or in a `*_tests.rs` sibling
  wired with `#[path]` when the module is a directory. They're kept together
  on purpose: most cross several submodules of the same folder.
- **`impl` blocks are split across files freely.** Rust only requires an
  inherent `impl` to be in the same *crate* as the type, not the same module,
  so `impl Deck` lives in seven files with no traits and no wrappers.

**Visibility trap when moving code down a level.** `pub(super)` in
`editor/state.rs` meant "visible in `editor`". Once that file became
`editor/state/mod.rs` with submodules, `pub(super)` inside those submodules
means "visible in `state`" — narrower, and it breaks `canvas_view` /
`properties_panel`. Items moved down use `pub(in crate::editor)`, which
preserves the original reach. Same pattern in `slot_chrome/` and `gallery/`.

### canvas-core: document model

```
canvas-core/src/
├─ command/     # trait Command + one file per family
│  ├─ transform.rs    SetTransform, SetCrop, SetPageSize
│  ├─ appearance.rs   SetBlur/Shadow/Effects/Content/Opacity/Visible/Locked, Rename
│  ├─ structure.rs    InsertLayer, RemoveLayer, Reorder, Group, Ungroup
│  └─ history.rs      Composite + History
├─ document/    # mod.rs = Document, page.rs = Page + hit-testing,
│               # tree.rs = the preorder invariant and the only safe mutations
├─ geometry/    # align.rs, resize.rs, crop.rs — pure, no UI
└─ layer.rs  selection.rs  snap.rs  error.rs
```

- `Document` → `Page`s → `Layer`s (`layer.rs`): a layer's content is one of
  `ImageContent` / `TextContent` / `ShapeContent` / `SvgContent` /
  `GroupContent`.
- **Groups use `parent_id` + a preorder invariant**, not nested `Vec<Layer>`:
  a group's descendants occupy the contiguous span directly above its header
  in `Page::layers`. The renderer and SVG exporter walk that flat list with
  an index + a "subtree end" stack. Any tree mutation MUST go through
  `Page::move_subtree`/`insert_child` (`document/tree.rs`) — hand-rolling
  `Vec` splices breaks the invariant.
- **Undo/redo is the Command pattern** (`command/`, trait `Command` with
  `apply`/`revert`, driven by `History`). Continuous gestures (dragging a
  layer, moving a slider) do **not** push a command per frame: the UI mutates
  the document directly during the gesture and pushes one command with
  before/after state via `History::push_applied` when it ends — one drag =
  one undo step. Composite/multi-step operations (e.g. resize page + reflow
  background) group into a single `Composite` command.
- `Selection` (`selection.rs`) supports multi-select with a **primary**
  layer: the primary drives the properties panel and canvas gestures; the
  rest of the selection only participates in bulk ops (group, delete, copy).

### canvas-render

```
canvas-render/src/
├─ blur/    mod.rs (types) · params.rs · engine.rs (pipelines, lifecycle)
│           · passes.rs (effect passes) · sync.rs (layer synchronization) · *.wgsl
└─ scene/   mod.rs (build_scene) · document.rs (append_document) · raster.rs · text.rs · shape.rs
```

- `CanvasRenderer` wraps a vello `Renderer` sharing the **same wgpu
  device/queue as the window** (no separate GPU context). It paints to an
  `Rgba8Unorm` texture that egui displays via `register_texture` — no CPU
  readback for on-screen preview.
- GPU effects (blur, color filters) are non-destructive and chained: applied
  live for preview, only baked into pixels on save (`bake_*` examples
  exercise this path headlessly).
- **Resolution cap for GPU work.** Any image whose long side exceeds
  `blur::MAX_FX_DIM` (2048) is downscaled (CPU, `image::thumbnail`) before it
  enters the effect pipeline or vello's image atlas. Reason: vello's atlas is
  SQUARE with a hard 8192 cap and silently drops images that don't fit
  (`xy = None` in `resolve_pending_images`) — two 3072×4096 photos fill it and
  a third (or a bake sharing the atlas with the live deck) got dropped,
  flattening blurred backgrounds and losing the sharp layer on tall phone
  photos. The document keeps full-res pixels; only what the GPU
  processes/registers is capped, blur radii are scaled to the working
  resolution so N source pixels stay N source pixels, and effect-less big
  layers get a reduced display copy via the `display` cache in `BlurEngine`.
- Layer shadows use vello's `Scene::draw_blurred_rounded_rect` directly — no
  custom GPU shader for that one.
- Layers are clipped to the page rect both when rendering and when baking.
- **`vello::Scene` is a CPU-side encoding buffer**, so scene-building logic
  is unit-testable with no GPU: build the scene and read
  `scene.encoding().n_paths` / `n_open_clips` (`scene/tests.rs`).
- `append_document` walks the layer list with a manual index and closes
  opacity layers at the end of each turn. **Never `continue` out of its
  body** — that skips both the `i += 1` and the `pop_layer`. Missing textures
  go through `drawable_image() -> Option<_>` and an `if let` instead.
- **Hot path optimizations.** `CanvasSurface` owns a persistent `vello::Scene`
  reused across frames via `scene_mut()` (which calls `reset()`, not `new()`),
  avoiding a per-frame allocation. `append_document` pre-reserves its group
  stack with `Vec::with_capacity(8)` (typical group nesting depth ≤ 5).
  `sync_and_append` iterates `page.layers` directly instead of collecting
  into a throwaway `Vec`. `sync_layer` tracks `src_blob_id` (from
  `Blob::id()`) to detect when the source pixels changed and only re-uploads
  the GPU texture on actual change — editing an image without touching the
  blur slider no longer leaves stale pixels in the processed texture.

### canvas-io

```
canvas-io/src/
├─ export/   mod.rs (ExportFormat) · pdf.rs
│            └─ svg/  mod.rs (document_to_svg) · image.rs · text.rs · shape.rs · util.rs
├─ sidecar/  mod.rs · io.rs · paths.rs · payload.rs · trash.rs · container.rs
└─ load.rs  save.rs  metadata.rs  png_codec.rs  probe.rs  svg.rs  thumbs.rs  clipboard.rs
```

- Loading applies EXIF orientation (the `image` crate does not do this
  itself; `kamadak-exif` is used specifically for that — note its library
  name is `exif`, not `kamadak_exif`).
- Saving is atomic: write a temp file in the same directory, fsync, then
  `ReplaceFileW` on Windows.
- ICC profile and EXIF blocks are preserved byte-for-byte via `img-parts`
  (not `lcms2` — that's a color-conversion engine, not a block-preservation
  tool); `Orientation` is patched to 1 in place since pixels are saved
  already oriented.
- The sidecar (`foto.png.canvas`) stores the full editable document
  (layers + embedded pixels) next to the image; reopening the PNG restores
  editable layers instead of a flattened image. If the on-disk image changed
  independently of the sidecar, the app must detect and prompt.
- **Sidecar format v5 is a container** (`sidecar/container.rs`):
  `"CANVAS5"` magic + u64-LE JSON-header length + JSON header +
  `[u32-LE blob_len · PNG blob]*`. Pixels live as binary blobs indexed from
  the header — ~25 % smaller than the v1–v4 pure-JSON files (base64
  pixels), which are still read with no migration. Defense against hostile
  files: `.canvas` reads are capped at 512 MiB before allocation
  (`MAX_CANVAS_BYTES`), layer PNG decoding runs under `image::Limits`
  (`MAX_LAYER_PNG_DIM` / `MAX_LAYER_PNG_ALLOC`), and any corrupt container
  fails as a clean `IoError::Decode` — never a panic.
- SVG/GIF are load-only: `Ctrl+S` never overwrites them, it redirects to
  "Save as…".
- Export (`export/`) supports PNG/JPEG/SVG/PDF with scale. PDF goes through
  `svg2pdf` on top of **`resvg::usvg`** (not `svg2pdf`'s own usvg) so both
  share one usvg version — re-verify with `cargo tree -i usvg` after bumping
  `resvg`. Text is exported as real `<text>`/`<tspan>` elements (one tspan
  per line, using `canvas_render::text_lines` metrics), not rasterized or
  left to the SVG viewer to line-break.

### canvas-shell
- `ShellIntegration` trait normalizes OS-level "open" events into
  `OpenPath`. All three platform implementations are real:
  - **`windows.rs`**: ProgID `CanvasDesktop.Image`, `OpenWithProgids` per
    extension, folder context menu, `SHChangeNotify`.
  - **`linux.rs`**: installs a `.desktop` entry in
    `~/.local/share/applications/` (or `$XDG_DATA_HOME`) with `MimeType=`,
    deduplicated, and runs `update-desktop-database`. `unregister` removes
    the file.
  - **`macos.rs`**: creates a temporary `Canvas Desktop.app` bundle with
    `Contents/MacOS` and `Contents/Info.plist`, then registers the complete
    bundle via `lsregister`; `unregister` removes that bundle after calling
    `lsregister -u`. Production packaging should register the final bundle.
  - **`linux.rs`**: installs a `.desktop` entry under
    `~/.local/share/applications/` or `$XDG_DATA_HOME`, escapes special
    characters in `Exec=`, and removes the entry during unregister.
  Cross-compile verification: `cargo check -p canvas-shell --target
  x86_64-unknown-linux-gnu` / `x86_64-apple-darwin` from any OS.
- Single-instance enforcement (`single_instance.rs`, via `interprocess`): a
  second launch forwards its path(s) to the already-running primary over a
  local socket and exits with code 0 rather than opening a second window
  (dev escape: `CANVAS_DESKTOP_MULTI_INSTANCE=1` runs standalone instead).
  `acquire_instance_with_name` takes a socket name parameter (used by tests
  with a unique-per-PID name); `acquire_instance` uses the production
  constant. `accept_one_sync` accepts one connection synchronously (test
  helper).

### canvas-app

```
canvas-app/src/
├─ main.rs        # entry point ONLY: logging, --register-shell flags, single
│                 # instance, eframe launch. The App itself lives in app/.
├─ lock.rs        # lock_ok(): Mutex locking with poisoning recovery — never
│                 # `lock().unwrap()` on shared state
├─ app/
│  ├─ mod.rs      # struct App, View, Nav, eframe::App::update; shared state
│  │              # lives in AppInner (Arc<Mutex>) + one Workspace per window
│  ├─ bootstrap.rs            # App construction: fonts, renderer, menus,
│  │                          # single-instance listener, root workspace
│  ├─ workspace.rs            # Workspace: ALL state of one native window
│  ├─ workspace_lifecycle.rs  # create/close/focus windows + persistence
│  ├─ ws_frame.rs             # the per-window UI frame (root and children)
│  ├─ switcher.rs             # Ctrl+Tab / Ctrl+` window switcher
│  ├─ frame.rs    # EditorFrame<'a>: the borrows the editor view needs
│  ├─ messages/   # mod.rs = the rx loop + one-line dispatch per AppMsg;
│  │              # load/save/export/gallery/document/shell/unsplash.rs = bodies
│  ├─ views/      # welcome.rs · loading.rs · gallery.rs
│  │  └─ editor/  # mod.rs = orchestration only; save_flow · file_ops ·
│  │              # modals · panels · deck_nav
│  └─ menu_actions.rs  navigation.rs  persistence.rs  ui_menu.rs
│     ui_modals.rs  window.rs
├─ deck/          # mod.rs (Deck) · model.rs · geometry.rs · layout.rs
│                 # cache.rs (eviction budget) · loading.rs (scheduler,
│                 #               max_inflight_loads() = dynamic by core count)
│                 # scan.rs (disk sync) · nav.rs
├─ editor/
│  ├─ canvas/     # mod.rs = canvas_ui, orchestration only; layout · context_menu ·
│  │              # picking · camera · paint · url_popup
│  ├─ state/      # mod.rs (EditorState) · constructors · layer_factory ·
│  │              # background · shortcuts · history · sidecar
│  ├─ slot_chrome/  mod.rs · header.rs · icons.rs
│  ├─ properties_panel/  mod.rs · layer_common · content{,_shape,_text} ·
│  │                     effects · page
│  └─ interaction.rs  layer_ops.rs  overlay.rs  viewport.rs
├─ gallery/          mod.rs · item.rs · ui/{mod,cell,shell,gallery_view}.rs
│                 # ui/folder_panel/{mod,rows,content}.rs
├─ layers_panel/  mod.rs · tab_strip.rs · tab_draw.rs · insert.rs ·
│                 # ops.rs · row.rs
├─ menus/         mod.rs · fallback.rs (non-Windows) · native/{mod,build}.rs
├─ unsplash/      mod.rs · types.rs · api.rs (Authorization header, download
│                 # cap) · state.rs · panel.rs · card.rs
├─ loader/        # the off-thread disk work, one file per op family:
│                 # load_ops · save_ops · export_ops · gallery_ops ·
│                 # image_import · file_ops · unsplash_ops
├─ app_icons.rs   # hand-drawn egui iconography (over 400 on purpose)
└─ clipboard.rs  crash_log.rs  deck_strip.rs  export.rs  paste_hook.rs
   sidebar.rs  surface.rs  watcher.rs  welcome.rs  settings/{mod,choices,sort}.rs
```

- The app is **multi-window**: `App` owns an `AppInner` (shared state under
  `Arc<Mutex<>>`, locked with `lock_ok()` — poisoning recovery, not a
  panic) and one `Workspace` per native window. Each workspace carries its
  own `View` (`Welcome` / `Loading` / `Gallery` / `Editor`), deck, watcher
  and save/export flows, and its own `mpsc` channel into `AppMsg` — no
  routing by window id. Disk work (load, save, thumbnails, watcher)
  happens off the UI thread. `main.rs` is just the entry point.
- `App`'s fields are grouped by domain into `SaveFlow` / `ExportFlow` /
  `DeckOps` / `MenuMirror` rather than sitting flat. `SaveFlow` and
  `ExportFlow` are passed directly to `overwrite_modal_ui` / `export_flow_ui`
  instead of unpacking their fields.
- **Parameter-grouping structs are the convention for long signatures.**
  Instead of `#[allow(too_many_arguments)]`, extract a struct: `CanvasContext`
  (canvas_ui), `SaveContext` (start_save/start_save_design), `PaintGeometry`
  (paint), `PressGeometry` (handle_press), `SaveInput` (spawn_save),
  `SyncLayerRequest` + `PassInput` (blur passes), `RenderDims`
  (render_with_base), `RenderRefs` (sync_and_append).
- **`EditorFrame` (`app/frame.rs`) exists because `&mut App` is impossible
  there**: `editor_view_ui` runs while `state` is borrowed out of
  `self.view`, so it takes independent `&'a mut` borrows of the *other*
  fields. Add a field here instead of adding a parameter.
- **Loader errors are typed**: loader ops return `Result<_, IoError>` /
  `UnsplashError` (thiserror) and `AppMsg` carries them as-is; the UI turns
  them into user-facing messages. No `String` error plumbing. The Unsplash
  access key travels in the `Authorization` header — never in the query
  string — and image downloads share one bounded client (`http::get_bytes_bounded`,
  agent + limit in `app/http.rs`) used by both Unsplash and URL replacement,
  each mapping `HttpError` to its own error type.
- **`app/views/editor/mod.rs` and `editor/canvas/mod.rs` are orchestration
  only, and the order in which they call their submodules is significant** —
  the code comments say so explicitly (e.g. placeholder materialization must
  run after `handle_messages` and before the save block). Don't reorder when
  editing them.
- Navigation that might interrupt an in-flight save is deferred (`Nav`
  enum + `after_save`) rather than mutating `view` mid-borrow.
- `watcher.rs` uses `notify` to detect external changes to the open file and
  shows a "Reload / Keep mine" banner; the app's own saves must not
  self-trigger it.
- **Deleting layers has exactly one implementation**:
  `editor::delete_selected` (`editor/layer_ops.rs`). Menu, context menu,
  `Delete`, `Ctrl+X` and the layers-panel button all call it, and it skips
  `effective_locked` layers. Don't add a second one.

## Fixed dependency-version decisions (do not bump one without the other)

- **eframe 0.35 + vello 0.9 share wgpu 29.** Bumping either requires
  revalidating the pair.
- **parley 0.11 shares peniko 0.6 with vello 0.9** for text. After bumping
  vello, re-check with `cargo tree -i peniko` and run the `text_probe`
  example.
- **svg2pdf 0.13 / resvg 0.45 must share one `usvg` in the tree** — check
  with `cargo tree -i usvg` after touching either.
- `arboard`'s `image-data` feature is pinned to match what `egui-winit`
  already pulls in transitively, to avoid duplicating the crate.
- **Test-count guard in CI** (`.github/workflows/ci.yml`): the `test` job
  fails if `cargo test --workspace -- --list` lists fewer than 320 tests.
  The floor sits well below the real total (~430 listed on Windows)
  because tests are cfg-gated per OS — it exists to catch wholesale test
  deletions, not to be an exact checkpoint. The 11 GPU-only `#[ignore]`
  tests in `crates/canvas-render/tests/gpu_bake.rs` are outside it.

## Where the reasoning lives

There is no separate PLAN/REFACTOR document: the *why* of each module
boundary lives in the module doc comments (every split file says what it
owns and what its neighbors own) and in git history. Read the sibling doc
comments before undoing a module boundary or re-litigating an architectural
choice — e.g. why groups use `parent_id` instead of nested layers
(`canvas-core/src/document/tree.rs`), why the tab strip decides clicks by
geometry instead of egui drag-and-drop (`layers_panel/tab_strip.rs`), or
why crop is "trim at the edges" rather than destructive
(`canvas-core/src/geometry/crop.rs`).
