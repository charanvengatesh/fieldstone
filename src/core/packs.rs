//! Discovers content packs and merges them into registries. `assets/packs/*` load first,
//! then `mods/*`; within each root, packs load after their declared dependencies.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::defs::*;
use super::registry::Registry;

pub const PACK_ROOTS: [&str; 2] = ["assets/packs", "mods"];
/// Highest pack format this build understands.
pub const PACK_FORMAT: u32 = 1;

/// Packs that were loaded, in order. Scripts are consumed by the scripting layer later.
#[derive(Resource, Default, Debug)]
#[allow(dead_code)] // read by the scripting layer (M3)
pub struct LoadedPacks(pub Vec<(PathBuf, PackDef)>);

/// Reads every pack directory and fills the registries. Runs before the app starts so
/// every other system can assume the content exists.
pub fn load_all(app: &mut App) {
    let mut packs: Vec<(PathBuf, PackDef)> = Vec::new();
    for root in PACK_ROOTS {
        let Ok(dirs) = fs::read_dir(root) else {
            continue;
        };
        let mut found: Vec<(PathBuf, PackDef)> = dirs
            .flatten()
            .map(|d| d.path())
            .filter(|p| p.is_dir())
            .map(|p| {
                let meta = read_ron::<PackDef>(&p.join("pack.ron")).unwrap_or_else(|| PackDef {
                    name: p.file_name().unwrap().to_string_lossy().into_owned(),
                    ..default()
                });
                (p, meta)
            })
            .collect();
        found.retain(|(p, meta)| {
            if !meta.enabled {
                info!("pack '{}' at {} is disabled", meta.name, p.display());
            }
            meta.enabled
        });
        found.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        packs.extend(order_by_depends(found));
    }
    if packs.is_empty() {
        error!("no content packs found under {PACK_ROOTS:?}; the world will be empty");
    }

    let w = app.world_mut();
    for (dir, meta) in &packs {
        info!("loading pack '{}' from {}", meta.name, dir.display());
        if meta.format > PACK_FORMAT {
            warn!(
                "pack '{}' uses format {} but this build knows {}; some fields may be ignored",
                meta.name, meta.format, PACK_FORMAT
            );
        }
        merge::<ColorDef>(w, dir);
        merge::<ShapeDef>(w, dir);
        merge::<TerrainDef>(w, dir);
        merge::<ObjectDef>(w, dir);
        merge::<ItemDef>(w, dir);
        merge::<RecipeDef>(w, dir);
        merge::<LootDef>(w, dir);
        merge::<WorldGenDef>(w, dir);
        merge::<CreatureDef>(w, dir);
        merge::<FishTableDef>(w, dir);
        merge::<HintDef>(w, dir);
    }
    app.insert_resource(LoadedPacks(packs));
}

fn merge<T: Def + std::fmt::Debug>(world: &mut World, dir: &Path) {
    let path = dir.join(T::FILE);
    if !path.exists() {
        return;
    }
    let Some(defs) = read_ron::<Vec<T>>(&path) else {
        return;
    };
    let mut reg = world.get_resource_or_init::<Registry<T>>();
    for d in defs {
        if reg.get(d.id()).is_some() {
            info!("{} overrides '{}'", path.display(), d.id());
        }
        reg.insert(d);
    }
}

fn read_ron<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    match ron::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            error!("{}: {e}", path.display());
            None
        }
    }
}

/// Stable topological order: a pack comes after everything it depends on. Unknown
/// dependencies are ignored; cycles fall back to name order.
fn order_by_depends(mut packs: Vec<(PathBuf, PackDef)>) -> Vec<(PathBuf, PackDef)> {
    let mut out: Vec<(PathBuf, PackDef)> = Vec::with_capacity(packs.len());
    while !packs.is_empty() {
        let ready = packs.iter().position(|(_, p)| {
            p.depends.iter().all(|d| {
                out.iter().any(|(_, o)| &o.name == d) || !packs.iter().any(|(_, q)| &q.name == d)
            })
        });
        out.push(packs.remove(ready.unwrap_or(0)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str, depends: &[&str]) -> (PathBuf, PackDef) {
        (
            PathBuf::new(),
            PackDef {
                name: name.into(),
                format: 1,
                enabled: true,
                depends: depends.iter().map(|s| s.to_string()).collect(),
                scripts: vec![],
            },
        )
    }

    #[test]
    fn dependencies_load_first() {
        let order = order_by_depends(vec![pack("b", &["c"]), pack("a", &[]), pack("c", &["a"])]);
        let names: Vec<_> = order.iter().map(|p| p.1.name.as_str()).collect();
        assert_eq!(names, ["a", "c", "b"]);
    }
}
