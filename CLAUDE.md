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
`crates/canvas-render/tests/gpu_bake.rs` has 7 GPU integration tests
(marked `#[ignore]` — run with `cargo test -p canvas-render --test gpu_bake
-- --ignored`); they replicate `bake_blur` and `save_roundtrip` with synthetic
images and assertions on the baked pixels, catching GPU shader regressions
that CPU tests can't.
`crates/canvas-shell/tests/integration.rs` tests cross-platform path
normalization (argv parsing, hidden-file detection, `ShellEvent`).
`crates/canvas-shell/src/single_instance.rs` has unit tests that use a
unique-per-PID socket name to avoid colliding with a real instance.

**Golden rule (from PLAN.md): every phase is verified by running the app, not
just by compiling.** For UI/interaction changes, actually run `cargo run -p
canvas-app` and exercise the feature — clippy/fmt/tests catch correctness,
not feature behavior.

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

Every crate was split by responsibility (see `REFACTOR.md` for the phase log
and the reasoning). Three conventions hold everywhere — follow them when
adding code:

- **≤ 400 lines of code per file, ≤ 80 per function.** Tests don't count
  toward it. Two files are over on purpose and say so in their doc comment:
  `scene/mod.rs::append_document` and the orchestrators below.
- **Zero `#[allow(clippy::too_many_arguments)]` in production code.** When a
  function accumulates too many parameters, group them into a struct
  (`CanvasContext`, `SaveContext`, `PaintGeometry`, `SyncLayerRequest`,
  `PassInput`, `RenderDims`, `PressGeometry`, `RenderRefs`, `SaveInput`,
  etc.) instead of suppressing the lint. The only remaining `#[allow]` is in
  `examples/verify_live_blur_update.rs`, not in crate code.
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
│           · passes.rs (the color → blur-H → blur-V chain) · *.wgsl
└─ scene/   mod.rs (build_scene, append_document) · raster.rs · text.rs · shape.rs
```

- `CanvasRenderer` wraps a vello `Renderer` sharing the **same wgpu
  device/queue as the window** (no separate GPU context). It paints to an
  `Rgba8Unorm` texture that egui displays via `register_texture` — no CPU
  readback for on-screen preview.
- GPU effects (blur, color filters) are non-destructive and chained: applied
  live for preview, only baked into pixels on save (`bake_*` examples
  exercise this path headlessly).
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
├─ sidecar/  mod.rs · io.rs · paths.rs · payload.rs · trash.rs
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
  - **`macos.rs`**: generates an `Info.plist` with `CFBundleDocumentTypes`
    + `UTExportedTypeDeclarations` in a temp dir and registers it via
    `lsregister`. `unregister` calls `lsregister -u`.
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
├─ app/
│  ├─ mod.rs      # struct App, View, Nav, and eframe::App::update
│  ├─ frame.rs    # EditorFrame<'a>: the 11 borrows the editor view needs
│  ├─ messages/   # mod.rs = the rx loop + one-line dispatch per AppMsg;
│  │              # load/save/export/gallery/document/shell.rs = the bodies
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
│  ├─ canvas/     # mod.rs = canvas_ui, orchestration only; context_menu ·
│  │              # picking · camera · paint · url_popup
│  ├─ state/      # mod.rs (EditorState) · constructors · layer_factory ·
│  │              # background · shortcuts · history · sidecar
│  ├─ slot_chrome/  mod.rs · header.rs · icons.rs
│  ├─ properties_panel/  mod.rs · layer_common · content{,_shape,_text} ·
│  │                     effects · page
│  └─ interaction.rs  layer_ops.rs  overlay.rs  viewport.rs
├─ gallery/       mod.rs + ui/{mod,cell,folder_panel,shell}.rs
├─ layers_panel/  mod.rs · ops.rs · row.rs
├─ menus/         mod.rs · fallback.rs (non-Windows) · native/{mod,build}.rs
├─ loader/        # the off-thread disk work: load/save/export/gallery ops
└─ clipboard.rs  deck_strip.rs  export.rs  paste_hook.rs  settings.rs
   sidebar.rs  surface.rs  watcher.rs  welcome.rs
```

- `app/mod.rs` owns the `App` (eframe) state machine: `View` is one of
  `Welcome` / `Loading` / `Gallery` / `Editor`. Disk work (load, save,
  thumbnails, watcher) happens off the UI thread; results come back over
  `std::sync::mpsc` channels into `AppMsg`. `main.rs` is just the entry
  point.
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
- **Test count: 299** (`cargo test --workspace`, excluding 7 GPU-only
  `#[ignore]` tests in `crates/canvas-render/tests/gpu_bake.rs`). The count
  is a sanity checkpoint — a drop means a regression in test coverage.

## Where the plan lives

`REFACTOR.md` is the companion for the *structural* decisions: the phase log
of the split into modules, why a few files are deliberately over the size
target, the known-bug notes found along the way, and the post-refactor
optimization log (parameter-grouping structs, hot-path render optimizations,
shell implementations, test additions). Read it before undoing a module
boundary or re-litigating an architectural choice (e.g. why groups use
`parent_id` instead of nested layers, why crop is "trim at the edges" rather
than destructive).
