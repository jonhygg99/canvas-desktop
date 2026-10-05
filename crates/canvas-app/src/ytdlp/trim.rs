//! Trim no destructivo con extremos estables e historial local a la ventana.
use super::VideoEdit;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum TrimEdge {
    Start,
    End,
}

#[derive(Default)]
pub(super) struct TrimHistory {
    pub undo: Vec<(f64, f64)>,
    pub redo: Vec<(f64, f64)>,
    gesture: Option<(f64, f64)>,
}

pub(super) fn timecode(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

impl VideoEdit {
    pub(super) fn seek(&mut self, time: f64) {
        self.playing = false;
        self.playback.stop();
        self.last_tick = None;
        crate::audio::pause_for(&self.path);
        self.playhead = time.clamp(0.0, self.duration.unwrap_or(time).max(0.0));
    }

    pub(super) fn set_trim_edge(&mut self, edge: TrimEdge, time: f64) {
        let Some(duration) = self.duration else {
            return;
        };
        if !time.is_finite() {
            return;
        }
        let before = (self.trim_start, self.trim_end);
        let gap = 0.1_f64.min(duration);
        match edge {
            TrimEdge::Start => self.trim_start = time.clamp(0.0, (self.trim_end - gap).max(0.0)),
            TrimEdge::End => {
                self.trim_end = time.clamp((self.trim_start + gap).min(duration), duration)
            }
        }
        self.record_trim(before);
        let preview = match edge {
            TrimEdge::Start => self.trim_start,
            TrimEdge::End => (self.trim_end - 1.0 / self.source_fps).max(self.trim_start),
        };
        self.seek(preview);
    }

    fn record_trim(&mut self, before: (f64, f64)) {
        if before != (self.trim_start, self.trim_end) && self.trim_history.gesture.is_none() {
            self.trim_history.undo.push(before);
            if self.trim_history.undo.len() > 100 {
                self.trim_history.undo.remove(0);
            }
            self.trim_history.redo.clear();
        }
    }

    pub(super) fn begin_trim_gesture(&mut self) {
        self.trim_history
            .gesture
            .get_or_insert((self.trim_start, self.trim_end));
    }

    pub(super) fn finish_trim_gesture(&mut self) {
        if let Some(before) = self.trim_history.gesture.take() {
            self.record_trim(before);
        }
    }

    pub(super) fn undo_trim(&mut self) {
        self.finish_trim_gesture();
        if let Some(range) = self.trim_history.undo.pop() {
            self.trim_history
                .redo
                .push((self.trim_start, self.trim_end));
            (self.trim_start, self.trim_end) = range;
            self.seek(self.trim_start);
        }
    }

    pub(super) fn redo_trim(&mut self) {
        if let Some(range) = self.trim_history.redo.pop() {
            self.trim_history
                .undo
                .push((self.trim_start, self.trim_end));
            (self.trim_start, self.trim_end) = range;
            self.seek(self.trim_start);
        }
    }

    pub(super) fn reset_trim(&mut self) {
        let Some(duration) = self.duration else {
            return;
        };
        let before = (self.trim_start, self.trim_end);
        (self.trim_start, self.trim_end) = (0.0, duration);
        self.record_trim(before);
        self.seek(0.0);
    }
}
