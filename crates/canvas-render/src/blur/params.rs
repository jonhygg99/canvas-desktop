//! Parametros que viajan a los shaders: los del filtro de color (una pasada)
//! y los del desenfoque (dos pasadas), con su empaquetado a bytes.

use canvas_core::Effects;

/// Radio máximo del kernel (taps por lado). El slider de la UI llega a 100.
pub(super) const MAX_RADIUS: i32 = 100;

/// Parámetros del filtro de color (0 = neutro en todos).
#[derive(Clone, Copy, PartialEq, Default)]
pub struct ColorParams {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub grayscale: f32,
    pub sepia: f32,
}

impl ColorParams {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    pub(super) fn to_bytes(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, v) in [
            self.brightness,
            self.contrast,
            self.saturation,
            self.temperature,
            self.grayscale,
            self.sepia,
            0.0,
            0.0,
        ]
        .into_iter()
        .enumerate()
        {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

impl From<&Effects> for ColorParams {
    fn from(e: &Effects) -> Self {
        Self {
            brightness: e.brightness,
            contrast: e.contrast,
            saturation: e.saturation,
            temperature: e.temperature,
            grayscale: e.grayscale,
            sepia: e.sepia,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct BlurParams {
    pub(super) dir: [f32; 2],
    pub(super) sigma: f32,
    pub(super) radius: i32,
}

// SAFETY del transmute manual: BlurParams es #[repr(C)], 16 bytes, sin padding.
pub(super) fn blur_params_bytes(p: &BlurParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&p.dir[0].to_le_bytes());
    out[4..8].copy_from_slice(&p.dir[1].to_le_bytes());
    out[8..12].copy_from_slice(&p.sigma.to_le_bytes());
    out[12..16].copy_from_slice(&p.radius.to_le_bytes());
    out
}
