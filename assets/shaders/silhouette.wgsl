// The player's silhouette: the figure texture as a flat tint, drawn with the depth test
// turned round (see SilhouetteArt::specialize), so it shows only where a block covers
// the figure. src/debug.rs draw_behind mirrors this on the CPU for snapshots.

struct SilhouetteParams {
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> silhouette: SilhouetteParams;
@group(2) @binding(1) var figure: texture_2d<f32>;
@group(2) @binding(2) var figure_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(figure, figure_sampler, in.uv);
    if (texel.a < 0.5) {
        discard;
    }
    // A little of the figure's own shading survives so hair and tunic still read.
    let luma = 0.4 + 0.6 * dot(texel.rgb, vec3<f32>(0.3, 0.6, 0.1));
    return vec4<f32>(silhouette.tint.rgb * luma, silhouette.tint.a);
}
