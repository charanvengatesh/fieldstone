//! Every content schema in one place. Packs (`assets/packs/*`, `mods/*`) are RON files
//! holding `Vec<...>` of these; the engine never hardcodes a concrete item, tile, or recipe.

use serde::{Deserialize, Serialize};

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

pub type Rgb = [u8; 3];

/// `pack.ron`: pack metadata. Packs load in dependency order; later packs override earlier ids.
#[derive(Deserialize, Default, Debug, Clone)]
pub struct PackDef {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)] // consumed by the scripting layer (M3)
    pub scripts: Vec<String>,
}

/// `palette.ron`: named colors every other file refers to.
#[derive(Deserialize, Debug, Clone)]
pub struct ColorDef {
    pub id: String,
    pub rgb: Rgb,
}
def!(ColorDef, "palette.ron");

/// One primitive inside a shape. `color` is a palette id. Offsets are in cells (1.0 = one tile).
#[derive(Deserialize, Debug, Clone)]
pub enum Part {
    Circle {
        r: f32,
        color: String,
        #[serde(default)]
        at: [f32; 2],
    },
    Rect {
        w: f32,
        h: f32,
        color: String,
        #[serde(default)]
        at: [f32; 2],
        #[serde(default)]
        rot: f32,
    },
    Poly {
        sides: u32,
        r: f32,
        color: String,
        #[serde(default)]
        at: [f32; 2],
        #[serde(default)]
        rot: f32,
    },
}

/// `shapes.ron`: vector art recipes, drawn back to front.
#[derive(Deserialize, Debug, Clone)]
pub struct ShapeDef {
    pub id: String,
    pub parts: Vec<Part>,
}
def!(ShapeDef, "shapes.ron");

/// `terrain.ron`: the ground layer of a tile.
#[derive(Deserialize, Debug, Clone)]
pub struct TerrainDef {
    pub id: String,
    pub color: String,
    #[serde(default = "yes")]
    pub walkable: bool,
    #[serde(default)]
    #[allow(dead_code)] // fishing (M3)
    pub fish_table: Option<String>,
}
def!(TerrainDef, "terrain.ron");

/// `objects.ron`: anything standing on a tile: trees, rocks, ore, walls, workbenches, crops.
#[derive(Deserialize, Debug, Clone)]
pub struct ObjectDef {
    pub id: String,
    pub shape: String,
    #[serde(default = "yes")]
    pub solid: bool,
    /// Tool power needed to damage it at all. 0 means bare hands work.
    #[serde(default)]
    pub hardness: u8,
    /// Tool tag that works on it (`"axe"`, `"pickaxe"`); `None` means anything.
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default = "one")]
    pub hp: f32,
    #[serde(default)]
    pub drops: Option<String>,
    /// Crafting station tag this object provides when the player stands next to it.
    #[serde(default)]
    pub station: Option<String>,
}
def!(ObjectDef, "objects.ron");

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
pub enum Use {
    #[default]
    None,
    /// Places the named object on an empty tile.
    Place(String),
    Tool {
        tag: String,
        power: u8,
    },
    Weapon {
        damage: f32,
        range: f32,
        cooldown: f32,
    },
    Eat {
        hp: f32,
        stamina: f32,
    },
    Seed(String),
    Rod {
        power: u8,
    },
}

/// `items.ron`
#[derive(Deserialize, Debug, Clone)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    pub shape: String,
    #[serde(default = "stack")]
    pub stack_max: u32,
    #[serde(default)]
    #[allow(dead_code)] // recipe and script filters (M2+)
    pub tags: Vec<String>,
    #[serde(default, rename = "use")]
    pub usage: Use,
}
def!(ItemDef, "items.ron");

/// `recipes.ron`
#[derive(Deserialize, Debug, Clone)]
pub struct RecipeDef {
    pub id: String,
    pub inputs: Vec<(String, u32)>,
    pub output: (String, u32),
    /// Station tag required nearby, if any.
    #[serde(default)]
    pub station: Option<String>,
}
def!(RecipeDef, "recipes.ron");

#[derive(Deserialize, Debug, Clone)]
pub struct LootEntry {
    pub item: String,
    #[serde(default = "one_u32")]
    pub min: u32,
    #[serde(default = "one_u32")]
    pub max: u32,
    #[serde(default = "onef")]
    pub chance: f32,
}

/// `loot.ron`
#[derive(Deserialize, Debug, Clone)]
pub struct LootDef {
    pub id: String,
    pub entries: Vec<LootEntry>,
}
def!(LootDef, "loot.ron");

/// One rule for sprinkling objects over generated terrain.
#[derive(Deserialize, Debug, Clone)]
pub struct Scatter {
    pub object: String,
    /// Terrain ids this may appear on.
    pub on: Vec<String>,
    /// Probability per eligible tile, 0..1.
    pub density: f32,
    /// Optional low-frequency noise gate `(scale, threshold)`: only where noise > threshold.
    #[serde(default)]
    pub noise: Option<(f64, f64)>,
}

/// `worldgen.ron`: a single record describing how terrain is generated from two noise fields
/// (height and moisture, both in -1..1).
#[derive(Deserialize, Debug, Clone)]
pub struct WorldGenDef {
    pub id: String,
    pub height_scale: f64,
    pub moisture_scale: f64,
    /// Height bands, lowest first: `(upper_bound, terrain)`. The last band catches everything above.
    pub bands: Vec<(f64, String)>,
    /// Terrain swapped in where moisture > threshold: `(from, to, threshold)`.
    #[serde(default)]
    pub wet: Vec<(String, String, f64)>,
    #[serde(default)]
    pub scatter: Vec<Scatter>,
}
def!(WorldGenDef, "worldgen.ron");

fn yes() -> bool {
    true
}
fn one() -> f32 {
    1.0
}
fn onef() -> f32 {
    1.0
}
fn one_u32() -> u32 {
    1
}
fn stack() -> u32 {
    99
}
