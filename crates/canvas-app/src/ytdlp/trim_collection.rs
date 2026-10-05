//! Recortes independientes del mismo archivo; el activo conserva su historial.
use super::{blur_radius_for_slider, frame_index_at, TrimHistory, VideoAccept, VideoEdit};

pub(super) struct TrimSegment {
    range: (f64, f64),
    history: TrimHistory,
}

impl VideoEdit {
    pub(super) fn trim_ranges(&self) -> Vec<(f64, f64)> {
        if self.trims.is_empty() {
            return vec![(self.trim_start, self.trim_end)];
        }
        self.trims
            .iter()
            .enumerate()
            .map(|(index, trim)| {
                if index == self.active_trim {
                    (self.trim_start, self.trim_end)
                } else {
                    trim.range
                }
            })
            .collect()
    }

    fn store_active_trim(&mut self) {
        self.finish_trim_gesture();
        let segment = TrimSegment {
            range: (self.trim_start, self.trim_end),
            history: std::mem::take(&mut self.trim_history),
        };
        if self.trims.is_empty() {
            self.trims.push(segment);
        } else {
            self.trims[self.active_trim] = segment;
        }
    }

    pub(super) fn add_trim(&mut self) {
        let Some(duration) = self.duration.filter(|d| *d > 0.0) else {
            return;
        };
        let length = (self.trim_end - self.trim_start).max(0.1);
        let start = if duration - self.trim_end >= 0.1 {
            self.trim_end
        } else {
            0.0
        };
        self.store_active_trim();
        self.trims.push(TrimSegment {
            range: super::clamp_trim(start, (start + length).min(duration), duration),
            history: TrimHistory::default(),
        });
        self.activate_trim(self.trims.len() - 1);
    }

    pub(super) fn select_trim(&mut self, index: usize) {
        if index == self.active_trim || index >= self.trims.len() {
            return;
        }
        self.store_active_trim();
        self.activate_trim(index);
    }

    fn activate_trim(&mut self, index: usize) {
        self.active_trim = index;
        (self.trim_start, self.trim_end) = self.trims[index].range;
        self.trim_history = std::mem::take(&mut self.trims[index].history);
        self.timeline_gesture = None;
        self.timeline_range = None;
        self.seek(self.trim_start);
    }

    pub(super) fn remove_trim(&mut self, index: usize) {
        if self.trims.len() <= 1 || index >= self.trims.len() {
            return;
        }
        if index == self.active_trim {
            self.select_trim(if index > 0 { index - 1 } else { 1 });
        }
        self.trims.remove(index);
        if self.active_trim > index {
            self.active_trim -= 1;
        }
    }

    pub(super) fn build_accepts(&self) -> Vec<VideoAccept> {
        let ranges = self.trim_ranges();
        let multiple = ranges.len() > 1;
        ranges
            .into_iter()
            .enumerate()
            .map(|(index, (start, end))| {
                let mut accept = build_accept(self);
                accept.trim_start = start;
                accept.trim_end = Some(end);
                accept.poster = frame_index_at(self, start)
                    .and_then(|i| self.frames.get(i))
                    .cloned()
                    .unwrap_or_default();
                if multiple {
                    accept.title = format!("{} — Trim {}", self.title, index + 1);
                }
                accept
            })
            .collect()
    }
}

/// Construye el accept (siempre con algo válido: el botón lo exige).
pub(super) fn build_accept(edit: &VideoEdit) -> VideoAccept {
    VideoAccept {
        path: edit.path.clone(),
        title: edit.title.clone(),
        size: edit.size,
        video_size: edit.video_size,
        trim_start: edit.trim_start,
        trim_end: Some(edit.trim_end),
        blur_radius: blur_radius_for_slider(edit.blur),
        zoom: edit.zoom,
        position: edit.position,
        poster: frame_index_at(edit, edit.trim_start)
            .and_then(|i| edit.frames.get(i))
            .cloned()
            .unwrap_or_default(),
    }
}
