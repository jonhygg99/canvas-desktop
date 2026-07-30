# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Canvas Desktop — a native (no Electron, no webview, no JS) Canva-like design
editor written in Rust, on `winit` + `wgpu` + `egui` + `vello`. Windows is the
primary target; macOS/Linux shell integration are compilable stubs for now.
Docs and code comments are in Spanish; keep new comments/UI strings
consistent with the surrounding language (UI strings are English per PLAN.md
Fase 1, comments are Spanish).

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
├─ canvas-shell/    # OS-open normalization → OpenPath; per-OS integration (windows.rs real, linux/macos stubs)
└─ canvas-app/      # `canvas-desktop` binary: eframe/egui + vello, Welcome/Gallery/Editor states
```

### canvas-core: document model
- `Document` → `Page`s → `Layer`s (`layer.rs`): a layer's content is one of
  `ImageContent` / `TextContent` / `ShapeContent` / `SvgContent` /
  `GroupContent`.
- **Groups use `parent_id` + a preorder invariant**, not nested `Vec<Layer>`:
  a group's descendants occupy the contiguous span directly above its header
  in `Page::layers`. The renderer and SVG exporter walk that flat list with
  an index + a "subtree end" stack. Any tree mutation MUST go through
  `Page::move_subtree`/`insert_child` — hand-rolling `Vec` splices breaks the
  invariant.
- **Undo/redo is the Command pattern** (`command.rs`, trait `Command` with
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

### canvas-io
- Loading applies EXIF orientation (the `image` crate does not do this
  itself; `kamadak-exif` is used specifically for that).
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
- Export (`export.rs`) supports PNG/JPEG/SVG/PDF with scale. PDF goes through
  `svg2pdf` on top of **`resvg::usvg`** (not `svg2pdf`'s own usvg) so both
  share one usvg version — re-verify with `cargo tree -i usvg` after bumping
  `resvg`. Text is exported as real `<text>`/`<tspan>` elements (one tspan
  per line, using `canvas_render::text_lines` metrics), not rasterized or
  left to the SVG viewer to line-break.

### canvas-shell
- `ShellIntegration` trait normalizes OS-level "open" events into
  `OpenPath`. `windows.rs` is the real implementation (ProgID
  `CanvasDesktop.Image`, `OpenWithProgids` per extension, folder context
  menu, `SHChangeNotify`); `linux.rs`/`macos.rs` compile but return
  `NotImplemented`.
- Single-instance enforcement (`single_instance.rs`, via `interprocess`): a
  second launch forwards its path(s) to the already-running primary over a
  local socket and exits with code 0 rather than opening a second window.

### canvas-app
- `main.rs` owns the `App` (eframe) state machine: `View` is one of
  `Welcome` / `Loading` / `Gallery` / `Editor`. Disk work (load, save,
  thumbnails, watcher) happens off the UI thread; results come back over
  `std::sync::mpsc` channels into `AppMsg`.
- Navigation that might interrupt an in-flight save is deferred (`Nav`
  enum + `after_save`) rather than mutating `view` mid-borrow.
- `watcher.rs` uses `notify` to detect external changes to the open file and
  shows a "Reload / Keep mine" banner; the app's own saves must not
  self-trigger it.

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

## Where the plan lives

`PLAN.md` tracks phases done/pending and records "decisions taken, don't
reopen without reason" — check it before re-litigating an architectural
choice (e.g. why groups use `parent_id` instead of nested layers, why crop is
"trim at the edges" rather than destructive). `PROMPT.md` is the original
product spec this plan was derived from.
