// The player's silhouette: the figure's own vertex colours as a flat tint, drawn with the
// depth test turned round (see SilhouetteArt::specialize), so it shows only where a block
// covers the figure. src/debug.rs draw_model in Mode::Behind mirrors this on the CPU.

struct SilhouetteParams {
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> silhouette: SilhouetteParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // A little of the figure's own shading survives so hair and tunic still read.
    let luma = 0.4 + 0.6 * min(dot(in.color.rgb, vec3<f32>(0.3, 0.6, 0.1)), 1.0);
    return vec4<f32>(silhouette.tint.rgb * luma, silhouette.tint.a);
}
