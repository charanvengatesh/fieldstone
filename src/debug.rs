//! Debug probe: pictures and text written under `debug/` in the user directory. Scripts
//! drive it (`mods/debug` binds F9); automatic capture every N seconds starts when the
//! environment variable `FIELDSTONE_DEBUG_AUTO=N` is set or a file `debug/auto` holds N,
//! which is how a run nobody is watching can be looked at afterwards.

use std::path::PathBuf;

use std::fmt::Write as _;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

use crate::core::time::{Daylight, GameTime, daylight, sky_color};
use crate::core::{BlockDef, ItemDef, Registry};
use crate::interact::Target;
use crate::inventory::Inventory;
use crate::player::{Player, figure_faces, figure_texture};
use crate::world::iso::View;
use crate::world::light;
use crate::world::mesh::{
    Faces, Grid, LEVELS, Quality, SUBMERGED, WATER_ALPHA, chunk_faces, compose, unpack_flags,
};

/// Time-of-day inputs the block drawing needs.
pub struct Lighting {
    pub day: Daylight,
}
use crate::world::tiles::{TILE, Tiles};
use crate::world::worldgen::WorldGen;
use crate::world::{CHUNK, Focus, Map};

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
/// [`snapshot`] has no such limit.
pub fn screenshot(world: &mut World, name: &str) {
    if world
        .query_filtered::<(), With<PrimaryWindow>>()
        .iter(world)
        .next()
        .is_none()
    {
        return;
    }
    let path = dir().join(format!("{}.png", safe_name(name)));
    info!("debug: screenshot {}", path.display());
    world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// Draws what the camera sees with the software canvas and saves `debug/<name>.png`.
/// Works without a window and whatever the window's state.
pub fn snapshot(world: &mut World, name: &str, scale: f32) -> Result<PathBuf, String> {
    let (pos, shown_z, facing, frame, lit) = world
        .query::<&Player>()
        .iter(world)
        .next()
        .map(|p| (p.body.pos, p.shown_z, p.facing, p.strip_frame(), p.lit))
        .ok_or("no player")?;
    let map = world.resource::<Map>();
    let view = *world.resource::<View>();
    let grid = world.resource::<Grid>().0;
    let wg = world.resource::<WorldGen>();
    let tiles = world.get_resource::<Tiles>().ok_or("no tiles yet")?;
    let time = world.resource::<Time>().elapsed_secs();
    let silhouette = world.resource::<crate::player::Silhouette>().0;
    let center = view.project(world.resource::<Focus>().0).truncate();
    let clock = world.resource::<GameTime>();
    let sky = sky_color(clock.frac());
    let mut canvas = Canvas::new(
        1280,
        800,
        scale,
        center,
        Color::linear_rgb(sky.x, sky.y, sky.z),
    );
    let quality = *world.resource::<Quality>();
    let lighting = Lighting {
        day: daylight(clock.frac()),
    };
    let mut waters = Vec::new();
    for chunk in map.chunks.keys() {
        let lamps = light::flood(
            *chunk,
            map,
            &tiles.light,
            |b| tiles.passes[b as usize],
            |p| map.get(p).unwrap_or_else(|| wg.block(p)),
        );
        let faces = chunk_faces(*chunk, grid, quality, tiles, &lamps, |p| {
            map.get(p).unwrap_or_else(|| wg.block(p))
        });
        let origin = (*chunk * CHUNK).extend(0).as_vec3();
        canvas.draw_blocks(origin, view, &faces.solid, tiles, time, &lighting);
        if faces.water.len() > 0 {
            waters.push((origin, faces.water));
        }
    }
    let at = view.project(pos.with_z(shown_z)) + Vec3::Z * 0.05;
    let mut figure = figure_faces(facing, frame, view.tilt);
    let lamp = light::level_at(
        map,
        &tiles.light,
        |b| tiles.passes.get(b as usize).copied().unwrap_or(false),
        (pos + Vec3::Z * 0.9).floor().as_ivec3(),
    );
    let c = crate::player::figure_light(lit, lamp, &lighting.day);
    figure.colors = vec![[c.x, c.y, c.z, 1.0]; 4];
    let texture = figure_texture();
    canvas.draw(at, &figure, &texture);
    // Where blocks hide the figure, its silhouette shows through them.
    if silhouette {
        canvas.draw_behind(at, &figure, &texture, crate::player::SILHOUETTE);
    }
    // Water goes over everything solid, the player included when they are under it.
    for (origin, faces) in &waters {
        canvas.draw_water(*origin, view, faces, tiles, time, &lighting);
    }
    canvas.save(name)
}

const HALF_W: i32 = 24;
const HALF_H: i32 = 12;

/// A plain-text picture of the game right now: seed, view, player, inventory, target, and
/// maps of the surface block and its height around the player.
pub fn report(world: &mut World) -> String {
    let player = world
        .query::<(&Player, &Inventory)>()
        .iter(world)
        .next()
        .map(|(p, inv)| (p.body, inv.slots.clone(), inv.selected));
    let mut out = String::new();
    let map = world.resource::<Map>();
    let blocks = world.resource::<Registry<BlockDef>>();
    let items = world.resource::<Registry<ItemDef>>();
    let view = world.resource::<View>();
    let target = world.resource::<Target>();
    let seed = world.get_resource::<WorldGen>().map(|w| w.seed);
    let packs = world
        .get_resource::<crate::core::packs::LoadedPacks>()
        .map(|p| {
            p.0.iter()
                .map(|(_, m)| m.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let _ = writeln!(out, "fieldstone {} debug dump", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "seed {seed:?}  day {} at {:.2}  view rot {} ({:.0} degrees)  grid {}  chunks {} loaded, {} meshed, {} faces, {} parked, {} edited  packs {packs} ({} blocks, {} items)",
        world.resource::<GameTime>().day(),
        world.resource::<GameTime>().frac(),
        view.rot(),
        view.angle.to_degrees(),
        world.resource::<Grid>().0,
        map.chunks.len(),
        map.chunks.values().filter(|c| !c.dirty).count(),
        map.chunks.values().map(|c| c.quads as usize).sum::<usize>(),
        map.parked.len(),
        map.chunks.values().filter(|c| c.modified).count(),
        blocks.len(),
        items.len(),
    );
    let Some((body, slots, selected)) = player else {
        out.push_str("no player\n");
        return out;
    };
    let cell = body.pos.floor().as_ivec3();
    let under = map.block(cell - IVec3::Z);
    let _ = writeln!(
        out,
        "\nplayer at {:.2} {:.2} {:.2}  cell {cell}  grounded {}  standing on {}",
        body.pos.x,
        body.pos.y,
        body.pos.z,
        body.grounded,
        blocks.id_of(under),
    );
    let _ = writeln!(out, "selected slot {selected}");
    for (i, s) in slots.iter().enumerate() {
        if let Some(s) = s {
            let _ = writeln!(out, "  slot {i:2}: {} x{}", items.id_of(s.item), s.n);
        }
    }
    match target.0 {
        Some(h) => {
            let _ = writeln!(
                out,
                "target {} at {} face {}",
                blocks.id_of(map.block(h.block)),
                h.block,
                h.normal
            );
        }
        None => {
            let _ = writeln!(out, "target none");
        }
    }
    // Maps: world +y up the page, +x to the right, whatever the view rotation.
    let letters = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let glyph = |b: u16| letters[b as usize % letters.len()] as char;
    let digits = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    let _ = writeln!(
        out,
        "\nsurface {}x{} around the player (world +x right, +y up; @ player, ? unloaded):",
        HALF_W * 2 + 1,
        HALF_H * 2 + 1
    );
    let mut heights = String::new();
    let mut seen: Vec<u16> = Vec::new();
    for dy in (-HALF_H..=HALF_H).rev() {
        let (mut row, mut hrow) = (String::new(), String::new());
        for dx in -HALF_W..=HALF_W {
            let (x, y) = (cell.x + dx, cell.y + dy);
            match map.top(x, y) {
                Some(z) => {
                    let b = map.block(IVec3::new(x, y, z));
                    if !seen.contains(&b) {
                        seen.push(b);
                    }
                    row.push(if dx == 0 && dy == 0 { '@' } else { glyph(b) });
                    hrow.push(digits[z as usize % digits.len()] as char);
                }
                None => {
                    row.push('?');
                    hrow.push('?');
                }
            }
        }
        let _ = writeln!(out, "{row}");
        let _ = writeln!(heights, "{hrow}");
    }
    let _ = writeln!(out, "\nheights (0-9 then A=10 ... V=31):\n{heights}");
    seen.sort_unstable();
    let _ = writeln!(out, "legend:");
    for b in seen {
        let _ = writeln!(out, "  {} {}", glyph(b), blocks.id_of(b));
    }
    out
}

/// Writes [`report`] to `debug/<name>.txt`.
pub fn dump(world: &mut World, name: &str) {
    let path = dir().join(format!("{}.txt", safe_name(name)));
    match std::fs::write(&path, report(world)) {
        Ok(()) => info!("debug: dump {}", path.display()),
        Err(e) => error!("debug: {}: {e}", path.display()),
    }
}

/// Appends a line to `debug/<name>`.
pub fn append(name: &str, line: &str) {
    use std::io::Write;
    let path = dir().join(safe_name(name));
    let r = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(e) = r {
        error!("debug: {}: {e}", path.display());
    }
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
        info!("debug: automatic capture every {every}s");
        commands.insert_resource(Auto {
            every,
            since: 0.0,
            shots: 0,
        });
    }
}

/// Every interval: a window screenshot, a software snapshot and a dump.
fn auto(world: &mut World) {
    let dt = world.resource::<Time>().delta_secs();
    let Some(mut auto) = world.get_resource_mut::<Auto>() else {
        return;
    };
    auto.since += dt;
    if auto.since < auto.every || auto.shots >= AUTO_LIMIT {
        return;
    }
    auto.since = 0.0;
    auto.shots += 1;
    let name = format!("{:03}-auto", auto.shots);
    screenshot(world, &name);
    if let Err(e) = snapshot(world, &format!("{name}-soft"), 2.0) {
        warn!("debug: snapshot: {e}");
    }
    dump(world, &name);
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

/// How a raster pass treats the depth buffer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Opaque,
    Blend,
    Behind,
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

    /// Draws plain textured quads (texture times vertex colour) whose positions are
    /// relative to `offset`: texels in x and y, depth in z. Texels with alpha below one
    /// half are skipped.
    pub fn draw(&mut self, offset: Vec3, faces: &Faces, texture: &Image) {
        let Some(tex) = texture.data.as_ref() else {
            return;
        };
        let (tw, th) = (texture.width() as usize, texture.height() as usize);
        let place = |p: [f32; 3]| Vec3::from(p) + offset;
        self.raster(place, faces, |faces, i, w| {
            let mix = |f: &dyn Fn(usize) -> f32| (0..3).map(|k| w[k] * f(i[k])).sum::<f32>();
            let u = mix(&|k| faces.uvs[k][0]).clamp(0.0, 0.9999);
            let v = mix(&|k| faces.uvs[k][1]).clamp(0.0, 0.9999);
            let t = ((v * th as f32) as usize * tw + (u * tw as f32) as usize) * 4;
            if tex[t + 3] < 128 {
                return None;
            }
            let texel = Color::srgb_u8(tex[t], tex[t + 1], tex[t + 2]).to_linear();
            Some([
                texel.red * mix(&|k| faces.colors[k][0]),
                texel.green * mix(&|k| faces.colors[k][1]),
                texel.blue * mix(&|k| faces.colors[k][2]),
                1.0,
            ])
        });
    }

    /// Draws the texels of `faces` only where something nearer is already drawn, as a flat
    /// `tint` (rgb, alpha) mixed over it: the silhouette of a hidden figure, the way
    /// `assets/shaders/silhouette.wgsl` does it with an inverted depth test.
    pub fn draw_behind(&mut self, offset: Vec3, faces: &Faces, texture: &Image, tint: Vec4) {
        let Some(tex) = texture.data.as_ref() else {
            return;
        };
        let (tw, th) = (texture.width() as usize, texture.height() as usize);
        let place = |p: [f32; 3]| Vec3::from(p) + offset;
        self.raster_mode(place, faces, Mode::Behind, |faces, i, w| {
            let mix = |f: &dyn Fn(usize) -> f32| (0..3).map(|k| w[k] * f(i[k])).sum::<f32>();
            let u = mix(&|k| faces.uvs[k][0]).clamp(0.0, 0.9999);
            let v = mix(&|k| faces.uvs[k][1]).clamp(0.0, 0.9999);
            let t = ((v * th as f32) as usize * tw + (u * tw as f32) as usize) * 4;
            if tex[t + 3] < 128 {
                return None;
            }
            let texel = Color::srgb_u8(tex[t], tex[t + 1], tex[t + 2]).to_linear();
            let luma = 0.4 + 0.6 * (0.3 * texel.red + 0.6 * texel.green + 0.1 * texel.blue);
            Some([tint.x * luma, tint.y * luma, tint.z * luma, tint.w])
        });
    }

    /// Draws block faces the way `assets/shaders/block.wgsl` does: light evaluated at each
    /// texel's centre and banded, outlines from the mask in the vertex alpha, water rows
    /// slid by `time`. Keep the two in step.
    ///
    /// The faces are in world space relative to `origin`; `view` puts them on screen.
    ///
    /// `player` is the shader's see-through target (screen x, y, depth, radius) or zero
    /// radius for none.
    pub fn draw_blocks(
        &mut self,
        origin: Vec3,
        view: View,
        faces: &Faces,
        tiles: &Tiles,
        time: f32,
        lighting: &Lighting,
    ) {
        self.draw_pass(origin, view, faces, tiles, time, lighting, false);
    }

    /// Draws water surfaces the way `assets/shaders/water.wgsl` does: blended over what is
    /// under them, with foam where the outline mask marks a shore.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_water(
        &mut self,
        origin: Vec3,
        view: View,
        faces: &Faces,
        tiles: &Tiles,
        time: f32,
        lighting: &Lighting,
    ) {
        self.draw_pass(origin, view, faces, tiles, time, lighting, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_pass(
        &mut self,
        origin: Vec3,
        view: View,
        faces: &Faces,
        tiles: &Tiles,
        time: f32,
        lighting: &Lighting,
        water_pass: bool,
    ) {
        let Some(tex) = tiles.image.data.as_ref() else {
            return;
        };
        let size = tiles.size.as_vec2();
        let tile = TILE as f32;
        let place = |p: [f32; 3]| view.project(origin + Vec3::from(p));
        let mode = if water_pass {
            Mode::Blend
        } else {
            Mode::Opaque
        };
        self.raster_mode(place, faces, mode, |faces, i, w| {
            let uv_of = |k: usize| Vec2::from(faces.uvs[k]);
            let uv = uv_of(i[0]) * w[0] + uv_of(i[1]) * w[1] + uv_of(i[2]) * w[2];
            let px = uv * size;
            let texel = px.floor().clamp(Vec2::ZERO, size - Vec2::ONE);
            let cell = (texel / tile).floor() * tile;
            let local = texel - cell;
            // Light at the texel's centre: the weights that land on it in this triangle.
            let (e1, e2) = (uv_of(i[1]) - uv_of(i[0]), uv_of(i[2]) - uv_of(i[0]));
            let det = e1.perp_dot(e2);
            let wc = if det.abs() > 1e-14 {
                let d = (texel + Vec2::splat(0.5)) / size - uv_of(i[0]);
                let (a, b) = (d.perp_dot(e2) / det, e1.perp_dot(d) / det);
                [1.0 - a - b, a, b]
            } else {
                w
            };
            let mut channels = Vec3::ZERO;
            for k in 0..3 {
                let c = faces.colors[i[k]];
                channels += Vec3::new(c[0], c[1], c[2]) * wc[k];
            }
            channels = channels.max(Vec3::ZERO);
            let (mask, water, top, submerged) = unpack_flags(faces.colors[i[0]][3]);
            let mut light = compose(channels, &lighting.day);
            let brightness = light.max_element();
            if brightness > 1e-4 {
                let banded = (brightness * LEVELS).round().max(1.0) / LEVELS;
                light *= banded / brightness;
            }
            let mut sample = local;
            if water {
                let slide = ((time * 1.3 + local.y * 0.9).sin() * 1.5 + 0.5).floor();
                sample.x = (local.x + slide + tile).rem_euclid(tile);
            }
            let at = cell + sample;
            let t = ((at.y as usize) * tiles.size.x as usize + at.x as usize) * 4;
            if tex[t + 3] < 128 {
                // A hole in a cutout tile.
                return None;
            }
            let texel_colour = Color::srgb_u8(tex[t], tex[t + 1], tex[t + 2]).to_linear();
            let mut c = Vec3::new(texel_colour.red, texel_colour.green, texel_colour.blue) * light;
            if submerged {
                c *= SUBMERGED;
            }
            let edge = (mask & 1 != 0 && local.y < 0.5)
                || (mask & 2 != 0 && local.x > tile - 1.5)
                || (mask & 4 != 0 && local.y > tile - 1.5)
                || (mask & 8 != 0 && local.x < 0.5);
            if water {
                // Foam along a shore instead of ink; the surface lets the bed show through.
                if edge && top {
                    c = c.lerp(light * 0.9, 0.7);
                }
                return Some([c.x, c.y, c.z, if top { WATER_ALPHA } else { 0.85 }]);
            }
            if edge {
                c *= Vec3::new(0.50, 0.50, 0.58);
            }
            Some([c.x, c.y, c.z, 1.0])
        });
    }

    /// Fills every triangle with whatever `shade` returns for a pixel, given the three
    /// vertex indices and the pixel's weights on them, keeping the nearest result. `place`
    /// turns a vertex position into screen texels and depth.
    fn raster(
        &mut self,
        place: impl Fn([f32; 3]) -> Vec3,
        faces: &Faces,
        shade: impl Fn(&Faces, [usize; 3], [f32; 3]) -> Option<[f32; 4]>,
    ) {
        self.raster_mode(place, faces, Mode::Opaque, shade);
    }

    /// [`Canvas::raster`] with a choice of depth rule: `Opaque` claims the depth, `Blend`
    /// mixes by alpha over what is nearer-or-equal without claiming it (water), `Behind`
    /// mixes only where something nearer is already drawn (a silhouette).
    fn raster_mode(
        &mut self,
        place: impl Fn([f32; 3]) -> Vec3,
        faces: &Faces,
        mode: Mode,
        shade: impl Fn(&Faces, [usize; 3], [f32; 3]) -> Option<[f32; 4]>,
    ) {
        let (width, height, scale, center) = (self.width, self.height, self.scale, self.center);
        let to_px = |p: Vec3| {
            Vec2::new(
                (p.x - center.x) * scale + width as f32 / 2.0,
                height as f32 / 2.0 - (p.y - center.y) * scale,
            )
        };
        for q in 0..faces.len() {
            for tri in faces.triangles(q) {
                let i = tri.map(|k| q * 4 + k);
                let screen = i.map(|k| place(faces.positions[k]));
                let p = screen.map(to_px);
                let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
                if area.abs() < 1e-6 {
                    continue;
                }
                let lo = p[0].min(p[1]).min(p[2]).floor().max(Vec2::ZERO);
                let hi = p[0]
                    .max(p[1])
                    .max(p[2])
                    .ceil()
                    .min(Vec2::new(width as f32, height as f32));
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
                        let depth = (0..3).map(|k| w[k] * screen[k].z).sum::<f32>();
                        let at = (y * width + x) as usize;
                        let passes = match mode {
                            Mode::Behind => depth < self.depth[at],
                            _ => depth >= self.depth[at],
                        };
                        if !passes {
                            continue;
                        }
                        if let Some(color) = shade(faces, i, w) {
                            if mode == Mode::Opaque {
                                self.depth[at] = depth;
                                self.color[at] = [color[0], color[1], color[2]];
                            } else {
                                let a = color[3];
                                for (dst, src) in self.color[at].iter_mut().zip(color) {
                                    *dst += (src - *dst) * a;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The picture as an sRGB image. With `cutout`, pixels nothing was drawn on are
    /// transparent instead of the clear colour.
    pub fn into_image(self, cutout: bool) -> Image {
        let mut data = Vec::with_capacity(self.color.len() * 4);
        for (c, depth) in self.color.iter().zip(&self.depth) {
            let s = Color::linear_rgb(c[0], c[1], c[2]).to_srgba();
            data.extend(
                [s.red, s.green, s.blue].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
            );
            data.push(if cutout && *depth == f32::NEG_INFINITY {
                0
            } else {
                255
            });
        }
        let mut image = Image::new(
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
        image.sampler = bevy::image::ImageSampler::nearest();
        image
    }

    /// Writes `debug/<name>.png`.
    pub fn save(self, name: &str) -> Result<PathBuf, String> {
        let path = dir().join(format!("{}.png", safe_name(name)));
        self.into_image(false)
            .try_into_dynamic()
            .map_err(|e| e.to_string())?
            .save(&path)
            .map_err(|e| e.to_string())?;
        Ok(path)
    }
}

/// A block drawn alone as a 32 x 32 cut-out, for item icons: the same faces, tiles and
/// shading as in the world.
pub fn block_icon(block: u16, tiles: &Tiles) -> Image {
    let faces = chunk_faces(
        IVec2::ZERO,
        false,
        Quality::default(),
        tiles,
        &light::Grid::empty(),
        |p| if p == IVec3::ZERO { block } else { 0 },
    );
    let mut canvas = Canvas::new(32, 32, 1.0, Vec2::ZERO, Color::NONE);
    let noon = Lighting { day: daylight(0.5) };
    canvas.draw_blocks(Vec3::ZERO, View::default(), &faces.solid, tiles, 0.0, &noon);
    canvas.draw_water(Vec3::ZERO, View::default(), &faces.water, tiles, 0.0, &noon);
    canvas.into_image(true)
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, auto);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_keeps_the_nearer_of_two_quads() {
        let texture = Image::new_fill(
            Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255, 255, 255, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        let quad = |depth: f32, color: [f32; 4]| Faces {
            positions: vec![
                [-4.0, -4.0, depth],
                [4.0, -4.0, depth],
                [4.0, 4.0, depth],
                [-4.0, 4.0, depth],
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![color; 4],
            flips: vec![false],
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
        // A silhouette draws only where something nearer already is: behind the blue
        // quad it tints, out in the open (depth 2 over the black background at -inf) it
        // does not.
        let tint = Vec4::new(1.0, 1.0, 1.0, 0.5);
        canvas.draw_behind(Vec3::ZERO, &quad(2.0, [1.0; 4]), &texture, tint);
        let behind = canvas.color[8 * 16 + 8];
        assert!(
            behind[0] > 0.3 && behind[2] > 0.5,
            "tinted over blue: {behind:?}"
        );
        let mut open = Canvas::new(16, 16, 1.0, Vec2::ZERO, Color::BLACK);
        open.draw_behind(Vec3::ZERO, &quad(2.0, [1.0; 4]), &texture, tint);
        assert_eq!(open.color[8 * 16 + 8], [0.0, 0.0, 0.0]);
    }

    #[test]
    fn block_icons_are_cut_out_cubes() {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        let b = blocks.insert(ron::from_str(r#"(id: "b", color: (200, 60, 40))"#).unwrap());
        let tiles = crate::world::tiles::atlas(&blocks, None).1;
        let icon = block_icon(b, &tiles);
        let data = icon.data.as_ref().unwrap();
        let px = |x: usize, y: usize| &data[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        // Corners are clear; the top face, left face and right face are drawn, each darker.
        assert_eq!(px(0, 0)[3], 0);
        assert_eq!(px(31, 31)[3], 0);
        let (top, left, right) = (px(16, 8), px(8, 22), px(24, 22));
        assert!(top[3] == 255 && left[3] == 255 && right[3] == 255);
        assert!(
            top[0] > left[0] && left[0] > right[0],
            "{top:?} {left:?} {right:?}"
        );
    }

    /// Not a check: writes every base block's icon, enlarged, to `debug/icons.png`.
    #[test]
    #[ignore]
    fn icon_sheet() {
        let blocks = crate::world::worldgen::tests::base_blocks();
        let tiles = crate::world::tiles::atlas(&blocks, None).1;
        let n = blocks.len() as u32 - 1;
        let scale = 4u32;
        let mut data = vec![40u8; (n * 32 * scale * 32 * scale * 4) as usize];
        for b in 1..=n {
            let icon = block_icon(b as u16, &tiles);
            let src = icon.data.as_ref().unwrap();
            for y in 0..32 * scale {
                for x in 0..32 * scale {
                    let s = (((y / scale) * 32 + x / scale) * 4) as usize;
                    if src[s + 3] == 0 {
                        continue;
                    }
                    let d = ((y * n * 32 * scale + (b - 1) * 32 * scale + x) * 4) as usize;
                    data[d..d + 4].copy_from_slice(&src[s..s + 4]);
                }
            }
        }
        let sheet = Image::new(
            Extent3d {
                width: n * 32 * scale,
                height: 32 * scale,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        sheet
            .try_into_dynamic()
            .unwrap()
            .save(dir().join("icons.png"))
            .unwrap();
    }

    #[test]
    fn names_cannot_leave_the_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_name(""), "probe");
        assert_eq!(safe_name("001-f9"), "001-f9");
    }
}
