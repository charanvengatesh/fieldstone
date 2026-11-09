//! All of the interface: hotbar, inventory panel, pause menu, tooltip, toast and the debug
//! line. Item icons are one generated cube tinted with each item's colour.

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::image::ImageSampler;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::core::settings::{Keys, Settings};
use crate::core::{BlockDef, ItemDef, Paused, Registry, SaveRequest, Toast, UiHovered};
use crate::interact::Target;
use crate::inventory::{HOTBAR, Inventory, SLOTS, Stack, transfer};
use crate::player::Player;
use crate::world::Map;

const SLOT: f32 = 46.0;
const ICON: f32 = 34.0;
const GAP: f32 = 6.0;
const PANEL_BG: Color = Color::srgba(0.06, 0.07, 0.10, 0.84);
const PANEL_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const SLOT_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.30);
const BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);
const SELECTED: Color = Color::srgb(1.0, 0.88, 0.45);
const TEXT_DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.7);
const BAR_W: f32 = (SLOT + GAP) * HOTBAR as f32 + 10.0;

/// The cube every item icon is tinted from.
#[derive(Resource)]
pub struct Icon(pub Handle<Image>);

/// The item the pointer rests on, for the tooltip.
#[derive(Resource, Default)]
pub struct Tooltip(pub Option<u16>);

#[derive(Component)]
struct SlotUi(usize);
#[derive(Component)]
struct Swatch;
#[derive(Component)]
struct Count;
#[derive(Component)]
struct Hotbar;
#[derive(Component)]
struct HeldName;
#[derive(Component)]
struct ToastText;
#[derive(Component)]
struct DebugText;
#[derive(Component)]
struct InventoryPanel;
#[derive(Component)]
struct PauseMenu;
#[derive(Component)]
struct TooltipPanel;
#[derive(Component)]
struct TooltipText;

#[derive(Clone, Copy)]
enum MenuButton {
    Resume,
    Save,
    Quit,
}

/// Rounded translucent panel used for every surface.
fn panel(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            ..node
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(PANEL_BORDER),
    )
}

