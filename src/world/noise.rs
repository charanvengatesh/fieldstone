//! Value noise: a hashed lattice blended with a smooth curve, summed over octaves. Small,
//! deterministic and dependency-free.

use crate::core::hash01;

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Smooth noise in 0..1 at a 3D point.
pub fn value3(seed: u32, p: [f64; 3]) -> f32 {
    let cell = [p[0].floor(), p[1].floor(), p[2].floor()];
    let f = [
        smooth(p[0] - cell[0]),
        smooth(p[1] - cell[1]),
        smooth(p[2] - cell[2]),
    ];
    let (x, y, z) = (cell[0] as i32, cell[1] as i32, cell[2] as i32);
    let corner = |dx: i32, dy: i32, dz: i32| hash01(seed, x + dx, y + dy, (z + dz) as u32) as f64;
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let plane = |dz: i32| {
        lerp(
            lerp(corner(0, 0, dz), corner(1, 0, dz), f[0]),
            lerp(corner(0, 1, dz), corner(1, 1, dz), f[0]),
            f[1],
        )
    };
    lerp(plane(0), plane(1), f[2]) as f32
}

/// Fractal noise in 0..1 over the plane: each octave doubles the frequency and halves the
/// weight.
pub fn fbm2(seed: u32, x: f64, y: f64, octaves: u32) -> f32 {
    let (mut sum, mut weight, mut amp, mut freq) = (0.0, 0.0, 1.0, 1.0);
    for i in 0..octaves.max(1) {
        sum += amp * value3(seed.wrapping_add(i * 1013), [x * freq, y * freq, 0.0]);
        weight += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / weight
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_in_range_and_is_smooth() {
        let mut prev = fbm2(7, 0.0, 0.3, 4);
        for i in 1..400 {
            let v = fbm2(7, i as f64 * 0.01, 0.3, 4);
            assert!((0.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.08, "jump at step {i}");
            prev = v;
        }
    }

    #[test]
    fn seeds_differ() {
        let a: Vec<f32> = (0..16).map(|i| fbm2(1, i as f64 * 0.37, 2.1, 3)).collect();
        let b: Vec<f32> = (0..16).map(|i| fbm2(2, i as f64 * 0.37, 2.1, 3)).collect();
        assert_ne!(a, b);
        assert_eq!(
            a,
            (0..16)
                .map(|i| fbm2(1, i as f64 * 0.37, 2.1, 3))
                .collect::<Vec<_>>()
        );
    }
}
