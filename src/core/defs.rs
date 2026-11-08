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
    /// Pack format this pack was written for; the engine warns when it is newer than it knows.
    #[serde(default = "one_u32")]
    pub format: u32,
    /// Set to `false` to keep a pack on disk without loading it.
    #[serde(default = "yes")]
    pub enabled: bool,
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
    /// Opacity 0..255; shadows and glass use this.
    #[serde(default = "opaque")]
    pub a: u8,
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
    /// Fish table id (`fish.ron`) if this is water you can fish in.
    #[serde(default)]
    pub fish_table: Option<String>,
    /// Terrain this becomes when worked with a `hoe` tool.
    #[serde(default)]
    pub till: Option<String>,
}
def!(TerrainDef, "terrain.ron");

/// `objects.ron`: anything standing on a tile: trees, rocks, ore, walls, workbenches, crops.
#[derive(Deserialize, Debug, Clone)]
pub struct ObjectDef {
    pub id: String,
    pub shape: String,
    #[serde(default = "yes")]
    pub solid: bool,
    /// Half-size of the collision box in cells (0.5 fills the cell). Trees use a small trunk.
    #[serde(default = "half")]
    pub radius: f32,
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
    /// Radius in cells within which hostile creatures will not spawn.
    #[serde(default)]
    pub light: Option<f32>,
    /// Experience granted to the breaker.
    #[serde(default = "onef")]
    pub xp: f32,
    /// Terrain ids this may be placed on; `None` allows any walkable terrain.
    #[serde(default)]
    pub on: Option<Vec<String>>,
    /// Crops: `(next object id, seconds)`. Growth doubles next to water.
    #[serde(default)]
    pub grows_into: Option<(String, f32)>,
    /// Container slots; 0 means not a container.
    #[serde(default)]
    pub storage: u32,
    /// Using it (F / right click) swaps it for this object: doors, shrine stages.
    #[serde(default)]
    pub use_swaps: Option<String>,
    /// The swap needs this item held, and eats one.
    #[serde(default)]
    pub use_consumes: Option<String>,
    /// Using it sets the respawn point and, at night, sleeps until dawn.
    #[serde(default)]
    pub bed: bool,
    /// Visual offset from the cell center, in cells, picked per cell. Natural things wander;
    /// built things stay aligned.
    #[serde(default)]
    pub jitter: f32,
}

impl ObjectDef {
    /// Whether F / right click on this object does something itself (instead of placing).
    pub fn interactive(&self) -> bool {
        self.storage > 0 || self.use_swaps.is_some() || self.bed
    }
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
    /// Melee by default; with `projectile` (a shape id) it fires one toward the cursor instead.
    Weapon {
        damage: f32,
        range: f32,
        cooldown: f32,
        #[serde(default)]
        projectile: Option<String>,
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
    /// Only where one of these terrains touches the cell (8 neighbors): shoreline foam, reeds.
    #[serde(default)]
    pub near: Option<Vec<String>>,
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

#[derive(Deserialize, Debug, Clone)]
pub struct FishEntry {
    pub item: String,
    #[serde(default = "onef")]
    pub weight: f32,
    #[serde(default)]
    pub min_rod_power: u8,
    #[serde(default = "any")]
    pub time: TimeOfDay,
}

/// `fish.ron`: what bites in a body of water, keyed by `TerrainDef::fish_table`.
#[derive(Deserialize, Debug, Clone)]
pub struct FishTableDef {
    pub id: String,
    pub entries: Vec<FishEntry>,
}
def!(FishTableDef, "fish.ron");

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct StatsDef {
    pub hp: f32,
    #[serde(default)]
    pub stamina: f32,
    #[serde(default)]
    pub attack: f32,
    #[serde(default)]
    pub defense: f32,
    /// World units per second.
    pub speed: f32,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Behavior {
    /// Wanders; never attacks.
    Passive,
    /// Wanders until the player is within `aggro` cells, then chases and bites.
    Hostile { aggro: f32 },
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Day,
    Night,
    Any,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpawnDef {
    /// Terrain ids this creature may appear on.
    pub on: Vec<String>,
    #[serde(default = "any")]
    pub time: TimeOfDay,
    /// Relative weight among candidates for a cell.
    #[serde(default = "onef")]
    pub weight: f32,
}

/// `creatures.ron`: the player and every mob. The player is the record with id `player`.
#[derive(Deserialize, Debug, Clone)]
pub struct CreatureDef {
    pub id: String,
    pub shape: String,
    pub stats: StatsDef,
    #[serde(default = "passive")]
    pub ai: Behavior,
    #[serde(default)]
    pub drops: Option<String>,
    #[serde(default)]
    pub xp: f32,
    /// Absent for creatures that never spawn naturally (the player, bosses placed by scripts).
    #[serde(default)]
    pub spawn: Option<SpawnDef>,
}
def!(CreatureDef, "creatures.ron");

#[derive(Deserialize, Debug, Clone)]
pub enum HintWhen {
    /// Seconds since the game started.
    After(f32),
    HasItem(String),
    Placed(String),
    Night,
    Day(u32),
    Level(u32),
}

/// `hints.ron`: one-shot guidance lines shown top-center when their trigger first holds.
#[derive(Deserialize, Debug, Clone)]
pub struct HintDef {
    pub id: String,
    pub when: HintWhen,
    pub text: String,
    #[serde(default = "six")]
    pub secs: f32,
}
def!(HintDef, "hints.ron");

fn six() -> f32 {
    6.0
}
fn any() -> TimeOfDay {
    TimeOfDay::Any
}
fn passive() -> Behavior {
    Behavior::Passive
}
fn opaque() -> u8 {
    255
}
fn half() -> f32 {
    0.5
}
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
