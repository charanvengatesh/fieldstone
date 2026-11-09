//! Pointing at blocks, breaking them and placing them. The pointer picks the first block
//! along the view ray; holding the primary button wears it down, the secondary button
//! places the held block against the face that was hit.

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use bevy::window::PrimaryWindow;

use crate::camera::{MainCamera, Overlay};
use crate::core::settings::Keys;
use crate::core::{BlockDef, ItemDef, PointerCaptured, Registry, UiHovered};
use crate::inventory::{HOTBAR, Inventory};
use crate::player::Player;
use crate::world::iso::{Hit, View};
use crate::world::mesh::corners;
use crate::world::physics::{TALL, fits};
use crate::world::{AIR, HEIGHT, Map};

/// How far from the player's middle a block may be worked, in blocks.
pub const REACH: f32 = 5.0;
/// Seconds between placements while the button is held.
const PLACE_REPEAT: f32 = 0.2;

/// The pointer in world texels, if it is over the window.
#[derive(Resource, Default)]
pub struct Cursor(pub Option<Vec2>);

/// The block face under the pointer, when it is within reach.
#[derive(Resource, Default)]
pub struct Target(pub Option<Hit>);

/// Progress 0..1 on the block being broken.
#[derive(Resource, Default)]
pub struct Breaking {
    pub block: Option<IVec3>,
    pub progress: f32,
}

#[derive(Message, Clone, Copy)]
pub struct BlockBroken {
    pub at: IVec3,
    pub block: u16,
}

#[derive(Message, Clone, Copy)]
pub struct BlockPlaced {
    pub at: IVec3,
    pub block: u16,
}

#[derive(Component)]
pub struct Highlight;

pub fn track_cursor(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform), (With<MainCamera>, Without<Overlay>)>,
    mut cursor: ResMut<Cursor>,
) {
    let (camera, tf) = *cam;
    cursor.0 = window
        .cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(tf, p).ok());
}

pub fn within_reach(player: Vec3, block: IVec3) -> bool {
    let middle = player + Vec3::Z * (TALL / 2.0);
    (block.as_vec3() + Vec3::splat(0.5)).distance(middle) <= REACH
}

pub fn aim(
    cursor: Res<Cursor>,
    view: Res<View>,
    map: Res<Map>,
    ui: Res<UiHovered>,
    captured: Res<PointerCaptured>,
    player: Single<&Player>,
    mut target: ResMut<Target>,
) {
    target.0 = cursor
        .0
        .filter(|_| !ui.0 && !captured.0)
        .and_then(|at| view.pick(at, HEIGHT, |c| map.block(c) != AIR))
        .filter(|hit| within_reach(player.body.pos, hit.block));
}

pub fn break_block(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    target: Res<Target>,
    blocks: Res<Registry<BlockDef>>,
    items: Res<Registry<ItemDef>>,
    mut map: ResMut<Map>,
    mut breaking: ResMut<Breaking>,
    mut inv: Single<&mut Inventory, With<Player>>,
    mut broken: MessageWriter<BlockBroken>,
) {
    let aimed = target.0.map(|h| h.block);
    if !mouse.pressed(MouseButton::Left) || aimed.is_none() || aimed != breaking.block {
        breaking.block = aimed.filter(|_| mouse.pressed(MouseButton::Left));
        breaking.progress = 0.0;
        if breaking.block.is_none() {
            return;
        }
    }
    let at = aimed.expect("checked above");
    let id = map.block(at);
    let def = blocks.at(id);
    if id == AIR || !def.breakable {
        breaking.progress = 0.0;
        return;
    }
    breaking.progress += time.delta_secs() / def.hp.max(0.05);
    if breaking.progress < 1.0 {
        return;
    }
    breaking.block = None;
    breaking.progress = 0.0;
    map.set(at, AIR);
    if let Some(item) = items.idx(def.drops.as_deref().unwrap_or(&def.id)) {
        inv.add(item, 1, &items);
    }
    broken.write(BlockBroken { at, block: id });
}

