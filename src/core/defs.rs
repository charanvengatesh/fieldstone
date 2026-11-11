//! Every content schema in one place. Packs (`assets/packs/*`, `mods/*`) are RON files
//! holding `Vec<...>` of these; the engine never hardcodes a concrete block or item.

use std::collections::HashMap;

use serde::Deserialize;

/// Anything stored in a [`super::registry::Registry`]: has a unique string id within its kind.
pub trait Def: serde::de::DeserializeOwned + Send + Sync + 'static {
    const FILE: &'static str;
    fn id(&self) -> &str;
}

macro_rules! def {
    ($t:ty, $file:literal) => {
        impl Def for $t {
            const FILE: &'static str = $file;
            fn id(&self) -> &str {
                &self.id
            }
        }
    };
}

/// sRGB bytes, written `(r, g, b)` in RON.
pub type Rgb = [u8; 3];

/// The id of the empty block. The engine registers it first, so its index is always 0.
pub const AIR: &str = "air";

/// `pack.ron`: pack metadata. Packs load in dependency order; later packs override earlier ids.
#[derive(Deserialize, Default, Debug, Clone)]
pub struct PackDef {
    pub name: String,
    /// Pack format this pack was written for; the engine warns when it is newer than it knows.
    #[serde(default = "one_u32")]
    pub format: u32,
    /// Set to `false` to keep a pack on disk without loading it.
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub depends: Vec<String>,
    /// Lua files, relative to the pack folder.
    #[serde(default)]
    pub scripts: Vec<String>,
}

/// How a block's 16 x 16 pixel-art tiles are drawn from its colours.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pattern {
    /// Loose speckle: dirt, gravel.
    #[default]
    Grain,
    /// One flat colour.
    Smooth,
    /// Blotches and a few cracks.
    Stone,
    /// Fine speckle with ripples.
    Sand,
    /// Near white with sparse cool shade.
    Snow,
    /// Tufted top in the `top` colour; sides are soil with a ragged lip of it.
    Grass,
    /// Clumps with dark gaps.
    Leaves,
    /// Bark on the sides, rings in the `top` colour on the ends.
    Log,
    /// Boards with seams.
    Planks,
    /// Courses with mortar in the `accent` colour.
    Bricks,
    /// Stone with clusters of the `accent` colour.
    Ore,
    /// Wave dashes; the shader makes them move.
    Water,
    /// Clear with a rim and two glints; a cutout.
    Glass,
    /// A clump of grass blades, for a cross prop.
    Tuft,
    /// A stem with a bloom in the `accent` colour, for a cross prop.
    Flower,
    /// A stalk with a cap in the `top` colour, for a cross prop.
    Mushroom,
    /// A few small stones lying low, for a cross prop.
    Pebbles,
    /// A stick with a flame in the `top` colour, for a cross prop.
    Torch,
}

/// How a block fills its cell.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Cube,
    /// Two upright quads crossing at the centre, drawn from the side tile: plants, torches.
    /// Cross blocks never block movement and are see-through.
    Cross,
}

/// `blocks.ron`: one kind of cube. Its tiles are generated from `color`, `top` and `accent`
/// by `pattern`, unless a pack ships `textures/<id>_top.png` and `textures/<id>_side.png`
/// (or one `textures/<id>.png` for both), each 16 x 16.
#[derive(Deserialize, Debug, Clone)]
pub struct BlockDef {
    pub id: String,
    /// Shown in tooltips; the id when empty.
    #[serde(default)]
    pub name: String,
    pub color: Rgb,
    /// Colour of the top face when it differs from the sides (grass over dirt).
    #[serde(default)]
    pub top: Option<Rgb>,
    /// A second colour some patterns use: ore specks, mortar.
    #[serde(default)]
    pub accent: Option<Rgb>,
    #[serde(default)]
    pub pattern: Pattern,
    /// Minimum brightness 0..1 whatever the shade: crystals and lamps stay lit in shadow.
    #[serde(default)]
    pub glow: f32,
    /// Light this block gives off, 0 to 15; it spreads to its surroundings a level less
    /// per block.
    #[serde(default)]
    pub light: u8,
    #[serde(default)]
    pub shape: Shape,
    /// Texels with alpha under one half are holes: leaves with gaps, glass. The faces behind
    /// a cutout block are drawn, and light passes through it.
    #[serde(default)]
    pub cutout: bool,
    /// Blocks movement and can be stood on.
    #[serde(default = "yes")]
    pub solid: bool,
    /// `false` for surfaces nothing may stand on or wade into (water, lava).
    #[serde(default = "yes")]
    pub walkable: bool,
    /// Seconds of holding the break button.
    #[serde(default = "onef")]
    pub hp: f32,
    #[serde(default = "yes")]
    pub breakable: bool,
    /// Item id given when broken; the block's own item when absent.
    #[serde(default)]
    pub drops: Option<String>,
    /// Whether an item that places this block is generated automatically.
    #[serde(default = "yes")]
    pub item: bool,
}
def!(BlockDef, "blocks.ron");

