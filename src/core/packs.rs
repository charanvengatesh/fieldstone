//! Discovers content packs and merges them into registries. `assets/packs/*` load first,
//! then `mods/*`; within each root, packs load after their declared dependencies.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::defs::*;
use super::registry::Registry;

pub const PACK_ROOTS: [&str; 2] = ["assets/packs", "mods"];
/// Highest pack format this build understands.
pub const PACK_FORMAT: u32 = 2;

/// Packs that were loaded, in order.
#[derive(Resource, Default, Debug)]
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
    // Air is index 0 of the block registry, whatever the packs define.
    w.get_resource_or_init::<Registry<BlockDef>>()
        .insert(BlockDef::air());
    w.get_resource_or_init::<Registry<ItemDef>>();
    w.get_resource_or_init::<Registry<WorldGenDef>>();
    for (dir, meta) in &packs {
        info!("loading pack '{}' from {}", meta.name, dir.display());
        if meta.format > PACK_FORMAT {
            warn!(
                "pack '{}' uses format {} but this build knows {}; some fields may be ignored",
                meta.name, meta.format, PACK_FORMAT
            );
        }
        merge::<BlockDef>(w, dir);
        merge::<ItemDef>(w, dir);
        merge::<WorldGenDef>(w, dir);
    }
    block_items(w);
    app.insert_resource(LoadedPacks(packs));
}

/// Gives every block that asks for one an item that places it, unless a pack wrote its own.
fn block_items(world: &mut World) {
    let made: Vec<ItemDef> = {
        let blocks = world.resource::<Registry<BlockDef>>();
        let items = world.resource::<Registry<ItemDef>>();
        blocks
            .iter()
            .filter(|(_, b)| b.item && items.get(&b.id).is_none())
            .map(|(_, b)| ItemDef {
                id: b.id.clone(),
                name: b.label().to_owned(),
                color: b.top.unwrap_or(b.color),
                stack_max: 99,
                places: Some(b.id.clone()),
            })
            .collect()
    };
    let mut items = world.resource_mut::<Registry<ItemDef>>();
    for item in made {
        items.insert(item);
    }
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
    let mut overridden = Vec::new();
    for d in defs {
        if reg.get(d.id()).is_some() {
            overridden.push(d.id().to_owned());
        }
        reg.insert(d);
    }
    if !overridden.is_empty() {
        info!("{} overrides {}", path.display(), overridden.join(", "));
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
