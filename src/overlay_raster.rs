//! Antialiased pill silhouette (pure Rust, unit-testable).

/// Feather width in pixels for edge antialiasing.
pub const EDGE_FEATHER_PX: f32 = 1.0;
const SUPER_SAMPLE: i32 = 2;

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge0 - edge1).abs() < f32::EPSILON {
        return if x >= edge0 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Signed distance to a horizontal capsule (pill) in pixel coordinates.
pub fn capsule_sdf(px: f32, py: f32, width: f32, height: f32) -> f32 {
    let r = height * 0.5;
    let cx = px + 0.5 - width * 0.5;
    let cy = py + 0.5 - height * 0.5;
    let half_seg = (width - height).max(0.0) * 0.5;
    let qx = cx.abs() - half_seg;
    let dist = (qx.max(0.0).powi(2) + cy.powi(2)).sqrt();
    dist - r
}

#[inline]
pub fn sdf_to_coverage(sdf: f32) -> f32 {
    // Outside (sdf > 0) → 0; inside (sdf < 0) → 1; ~1px feather at the edge.
    smoothstep(0.5 * EDGE_FEATHER_PX, -0.5 * EDGE_FEATHER_PX, sdf)
}

/// Per-pixel coverage 0..=1 with 2×2 supersampling on the silhouette edge.
pub fn capsule_coverage(px: i32, py: i32, width: i32, height: i32) -> f32 {
    let w = width as f32;
    let h = height as f32;
    let mut sum = 0.0f32;
    let n = SUPER_SAMPLE as f32;
    for sy in 0..SUPER_SAMPLE {
        for sx in 0..SUPER_SAMPLE {
            let x = px as f32 + (sx as f32 + 0.5) / n;
            let y = py as f32 + (sy as f32 + 0.5) / n;
            sum += sdf_to_coverage(capsule_sdf(x, y, w, h));
        }
    }
    sum / (n * n)
}

/// Apply premultiplied BGRA alpha mask (Windows 32-bpp DIB byte order).
pub fn apply_pill_alpha_mask(pixels: &mut [u8], width: i32, height: i32) {
    let w = width.max(0) as usize;
    let h = height.max(0) as usize;
    let byte_len = w * h * 4;
    if pixels.len() < byte_len {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let cov = capsule_coverage(x as i32, y as i32, width, height);
            let i = (y * w + x) * 4;
            pixels[i] = (pixels[i] as f32 * cov).round() as u8;
            pixels[i + 1] = (pixels[i + 1] as f32 * cov).round() as u8;
            pixels[i + 2] = (pixels[i + 2] as f32 * cov).round() as u8;
            pixels[i + 3] = (cov * 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_is_fully_opaque() {
        let cov = capsule_coverage(63, 18, 126, 36);
        assert!(cov > 0.99, "center coverage was {cov}");
    }

    #[test]
    fn far_corner_is_transparent() {
        let cov = capsule_coverage(0, 0, 126, 36);
        assert!(cov < 0.01, "corner coverage was {cov}");
    }

    #[test]
    fn sdf_boundary_is_partially_covered() {
        let c = sdf_to_coverage(0.0);
        assert!(c > 0.4 && c < 0.6, "boundary coverage was {c}");
    }

    #[test]
    fn mask_zeroes_outside_pixels() {
        let mut px = vec![0u8; 126 * 36 * 4];
        px[0] = 255;
        px[1] = 255;
        px[2] = 255;
        apply_pill_alpha_mask(&mut px, 126, 36);
        assert_eq!(px[3], 0);
        assert_eq!(px[0], 0);
    }
}