fn label(text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// A slot box showing one inventory index: an icon child and a count child. Hovering it
/// feeds the tooltip.
fn spawn_slot(parent: &mut ChildSpawnerCommands, index: usize) -> Entity {
    let e = parent
        .spawn((
            SlotUi(index),
            Node {
                width: Val::Px(SLOT),
                height: Val::Px(SLOT),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(SLOT_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|s| {
            s.spawn((
                Swatch,
                ImageNode::new(Handle::default()).with_color(Color::NONE),
                Node {
                    width: Val::Px(ICON),
                    height: Val::Px(ICON),
                    ..default()
                },
            ));
            s.spawn((
                Count,
                label("", 12.0, Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(4.0),
                    bottom: Val::Px(2.0),
                    ..default()
                },
            ));
        })
        .id();
    parent
        .commands()
        .entity(e)
        .observe(
            move |_: On<PointerOver>,
                  inv: Single<&Inventory, With<Player>>,
                  mut tip: ResMut<Tooltip>| {
                tip.0 = inv.slots.get(index).copied().flatten().map(|s| s.item);
            },
        )
        .observe(|_: On<PointerOut>, mut tip: ResMut<Tooltip>| tip.0 = None);
    e
}

fn spawn(mut images: ResMut<Assets<Image>>, mut commands: Commands) {
    commands.insert_resource(Icon(images.add(cube_icon())));
    // Hotbar with the held item's name above it.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-BAR_W / 2.0)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn((HeldName, label("", 14.0, TEXT_DIM)));
            col.spawn((
                Hotbar,
                panel(Node {
                    padding: UiRect::all(Val::Px(5.0)),
                    column_gap: Val::Px(GAP),
                    ..default()
                }),
            ))
            .with_children(|p| {
                for i in 0..HOTBAR {
                    let e = spawn_slot(p, i);
                    p.commands().entity(e).observe(
                        move |_: On<PointerClick>, mut inv: Single<&mut Inventory, With<Player>>| {
                            inv.selected = i;
                        },
                    );
                }
            });
        });
    // Inventory panel above the hotbar.
    commands
        .spawn((
            InventoryPanel,
            panel(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0 + SLOT + 44.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-BAR_W / 2.0)),
                padding: UiRect::all(Val::Px(5.0)),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(HOTBAR as u16, SLOT),
                row_gap: Val::Px(GAP),
                column_gap: Val::Px(GAP),
                ..default()
            }),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            for i in HOTBAR..SLOTS {
                let e = spawn_slot(p, i);
                p.commands().entity(e).observe(
                    move |click: On<PointerClick>,
                          input: Res<ButtonInput<KeyCode>>,
                          items: Res<Registry<ItemDef>>,
                          mut inv: Single<&mut Inventory, With<Player>>| {
                        let whole = click.button != PointerButton::Secondary
                            && !input.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                        if whole {
                            // A plain click swaps with the selected hotbar slot.
                            let sel = inv.selected;
                            inv.slots.swap(i, sel);
                            return;
                        }
                        // Right-click peels one off, shift-click half, into an empty slot.
                        let n = inv.slots[i].map_or(0, |s| s.n);
                        let amount = if click.button == PointerButton::Secondary {
                            1
                        } else {
                            n.div_ceil(2)
                        };
                        split(&mut inv.slots, i, amount, &items);
                    },
                );
            }
        });
    // Toast line, top centre.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((ToastText, label("", 18.0, Color::WHITE)));
        });
    commands.spawn((
        DebugText,
        label("", 13.0, TEXT_DIM),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
        Visibility::Hidden,
    ));
    // Tooltip beside the pointer.
    commands
        .spawn((
            TooltipPanel,
            panel(Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                max_width: Val::Px(260.0),
                ..default()
            }),
            Visibility::Hidden,
            GlobalZIndex(20),
        ))
        .with_children(|p| {
            p.spawn((TooltipText, label("", 13.0, Color::WHITE)));
        });
    // Pause menu.
    commands
        .spawn((
            PauseMenu,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            Visibility::Hidden,
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            root.spawn(panel(Node {
                padding: UiRect::all(Val::Px(24.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                min_width: Val::Px(260.0),
                ..default()
            }))
            .with_children(|p| {
                p.spawn((
                    label("Paused", 30.0, Color::WHITE),
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                ));
                for (button, text) in [
                    (MenuButton::Resume, "Resume"),
                    (MenuButton::Save, "Save game"),
                    (MenuButton::Quit, "Save and quit"),
                ] {
                    p.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(7.0)),
                            ..default()
                        },
                        BackgroundColor(SLOT_BG),
                    ))
                    .with_children(|b| {
                        b.spawn(label(text, 16.0, Color::WHITE));
                    })
                    .observe(
                        move |_: On<PointerClick>,
                              mut paused: ResMut<Paused>,
                              mut save: ResMut<SaveRequest>| {
                            match button {
                                MenuButton::Resume => paused.0 = false,
                                MenuButton::Save => save.save = true,
                                MenuButton::Quit => {
                                    save.save = true;
                                    save.quit = true;
                                }
                            }
                        },
                    );
                }
                p.spawn(label(
                    "WASD move   click break   right-click place   Q/E turn   Tab bag",
                    12.0,
                    TEXT_DIM,
                ));
            });
        });
}

/// Moves `amount` from slot `i` into the first empty slot, never merging into a stack.
pub fn split(slots: &mut [Option<Stack>], i: usize, amount: u32, items: &Registry<ItemDef>) {
    let Some(empty) = slots.iter().position(|s| s.is_none()) else {
        return;
    };
    if amount == 0 || empty == i {
        return;
    }
    let mut dst = [None];
    if transfer(slots, i, &mut dst, amount, items) > 0 {
        slots[empty] = dst[0];
    }
}

