//! The picture pipeline. The world camera draws into a small render target (the picture,
//! one texel per pixel at `pixel_scale` window pixels each), pack shaders run over it in
//! order (`shaders.ron`, each a full-picture pass reading the one before), and a compositor
//! camera blows the result up to the window with nearest sampling and hosts the UI at the
//! window's own resolution. Mods add passes without touching Rust: a WGSL fragment shader
//! with the bindings in [`PostParams`].

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::image::ImageSampler;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureFormat, TextureUsages,
};
use bevy::shader::Shader;
use bevy::sprite_render::{Material2d, Material2dKey, Material2dPipeline, MeshMaterial2d};
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;

use crate::camera::MainCamera;
use crate::core::settings::Settings;
use crate::core::{Registry, ShaderDef};
use crate::world::stream::Picture;

/// What every pass shader reads at `@group(2) @binding(0)`.
#[derive(ShaderType, Clone, Copy, Debug, PartialEq)]
pub struct PostParams {
    /// x: seconds, y: pixels per texel, z and w: the picture's width and height in texels.
    pub frame: Vec4,
    /// Sixteen numbers from `shaders.ron`, four `vec4`s; scripts can set them.
    pub params: [Vec4; 4],
}

/// One material type runs any pass: the shader is part of the pipeline key, so each pass
/// specialises its own pipeline with its own fragment shader.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(PostKey)]
pub struct PostArt {
    #[uniform(0)]
    pub post: PostParams,
    #[texture(1)]
    #[sampler(2)]
    pub picture: Handle<Image>,
    pub shader: Handle<Shader>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PostKey {
    shader: Handle<Shader>,
}

impl From<&PostArt> for PostKey {
    fn from(m: &PostArt) -> Self {
        Self {
            shader: m.shader.clone(),
        }
    }
}

impl Material2d for PostArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        // Replaced per pass in `specialize`.
        "shaders/blit.wgsl".into()
    }
    fn specialize(
        _pipeline: &Material2dPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = key.bind_group_data.shader.clone();
        }
        Ok(())
    }
}

/// A pass as the chain runs it.
#[derive(Clone, Debug)]
pub struct Pass {
    pub id: String,
    pub order: i32,
    pub enabled: bool,
    pub params: [f32; 16],
    shader: Handle<Shader>,
}

/// The chain's state: the two picture images the passes ping-pong between, the passes,
/// and whether the wiring needs redoing.
#[derive(Resource)]
pub struct Chain {
    pub images: [Handle<Image>; 2],
    pub size: UVec2,
    pub passes: Vec<Pass>,
    /// The compositor's blit shader.
    blit: Handle<Shader>,
    dirty: bool,
}

impl Chain {
    pub fn pass_mut(&mut self, id: &str) -> Option<&mut Pass> {
        self.dirty = true;
        self.passes.iter_mut().find(|p| p.id == id)
    }
}

/// Marks a pass's quad and camera with its index in the chain, and the compositor.
#[derive(Component)]
pub struct Stage(usize);
#[derive(Component)]
pub struct Compositor;

const BLIT: &str = include_str!("../assets/shaders/blit.wgsl");

fn picture_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(
        size.x.max(1),
        size.y.max(1),
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    image.sampler = ImageSampler::nearest();
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image
}

