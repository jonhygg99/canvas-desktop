//! Geometría de encuadre compatible con el overlay portrait de Flashcut-Auto.

use serde::{Deserialize, Serialize};

pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 1920;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Framing {
    pub x_pct: f32,
    pub y_pct: f32,
    pub scale_pct: u32,
}

impl Default for Framing {
    fn default() -> Self {
        Self {
            x_pct: 0.0,
            y_pct: 0.0,
            scale_pct: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Framing {
    pub fn is_valid(self) -> bool {
        self.x_pct.is_finite()
            && self.y_pct.is_finite()
            && (-100.0..=100.0).contains(&self.x_pct)
            && (-100.0..=100.0).contains(&self.y_pct)
            && (50..=200).contains(&self.scale_pct)
    }

    pub fn placement(self, source_width: f64, source_height: f64) -> Option<Placement> {
        if !self.is_valid()
            || !source_width.is_finite()
            || !source_height.is_finite()
            || source_width <= 0.0
            || source_height <= 0.0
        {
            return None;
        }
        let (w, h) = (f64::from(WIDTH), f64::from(HEIGHT));
        let cover = (w / source_width).max(h / source_height);
        // FFmpeg cuantiza el zoom a dimensiones pares.
        let scale = cover * f64::from(self.scale_pct) / 100.0;
        let width = (source_width * scale / 2.0).floor().max(1.0) * 2.0;
        let height = (source_height * scale / 2.0).floor().max(1.0) * 2.0;
        Some(Placement {
            x: offset(w, width, self.x_pct),
            y: offset(h, height, self.y_pct),
            width,
            height,
        })
    }
}

fn offset(frame: f64, size: f64, pct: f32) -> f64 {
    let gap = frame - size;
    let delta = f64::from(pct) / 100.0 * frame;
    if gap >= 0.0 {
        (gap / 2.0 + delta).clamp(0.0, gap)
    } else {
        (gap / 2.0 - delta).clamp(gap, 0.0)
    }
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
