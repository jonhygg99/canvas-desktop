// Desenfoque gaussiano separable (una dirección por pasada).
// El color se acumula premultiplicado por alfa para no ensuciar los bordes
// transparentes, y se des-premultiplica al final.

struct Params {
    // Dirección de la pasada en texels: (1,0) horizontal, (0,1) vertical.
    dir: vec2<f32>,
    sigma: f32,
    radius: i32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Triángulo que cubre toda la pantalla sin vertex buffer.
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: VsOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(src_tex));
    let texel = params.dir / dims;

    let center = textureSampleLevel(src_tex, src_samp, in.uv, 0.0);
    var acc = vec4<f32>(center.rgb * center.a, center.a);
    var total = 1.0;
    let s2 = 2.0 * params.sigma * params.sigma;

    for (var i = 1; i <= params.radius; i = i + 1) {
        let w = exp(-f32(i * i) / s2);
        let o = f32(i) * texel;
        let a = textureSampleLevel(src_tex, src_samp, in.uv + o, 0.0);
        let b = textureSampleLevel(src_tex, src_samp, in.uv - o, 0.0);
        acc += vec4<f32>(a.rgb * a.a + b.rgb * b.a, a.a + b.a) * w;
        total += 2.0 * w;
    }

    let alpha = acc.a / total;
    var rgb = vec3<f32>(0.0);
    if (alpha > 1e-5) {
        rgb = (acc.rgb / total) / alpha;
    }
    return vec4<f32>(rgb, alpha);
}
