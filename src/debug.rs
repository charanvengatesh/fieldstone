//! Debug probe: screenshots and text written under `debug/` in the user directory.
//! `F9` saves a screenshot. Automatic capture every N seconds starts when the environment
//! variable `FIELDSTONE_DEBUG_AUTO=N` is set or a file `debug/auto` holds N, which is how a
//! run nobody is watching can be looked at afterwards.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

use crate::world::mesh::Faces;

/// `debug/` under the user directory, created on first use.
pub fn dir() -> PathBuf {
    let d = crate::core::paths::user_dir().join("debug");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Keeps file names inside `debug/`: one path segment of safe characters.
pub fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_start_matches('.').to_owned();
    if s.is_empty() { "probe".to_owned() } else { s }
}

/// Saves the window to `debug/<name>.png`. Reads the swapchain, so the window must be on
/// screen and the app active; a game started from a background shell saves black images.
pub fn screenshot(commands: &mut Commands, name: &str) {
    let path = dir().join(format!("{}.png", safe_name(name)));
    info!("debug: screenshot {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// Automatic capture: seconds between shots, time since the last, shots taken.
#[derive(Resource)]
struct Auto {
    every: f32,
    since: f32,
    shots: u32,
}

const AUTO_LIMIT: u32 = 40;

fn setup(mut commands: Commands) {
    let every = std::env::var("FIELDSTONE_DEBUG_AUTO")
        .ok()
        .or_else(|| std::fs::read_to_string(dir().join("auto")).ok())
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|s| *s > 0.0);
    if let Some(every) = every {
        info!("debug: automatic screenshots every {every}s");
        commands.insert_resource(Auto {
            every,
            since: 0.0,
            shots: 0,
        });
    }
}

fn capture(
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    window: Query<(), With<PrimaryWindow>>,
    auto: Option<ResMut<Auto>>,
    mut manual: Local<u32>,
    mut commands: Commands,
) {
    if window.is_empty() {
        return;
    }
    if input.just_pressed(KeyCode::F9) {
        *manual += 1;
        screenshot(&mut commands, &format!("{:03}-f9", *manual));
    }
    if let Some(mut auto) = auto {
        auto.since += time.delta_secs();
        if auto.since >= auto.every && auto.shots < AUTO_LIMIT {
            auto.since = 0.0;
            auto.shots += 1;
            screenshot(&mut commands, &format!("{:03}-auto", auto.shots));
        }
    }
}

/// A software picture of mesh data: the same quads, texture, vertex colours and depth test
/// the GPU gets, drawn on the CPU. It needs no window, so a headless run can be looked at.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    /// Screen pixels per texel.
    pub scale: f32,
    /// The texel at the middle of the picture.
    pub center: Vec2,
    color: Vec<[f32; 3]>,
    depth: Vec<f32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, scale: f32, center: Vec2, clear: Color) -> Self {
        let c = clear.to_linear();
        Self {
            width,
            height,
            scale,
            center,
            color: vec![[c.red, c.green, c.blue]; (width * height) as usize],
            depth: vec![f32::NEG_INFINITY; (width * height) as usize],
        }
    }

    /// Draws quads whose positions are relative to `offset` (texels in x and y, depth in z).
    /// `texture` is tightly packed sRGB RGBA; texels with alpha below one half are skipped.
    pub fn draw(&mut self, offset: Vec3, faces: &Faces, texture: &Image) {
        let Some(tex) = texture.data.as_ref() else {
            return;
        };
        let (tw, th) = (texture.width() as usize, texture.height() as usize);
        let to_px = |p: [f32; 3]| {
            Vec2::new(
                (p[0] + offset.x - self.center.x) * self.scale + self.width as f32 / 2.0,
                self.height as f32 / 2.0 - (p[1] + offset.y - self.center.y) * self.scale,
            )
        };
        for q in 0..faces.len() {
            for tri in [[0, 1, 2], [0, 2, 3]] {
                let i = tri.map(|k| q * 4 + k);
                let p = i.map(|k| to_px(faces.positions[k]));
                let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
                if area.abs() < 1e-6 {
                    continue;
                }
                let lo = p[0].min(p[1]).min(p[2]).floor().max(Vec2::ZERO);
                let hi = p[0]
                    .max(p[1])
                    .max(p[2])
                    .ceil()
                    .min(Vec2::new(self.width as f32, self.height as f32));
                for y in lo.y as u32..hi.y as u32 {
                    for x in lo.x as u32..hi.x as u32 {
                        let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let w = [
                            (p[1] - c).perp_dot(p[2] - c) / area,
                            (p[2] - c).perp_dot(p[0] - c) / area,
                            (p[0] - c).perp_dot(p[1] - c) / area,
                        ];
                        if w.iter().any(|v| *v < 0.0) {
                            continue;
                        }
                        let mix =
                            |f: &dyn Fn(usize) -> f32| (0..3).map(|k| w[k] * f(i[k])).sum::<f32>();
                        let depth = mix(&|k| faces.positions[k][2]) + offset.z;
                        let at = (y * self.width + x) as usize;
                        if depth < self.depth[at] {
                            continue;
                        }
                        let u = mix(&|k| faces.uvs[k][0]).clamp(0.0, 0.9999);
                        let v = mix(&|k| faces.uvs[k][1]).clamp(0.0, 0.9999);
                        let t = ((v * th as f32) as usize * tw + (u * tw as f32) as usize) * 4;
                        if tex[t + 3] < 128 {
                            continue;
                        }
                        let texel = Color::srgb_u8(tex[t], tex[t + 1], tex[t + 2]).to_linear();
                        self.depth[at] = depth;
                        self.color[at] = [
                            texel.red * mix(&|k| faces.colors[k][0]),
                            texel.green * mix(&|k| faces.colors[k][1]),
                            texel.blue * mix(&|k| faces.colors[k][2]),
                        ];
                    }
                }
            }
        }
    }

    /// Writes `debug/<name>.png`.
    pub fn save(&self, name: &str) -> Result<PathBuf, String> {
        let mut data = Vec::with_capacity(self.color.len() * 4);
        for c in &self.color {
            let s = Color::linear_rgb(c[0], c[1], c[2]).to_srgba();
            data.extend(
                [s.red, s.green, s.blue].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
            );
            data.push(255);
        }
        let image = Image::new(
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        let path = dir().join(format!("{}.png", safe_name(name)));
        image
            .try_into_dynamic()
            .map_err(|e| e.to_string())?
            .save(&path)
            .map_err(|e| e.to_string())?;
        Ok(path)
    }
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, capture);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_keeps_the_nearer_of_two_quads() {
        let mut texture = crate::world::mesh::edge_texture();
        texture.data.as_mut().unwrap().fill(255);
        let quad = |depth: f32, color: [f32; 4]| Faces {
            positions: vec![
                [-4.0, -4.0, depth],
                [4.0, -4.0, depth],
                [4.0, 4.0, depth],
                [-4.0, 4.0, depth],
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![color; 4],
        };
        let mut canvas = Canvas::new(16, 16, 1.0, Vec2::ZERO, Color::BLACK);
        canvas.draw(Vec3::ZERO, &quad(5.0, [1.0, 0.0, 0.0, 1.0]), &texture);
        canvas.draw(Vec3::ZERO, &quad(2.0, [0.0, 1.0, 0.0, 1.0]), &texture);
        // The red quad is nearer, so drawing green afterwards changes nothing.
        assert_eq!(canvas.color[8 * 16 + 8], [1.0, 0.0, 0.0]);
        assert_eq!(canvas.color[0], [0.0, 0.0, 0.0]);
        canvas.draw(
            Vec3::new(0.0, 0.0, 9.0),
            &quad(2.0, [0.0, 0.0, 1.0, 1.0]),
            &texture,
        );
        assert_eq!(canvas.color[8 * 16 + 8], [0.0, 0.0, 1.0]);
    }

    #[test]
    fn names_cannot_leave_the_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_name(""), "probe");
        assert_eq!(safe_name("001-f9"), "001-f9");
    }
}