/// Everything needed to paint slot boxes. Shared by the hotbar and the inventory panel.
#[derive(SystemParam)]
struct SlotPainter<'w, 's> {
    items: Res<'w, Registry<ItemDef>>,
    icon: Res<'w, Icon>,
    boxes: Query<'w, 's, (&'static SlotUi, &'static mut BorderColor, &'static Children)>,
    swatches: Query<'w, 's, &'static mut ImageNode, With<Swatch>>,
    counts: Query<'w, 's, &'static mut Text, (With<Count>, Without<HeldName>)>,
}

impl SlotPainter<'_, '_> {
    fn paint(&mut self, children: &Children, slots: &[Option<Stack>], selected: Option<usize>) {
        for child in children {
            let Ok((slot, mut border, kids)) = self.boxes.get_mut(*child) else {
                continue;
            };
            let stack = slots.get(slot.0).copied().flatten();
            *border = BorderColor::all(if Some(slot.0) == selected {
                SELECTED
            } else {
                BORDER
            });
            for kid in kids {
                if let Ok(mut img) = self.swatches.get_mut(*kid) {
                    match stack {
                        Some(s) => {
                            let c = self.items.at(s.item).color;
                            img.image = self.icon.0.clone();
                            img.color = Color::srgb_u8(c[0], c[1], c[2]);
                        }
                        None => img.color = Color::NONE,
                    }
                }
                if let Ok(mut text) = self.counts.get_mut(*kid) {
                    text.0 = stack
                        .filter(|s| s.n > 1)
                        .map_or(String::new(), |s| s.n.to_string());
                }
            }
        }
    }
}

fn paint(
    inv: Single<&Inventory, With<Player>>,
    items: Res<Registry<ItemDef>>,
    hotbar: Single<&Children, With<Hotbar>>,
    panel: Single<(&Visibility, &Children), With<InventoryPanel>>,
    mut name: Single<&mut Text, (With<HeldName>, Without<Count>)>,
    mut painter: SlotPainter,
) {
    painter.paint(&hotbar, &inv.slots, Some(inv.selected));
    let (vis, children) = *panel;
    if *vis != Visibility::Hidden {
        painter.paint(children, &inv.slots, None);
    }
    name.0 = inv
        .held()
        .map_or(String::new(), |s| items.at(s.item).label().to_owned());
}

fn toggles(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    settings: Res<Settings>,
    mut paused: ResMut<Paused>,
    mut panels: ParamSet<(
        Single<&mut Visibility, With<InventoryPanel>>,
        Single<&mut Visibility, With<PauseMenu>>,
        Single<&mut Visibility, With<DebugText>>,
    )>,
) {
    let flip = |vis: &mut Visibility| {
        *vis = if *vis == Visibility::Hidden {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    };
    if keys.just(&input, &keys.inventory) {
        flip(&mut panels.p0());
    }
    if keys.just(&input, &keys.debug) {
        flip(&mut panels.p2());
    }
    if keys.just(&input, &keys.pause) {
        paused.0 = !paused.0;
        if paused.0 {
            crate::core::settings::save(&settings);
        }
    }
    **panels.p1() = if paused.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

fn texts(
    toast: Res<Toast>,
    time: Res<Time>,
    map: Res<Map>,
    target: Res<Target>,
    blocks: Res<Registry<BlockDef>>,
    player: Single<&Player>,
    mut lines: ParamSet<(
        Single<&mut Text, With<ToastText>>,
        Single<(&mut Text, &Visibility), With<DebugText>>,
    )>,
) {
    if lines.p0().0 != toast.text {
        lines.p0().0 = toast.text.clone();
    }
    let mut debug = lines.p1();
    let (text, vis) = &mut *debug;
    if **vis == Visibility::Hidden {
        return;
    }
    let p = player.body.pos;
    let aim = target.0.map_or("nothing".to_owned(), |h| {
        format!(
            "{} at {} face {}",
            blocks.id_of(map.block(h.block)),
            h.block,
            h.normal
        )
    });
    text.0 = format!(
        "{:.0} fps   pos {:.1} {:.1} {:.1}   chunks {}   aim {aim}",
        1.0 / time.delta_secs().max(1e-4),
        p.x,
        p.y,
        p.z,
        map.chunks.len(),
    );
}

fn tooltip(
    tip: Res<Tooltip>,
    items: Res<Registry<ItemDef>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut panel: Single<(&mut Node, &mut Visibility), With<TooltipPanel>>,
    mut text: Single<&mut Text, With<TooltipText>>,
) {
    let (node, vis) = &mut *panel;
    let Some(item) = tip.0 else {
        **vis = Visibility::Hidden;
        return;
    };
    let def = items.at(item);
    text.0 = match &def.places {
        Some(block) if *block != def.id => format!("{}\nPlaces {block}", def.label()),
        Some(_) => format!("{}\nBlock", def.label()),
        None => def.label().to_owned(),
    };
    **vis = Visibility::Visible;
    if let Some(p) = window.cursor_position() {
        node.left = Val::Px(p.x + 14.0);
        node.top = Val::Px(p.y + 14.0);
    }
}

/// Mirrors the picking hover map into `UiHovered` so world clicks ignore the interface.
fn track_hover(
    hover: Res<bevy::picking::hover::HoverMap>,
    nodes: Query<(), With<Node>>,
    mut ui: ResMut<UiHovered>,
) {
    ui.0 = hover
        .values()
        .flat_map(|m| m.keys())
        .any(|e| nodes.contains(*e));
}

/// A white isometric cube with shaded sides and inked edges, 32 texels square.
pub fn cube_icon() -> Image {
    const SIZE: i32 = 32;
    const SHADES: [u8; 3] = [255, 204, 158];
    const INK: u8 = 70;
    // 0 outside, 1 top, 2 left, 3 right.
    let face = |x: i32, y: i32| -> u8 {
        if x < 0 || y < 0 || x >= SIZE || y >= SIZE {
            return 0;
        }
        let dx = (x as f32 + 0.5 - 16.0) / 16.0;
        let py = y as f32 + 0.5;
        if dx.abs() + ((py - 8.0) / 8.0).abs() <= 1.0 {
            return 1;
        }
        let edge = 16.0 - 8.0 * dx.abs();
        if py >= edge && py <= edge + 16.0 {
            if dx < 0.0 { 2 } else { 3 }
        } else {
            0
        }
    };
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let f = face(x, y);
            if f == 0 {
                data.extend([0, 0, 0, 0]);
                continue;
            }
            let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| face(x + dx, y + dy) != f);
            let v = if edge { INK } else { SHADES[f as usize - 1] };
            data.extend([v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::nearest();
    img
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tooltip>()
            .add_systems(Startup, spawn)
            .add_systems(Update, track_hover.before(crate::player::PlayerSet))
            .add_systems(Update, (toggles, paint, texts, tooltip));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Registry<ItemDef> {
        let mut r = Registry::default();
        r.insert(ron::from_str::<ItemDef>(r#"(id: "dirt", stack_max: 10)"#).unwrap());
        r
    }

    #[test]
    fn split_goes_to_an_empty_slot_and_never_merges() {
        let items = items();
        let mut slots = vec![
            Some(Stack { item: 0, n: 7 }),
            Some(Stack { item: 0, n: 2 }),
            None,
        ];
        split(&mut slots, 0, 4, &items);
        assert_eq!(slots[0].unwrap().n, 3);
        assert_eq!(slots[1].unwrap().n, 2);
        assert_eq!(slots[2].unwrap().n, 4);
        // No room left: nothing moves.
        split(&mut slots, 0, 1, &items);
        assert_eq!(slots[0].unwrap().n, 3);
    }

    #[test]
    fn icon_has_three_faces_and_clear_corners() {
        let img = cube_icon();
        let data = img.data.as_ref().unwrap();
        let px = |x: usize, y: usize| &data[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(px(0, 0)[3], 0);
        assert_eq!(px(16, 8), &[255, 255, 255, 255]);
        assert_eq!(px(8, 22), &[204, 204, 204, 255]);
        assert_eq!(px(24, 22), &[158, 158, 158, 255]);
    }
}