/// Creates the pictures, points the world camera at the first, loads every pack pass and
/// spawns the compositor.
pub fn setup(
    defs: Res<Registry<ShaderDef>>,
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<Settings>,
    mut images: ResMut<Assets<Image>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PostArt>>,
    mut picture: ResMut<Picture>,
    world_cam: Single<Entity, With<MainCamera>>,
    mut commands: Commands,
) {
    let size = picture_size(&window, settings.pixel_scale);
    picture.size = size.as_vec2();
    let a = images.add(picture_image(size));
    let b = images.add(picture_image(size));
    commands
        .entity(*world_cam)
        .insert(RenderTarget::Image(ImageRenderTarget::from(a.clone())));

    let mut passes: Vec<Pass> = defs
        .iter()
        .map(|(_, d)| {
            let mut params = [0.0f32; 16];
            for (i, v) in d.params.iter().take(16).enumerate() {
                params[i] = *v;
            }
            Pass {
                id: d.id.clone(),
                order: d.order,
                enabled: d.enabled,
                params,
                shader: shaders.add(Shader::from_wgsl(
                    d.source.clone(),
                    format!("pack shader {}", d.id),
                )),
            }
        })
        .collect();
    passes.sort_by(|x, y| x.order.cmp(&y.order).then_with(|| x.id.cmp(&y.id)));
    let blit = shaders.add(Shader::from_wgsl(BLIT, "shaders/blit.wgsl"));

    // One camera and quad per pass, on their own render layer so each camera draws only
    // its quad. Targets are wired in `wire`.
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    for (i, pass) in passes.iter().enumerate() {
        let layer = RenderLayers::layer(10 + i);
        commands.spawn((
            Stage(i),
            Camera2d,
            Camera {
                order: 1 + i as isize,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Msaa::Off,
            RenderTarget::Image(ImageRenderTarget::from(b.clone())),
            layer.clone(),
        ));
        commands.spawn((
            Stage(i),
            Mesh2d(quad.clone()),
            MeshMaterial2d(materials.add(PostArt {
                post: PostParams {
                    frame: Vec4::ZERO,
                    params: [Vec4::ZERO; 4],
                },
                picture: a.clone(),
                shader: pass.shader.clone(),
            })),
            Transform::from_scale(Vec3::new(size.x as f32, size.y as f32, 1.0)),
            layer,
        ));
    }
    // The compositor: the window camera, the UI's home, drawing the final picture.
    let layer = RenderLayers::layer(9);
    commands.spawn((
        Compositor,
        Camera2d,
        Camera {
            order: 100,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Msaa::Off,
        IsDefaultUiCamera,
        layer.clone(),
    ));
    commands.spawn((
        Compositor,
        Mesh2d(quad),
        MeshMaterial2d(materials.add(PostArt {
            post: PostParams {
                frame: Vec4::ZERO,
                params: [Vec4::ZERO; 4],
            },
            picture: a.clone(),
            shader: blit.clone(),
        })),
        Transform::from_scale(Vec3::new(window.width(), window.height(), 1.0)),
        layer,
    ));
    commands.insert_resource(Chain {
        images: [a, b],
        size,
        passes,
        blit,
        dirty: true,
    });
}

/// Texels across and down the picture: the window's physical pixels over the pixel scale.
fn picture_size(window: &Window, pixel_scale: u32) -> UVec2 {
    let scale = pixel_scale.clamp(1, 6);
    UVec2::new(
        (window.physical_width() / scale).max(1),
        (window.physical_height() / scale).max(1),
    )
}

/// Keeps the pictures the right size for the window and the pixel scale, and the chain
/// wired: enabled passes alternate between the two pictures; the compositor reads the
/// last one written. Materials are only touched when something changed.
#[allow(clippy::too_many_arguments)]
pub fn wire(
    time: Res<Time>,
    settings: Res<Settings>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut chain: ResMut<Chain>,
    mut picture: ResMut<Picture>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<PostArt>>,
    mut stages: Query<
        (
            &Stage,
            Option<&mut Camera>,
            Option<&mut RenderTarget>,
            Option<&MeshMaterial2d<PostArt>>,
            Option<&mut Transform>,
        ),
        Without<Compositor>,
    >,
    mut compositor: Query<
        (Option<&MeshMaterial2d<PostArt>>, Option<&mut Transform>),
        With<Compositor>,
    >,
) {
    let size = picture_size(&window, settings.pixel_scale);
    if size != chain.size {
        for handle in &chain.images {
            if let Some(mut image) = images.get_mut(handle) {
                image.resize(Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                });
            }
        }
        chain.size = size;
        picture.size = size.as_vec2();
        chain.dirty = true;
    }
    // Time ticks for every enabled pass; only the handful of passes pay for it.
    let frame = Vec4::new(
        time.elapsed_secs(),
        settings.pixel_scale.clamp(1, 6) as f32,
        size.x as f32,
        size.y as f32,
    );
    let any_time = chain.passes.iter().any(|p| p.enabled);
    if !chain.dirty && !any_time {
        return;
    }
    // Which picture each enabled pass reads and writes.
    let mut read = 0usize;
    let mut plan: Vec<Option<(usize, usize)>> = Vec::new();
    for pass in &chain.passes {
        if pass.enabled {
            plan.push(Some((read, 1 - read)));
            read = 1 - read;
        } else {
            plan.push(None);
        }
    }
    for (stage, camera, target, material, transform) in &mut stages {
        let Some(slot) = plan.get(stage.0).copied() else {
            continue;
        };
        if let Some(mut camera) = camera {
            camera.is_active = slot.is_some();
            if let (Some((_, to)), Some(mut target)) = (slot, target) {
                let wanted = RenderTarget::Image(ImageRenderTarget::from(chain.images[to].clone()));
                if chain.dirty {
                    *target = wanted;
                }
            }
        }
        // Only the quad is scaled; the camera entity carries a transform too.
        if let (Some(mut tf), true) = (transform, material.is_some()) {
            let wanted = Vec3::new(size.x as f32, size.y as f32, 1.0);
            if tf.scale != wanted {
                tf.scale = wanted;
            }
        }
        if let (Some(material), Some((from, _))) = (material, slot)
            && let Some(mut m) = materials.get_mut(&material.0)
        {
            let pass = &chain.passes[stage.0];
            m.post.frame = frame;
            for k in 0..4 {
                m.post.params[k] = Vec4::from_slice(&pass.params[k * 4..k * 4 + 4]);
            }
            if chain.dirty {
                m.picture = chain.images[from].clone();
            }
        }
    }
    for (material, transform) in &mut compositor {
        if let (Some(mut tf), true) = (transform, material.is_some()) {
            let wanted = Vec3::new(window.width(), window.height(), 1.0);
            if tf.scale != wanted {
                tf.scale = wanted;
            }
        }
        if chain.dirty
            && let Some(material) = material
            && let Some(mut m) = materials.get_mut(&material.0)
        {
            m.picture = chain.images[read].clone();
            m.post.frame = frame;
            m.shader = chain.blit.clone();
        }
    }
    chain.dirty = false;
}

pub struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::sprite_render::Material2dPlugin::<PostArt>::default())
            .add_systems(Startup, setup.after(crate::camera::setup))
            .add_systems(Update, wire.after(crate::camera::follow));
    }
}
