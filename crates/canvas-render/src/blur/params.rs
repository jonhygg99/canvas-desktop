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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_params_identity_is_default() {
        assert!(ColorParams::default().is_identity());
        assert!(!ColorParams {
            brightness: 0.1,
            ..ColorParams::default()
        }
        .is_identity());
    }

    #[test]
    fn color_params_to_bytes_roundtrips() {
        let p = ColorParams {
            brightness: 0.5,
            contrast: -0.25,
            saturation: 0.75,
            temperature: 0.1,
            grayscale: 0.3,
            sepia: 0.2,
        };
        let bytes = p.to_bytes();
        // 8 valores f32 = 32 bytes.
        assert_eq!(bytes.len(), 32);
        let brightness = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert!((brightness - 0.5).abs() < 1e-6);
        let sepia = f32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        assert!((sepia - 0.2).abs() < 1e-6);
        // Los dos últimos valores deben ser 0 (padding).
        let pad7 = f32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let pad8 = f32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        assert!(pad7 == 0.0 && pad8 == 0.0);
    }

    #[test]
    fn color_params_from_effects_extracts_all_fields() {
        let effects = Effects {
            brightness: 0.1,
            contrast: 0.2,
            saturation: 0.3,
            temperature: 0.4,
            grayscale: 0.5,
            sepia: 0.6,
            ..Effects::default()
        };
        let p = ColorParams::from(&effects);
        assert!((p.brightness - 0.1).abs() < 1e-6);
        assert!((p.contrast - 0.2).abs() < 1e-6);
        assert!((p.saturation - 0.3).abs() < 1e-6);
        assert!((p.temperature - 0.4).abs() < 1e-6);
        assert!((p.grayscale - 0.5).abs() < 1e-6);
        assert!((p.sepia - 0.6).abs() < 1e-6);
    }

    #[test]
    fn blur_params_bytes_roundtrips() {
        let p = BlurParams {
            dir: [1.0, 0.0],
            sigma: 5.0,
            radius: 20,
        };
        let bytes = blur_params_bytes(&p);
        assert_eq!(bytes.len(), 16);
        let dir0 = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let dir1 = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let sigma = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let radius = i32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        assert!((dir0 - 1.0).abs() < 1e-6);
        assert!((dir1 - 0.0).abs() < 1e-6);
        assert!((sigma - 5.0).abs() < 1e-6);
        assert_eq!(radius, 20);
    }

    #[test]
    fn blur_params_horizontal_vs_vertical() {
        let h = BlurParams {
            dir: [1.0, 0.0],
            sigma: 3.0,
            radius: 10,
        };
        let v = BlurParams {
            dir: [0.0, 1.0],
            sigma: 3.0,
            radius: 10,
        };
        let hb = blur_params_bytes(&h);
        let vb = blur_params_bytes(&v);
        // El primer f32 (dir[0]) debe distinguir H de V.
        let h_dir0 = f32::from_le_bytes([hb[0], hb[1], hb[2], hb[3]]);
        let v_dir0 = f32::from_le_bytes([vb[0], vb[1], vb[2], vb[3]]);
        assert!((h_dir0 - 1.0).abs() < 1e-6);
        assert!((v_dir0 - 0.0).abs() < 1e-6);
    }

    #[test]
    fn max_radius_is_at_least_slider_max() {
        // El slider de la UI llega a 100; el kernel debe soportarlo.
        const { assert!(MAX_RADIUS >= 100) };
    }
}
