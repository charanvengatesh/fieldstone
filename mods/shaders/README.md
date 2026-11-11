# shaders

Optional finishing passes over the picture, all pack content: the engine renders the world into a
small picture (one texel per `pixel_scale` window pixels), runs each pass in
`shaders.ron` over it in order, and blows the result up to the window.

| Pass | Default | What it does |
|------|---------|--------------|
| `outline` | off | inks a texel where brightness or hue jumps against its neighbour |
| `crt` | off | scanlines, a vignette, optional barrel curve |

`F7` cycles plain (the default), outline, crt.

## Writing a pass

A pass is a WGSL fragment shader with these bindings (copy the header from any file in
`shaders/`):

```wgsl
struct PostParams { frame: vec4<f32>, params: array<vec4<f32>, 4> };
@group(2) @binding(0) var<uniform> post: PostParams;   // frame = (seconds, pixels per texel, width, height)
@group(2) @binding(1) var picture: texture_2d<f32>;    // the pass before this one
@group(2) @binding(2) var picture_sampler: sampler;    // nearest
```

The vertex output carries `uv` at `@location(2)`; the texel is `floor(uv * post.frame.zw)`.
Register it in `shaders.ron` with an `order`; `params` are sixteen numbers the shader
reads as `post.params[0..3]`, and scripts can change them with
`set_shader_param(id, index, value)`. `set_shader(id, on)` turns a pass on or off;
`shader_passes()` lists them.
