//! Zoom de timeline independiente del encuadre y del trim del vídeo.
use super::VideoEdit;

impl VideoEdit {
    pub(super) fn timeline_view(&self) -> (f64, f64) {
        self.timeline_range
            .unwrap_or((0.0, self.duration.unwrap_or(1.0)))
    }

    pub(super) fn fit_timeline_selection(&mut self) {
        let width = (self.trim_end - self.trim_start) * 1.2;
        self.set_timeline_view((self.trim_start + self.trim_end) / 2.0, width);
    }

    pub(super) fn zoom_timeline(&mut self, factor: f64) {
        let (start, end) = self.timeline_view();
        self.set_timeline_view(self.playhead.clamp(start, end), (end - start) * factor);
    }

    fn set_timeline_view(&mut self, center: f64, width: f64) {
        let duration = self.duration.unwrap_or(1.0);
        let width = width.clamp(0.1_f64.min(duration), duration);
        let start = (center - width / 2.0).clamp(0.0, duration - width);
        self.timeline_range = Some((start, start + width));
    }
}