pub fn place_block(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    target: Res<Target>,
    blocks: Res<Registry<BlockDef>>,
    items: Res<Registry<ItemDef>>,
    mut map: ResMut<Map>,
    mut cooldown: Local<f32>,
    mut player: Single<(&Player, &mut Inventory)>,
    mut placed: MessageWriter<BlockPlaced>,
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    if !mouse.pressed(MouseButton::Right) {
        *cooldown = 0.0;
        return;
    }
    if *cooldown > 0.0 {
        return;
    }
    let (player, inv) = &mut *player;
    let (Some(hit), Some(stack)) = (target.0, inv.held()) else {
        return;
    };
    let Some(block) = items
        .at(stack.item)
        .places
        .as_deref()
        .and_then(|id| blocks.idx(id))
    else {
        return;
    };
    let at = hit.block + hit.normal;
    // Only into loaded air, and never into the space the player occupies.
    if map.get(at) != Some(AIR) || !fits(player.body.pos, &|c| c == at) {
        return;
    }
    map.set(at, block);
    inv.consume_held();
    *cooldown = PLACE_REPEAT;
    placed.write(BlockPlaced { at, block });
}

/// Number keys pick a hotbar slot; scrolling steps through them.
pub fn select_slot(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    ui: Res<UiHovered>,
    mut wheel: MessageReader<MouseWheel>,
    mut acc: Local<f32>,
    mut inv: Single<&mut Inventory, With<Player>>,
) {
    for (i, key) in keys.hotbar.iter().enumerate().take(HOTBAR) {
        if input.just_pressed(*key) {
            inv.selected = i;
        }
    }
    // With a modifier held the wheel belongs to something else (zoom).
    let modified = input.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    for w in wheel.read() {
        if !ui.0 && !modified {
            *acc += match w.unit {
                MouseScrollUnit::Line => w.y,
                MouseScrollUnit::Pixel => w.y / 40.0,
            };
        }
    }
    while acc.abs() >= 1.0 {
        let step = if *acc > 0.0 { HOTBAR - 1 } else { 1 };
        inv.selected = (inv.selected + step) % HOTBAR;
        *acc -= acc.signum();
    }
}

pub fn spawn_highlight(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 4])
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    commands.spawn((
        Highlight,
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(materials.add(ColorMaterial {
            color: Color::srgba(1.0, 1.0, 1.0, 0.25),
            alpha_mode: AlphaMode2d::Blend,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Lays a translucent quad over the targeted face; it brightens as the block wears down.
pub fn show_highlight(
    target: Res<Target>,
    breaking: Res<Breaking>,
    view: Res<View>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut quad: Single<
        (
            &Mesh2d,
            &MeshMaterial2d<ColorMaterial>,
            &mut Transform,
            &mut Visibility,
        ),
        With<Highlight>,
    >,
) {
    let (mesh, material, tf, vis) = &mut *quad;
    let Some(hit) = target.0 else {
        **vis = Visibility::Hidden;
        return;
    };
    **vis = Visibility::Visible;
    if let Some(mut m) = meshes.get_mut(&mesh.0) {
        let positions: Vec<[f32; 3]> = corners(IVec3::ZERO, hit.normal)
            .iter()
            .map(|c| view.project(*c).to_array())
            .collect();
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    }
    // A hair nearer than the face so it never fights it.
    tf.translation = view.project(hit.block.as_vec3()) + Vec3::Z * 0.02;
    if let Some(mut m) = materials.get_mut(&material.0) {
        m.color = Color::srgba(
            1.0,
            1.0,
            1.0,
            0.25 + 0.5 * breaking.progress.clamp(0.0, 1.0),
        );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractSet;

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .init_resource::<Target>()
            .init_resource::<Breaking>()
            .add_message::<BlockBroken>()
            .add_message::<BlockPlaced>()
            .add_systems(
                Update,
                (aim, break_block, place_block, select_slot)
                    .chain()
                    .in_set(InteractSet)
                    .after(crate::player::PlayerSet)
                    .before(crate::world::WorldSet)
                    .run_if(crate::core::not_paused),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_is_measured_from_the_players_middle() {
        let player = Vec3::new(0.5, 0.5, 10.0);
        assert!(within_reach(player, IVec3::new(4, 0, 10)));
        assert!(!within_reach(player, IVec3::new(6, 0, 10)));
        // The block underfoot and the one overhead are both workable.
        assert!(within_reach(player, IVec3::new(0, 0, 9)));
        assert!(within_reach(player, IVec3::new(0, 0, 12)));
    }
}
