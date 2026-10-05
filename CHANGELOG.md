# Changelog

## [0.6.0] - 2026-10-05

### Added
- Video editing controls for zoom, position, background blur, canvas size, and precise start/end trimming with duration presets.
- Reset controls for video adjustments, clearer playback controls, and a scrollable settings panel sized to the preview.
- Video download progress and improved trim and preview workflows.

### Changed
- Reorganized the video editor around the preview, with centered playback controls and centered portrait canvases.
- Video downloads exclude audio by default.
- Removed the reset control from Canvas size while retaining the size selector.

### Fixed
- Trim slider state remains consistent when presets are selected after moving the start past the end.
- Video playback initializes reliably in the editor.
- Improved image crop interaction and handling of duplicated designs.

## [0.5.0] - 2026-10-05

### Added
- Portrait 9:16 framing for complete canvas compositions and individual Gallery assets.
- Gallery framing previews for videos, with autoplay, looping, seek, In/Out trim, and muted-by-default audio.
- Portable `.framing` sidecars and PNG export for Flashcut-Auto workflows.

### Changed
- Gallery can switch between normal assets and saved framings, and open an individual framing from its context menu.
