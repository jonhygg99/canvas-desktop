// Filtro de color en una sola pasada: brillo, contraste, saturación,
// temperatura, escala de grises y sepia. Neutro = todos a 0.

struct Params {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    temperature: f32,
    grayscale: f32,
    sepia: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Triángulo a pantalla completa sin vertex buffer (idéntico a blur.wgsl).
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: VsOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src, samp, in.uv);
    var c = texel.rgb;

    // Brillo y contraste.
    c = c + vec3<f32>(params.brightness);
    c = (c - vec3<f32>(0.5)) * (1.0 + params.contrast) + vec3<f32>(0.5);

    // Temperatura: cálido sube rojo y baja azul; frío al revés.
    c.r = c.r + params.temperature * 0.15;
    c.b = c.b - params.temperature * 0.15;

    // Saturación alrededor de la luminancia.
    let luma = dot(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), LUMA);
    c = mix(vec3<f32>(luma), c, 1.0 + params.saturation);

    // Escala de grises y sepia como mezclas.
    let luma2 = dot(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), LUMA);
    c = mix(c, vec3<f32>(luma2), params.grayscale);
    let sepia = vec3<f32>(
        dot(c, vec3<f32>(0.393, 0.769, 0.189)),
        dot(c, vec3<f32>(0.349, 0.686, 0.168)),
        dot(c, vec3<f32>(0.272, 0.534, 0.131)),
    );
    c = mix(c, sepia, params.sepia);

    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), texel.a);
}