impl BlockDef {
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    pub fn air() -> Self {
        Self {
            id: AIR.into(),
            name: "Air".into(),
            color: [0, 0, 0],
            top: None,
            accent: None,
            pattern: Pattern::Smooth,
            glow: 0.0,
            light: 0,
            shape: Shape::Cube,
            cutout: false,
            solid: false,
            walkable: true,
            hp: 0.0,
            breakable: false,
            drops: None,
            item: false,
        }
    }

    /// Whether a body cannot pass through this block: solid cubes only.
    pub fn blocks_movement(&self) -> bool {
        self.solid && self.shape == Shape::Cube
    }

    /// Whether light and the faces behind it show through: air, water, cutouts and props.
    pub fn see_through(&self) -> bool {
        self.pattern == Pattern::Water || self.cutout || self.shape == Shape::Cross
    }
}

/// `items.ron`: things that are not blocks. Every block with `item: true` gets a record here
/// automatically, so most packs never write this file.
#[derive(Deserialize, Debug, Clone)]
pub struct ItemDef {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "grey")]
    pub color: Rgb,
    #[serde(default = "stack")]
    pub stack_max: u32,
    /// Block id placed when used on a face.
    #[serde(default)]
    pub places: Option<String>,
}
def!(ItemDef, "items.ron");

impl ItemDef {
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// The terrain height field: fractal value noise mapped onto `low..=high`.
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct HeightDef {
    /// Cells per noise cell at the lowest octave; larger is gentler.
    pub scale: f64,
    pub octaves: u32,
    pub low: i32,
    pub high: i32,
}

/// One surface band: columns whose top is at or below `max` get `surface` on top and
/// `under` beneath it.
#[derive(Deserialize, Debug, Clone)]
pub struct BandDef {
    pub max: i32,
    pub surface: String,
    pub under: String,
}

/// `worldgen.ron`: a single record describing the terrain. Features (trees, ore) live in
/// `features.ron` so mods add to the world without replacing this.
#[derive(Deserialize, Debug, Clone)]
pub struct WorldGenDef {
    pub id: String,
    pub height: HeightDef,
    /// Columns lower than this are flooded with `sea_block` up to it.
    pub sea: i32,
    pub sea_block: String,
    /// Bands by surface height, lowest first; the last one catches everything above.
    pub bands: Vec<BandDef>,
    /// How many `under` blocks sit below the surface before `base` starts.
    #[serde(default = "three")]
    pub under_depth: i32,
    /// Fills the rest of the column.
    pub base: String,
    /// The unbreakable bottom layer at z 0.
    pub floor: String,
    /// Surface blocks a new player may start on; any dry ground when empty.
    #[serde(default)]
    pub spawn_on: Vec<String>,
}
def!(WorldGenDef, "worldgen.ron");

/// `stamps.ron`: a small 3D shape. `layers` run bottom to top; each layer is rows of
/// characters read like a map (first row is the highest y, x runs left to right). Every
/// layer has the same width and depth. A space leaves the world alone; any other character
/// must be in `legend`. The stamp's middle column sits on the cell it is placed at.
#[derive(Deserialize, Debug, Clone)]
pub struct StampDef {
    pub id: String,
    pub layers: Vec<Vec<String>>,
    pub legend: HashMap<char, String>,
}
def!(StampDef, "stamps.ron");

/// What a feature does to the generated terrain.
#[derive(Deserialize, Debug, Clone)]
pub enum Rule {
    /// Stands `stamp` on dry columns whose surface block is one of `on`.
    Surface {
        stamp: String,
        on: Vec<String>,
        /// Chance per eligible column, 0..1.
        density: f32,
        /// Only where low-frequency noise exceeds a threshold: `(cells per noise cell,
        /// threshold 0..1)`. Clusters stamps into groves.
        #[serde(default)]
        noise: Option<(f64, f32)>,
    },
    /// Swaps `block` for `with` inside the ground, between two heights.
    Replace {
        block: String,
        with: String,
        density: f32,
        /// As above, in three dimensions: carves veins.
        #[serde(default)]
        noise: Option<(f64, f32)>,
        #[serde(default = "any_height")]
        z: (i32, i32),
    },
}

/// `features.ron`: additions to the terrain. Records are merged by id like everything else,
/// so a mod adds trees or ore without touching the base generator.
#[derive(Deserialize, Debug, Clone)]
pub struct FeatureDef {
    pub id: String,
    pub rule: Rule,
}
def!(FeatureDef, "features.ron");

/// `shaders.ron`: a post-process pass over the picture, in order. `file` is a WGSL fragment
/// shader in the pack (see `mods/README.md` for its bindings); `source` is read from it
/// when the pack loads.
#[derive(Deserialize, Debug, Clone)]
pub struct ShaderDef {
    pub id: String,
    pub file: String,
    /// Passes run in ascending order; ties by id.
    #[serde(default)]
    pub order: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Sixteen numbers the shader reads as four `vec4`s; scripts can change them.
    #[serde(default)]
    pub params: Vec<f32>,
    #[serde(skip)]
    pub source: String,
}
def!(ShaderDef, "shaders.ron");

fn any_height() -> (i32, i32) {
    (0, i32::MAX)
}
fn yes() -> bool {
    true
}
fn onef() -> f32 {
    1.0
}
fn one_u32() -> u32 {
    1
}
fn three() -> i32 {
    3
}
fn stack() -> u32 {
    99
}
fn grey() -> Rgb {
    [160, 160, 160]
}
