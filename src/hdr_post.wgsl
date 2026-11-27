struct PostParams {
    exposure_ev: f32,
    bloom_threshold: f32,
    bloom_strength: f32,
    encode_srgb: f32,
};

struct PostVertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var input_color: texture_2d<f32>;
@group(0) @binding(1) var linear_clamp: sampler;
@group(0) @binding(2) var<uniform> params: PostParams;
@group(0) @binding(3) var bloom_color: texture_2d<f32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex: u32) -> PostVertex {
    let uv = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    let clip = uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    return PostVertex(vec4<f32>(clip, 0.0, 1.0), uv);
}

fn read_input(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(input_color, linear_clamp, uv, 0.0).rgb;
}

fn exposure() -> f32 {
    return exp2(clamp(params.exposure_ev, -8.0, 8.0));
}

fn bright_sample(uv: vec2<f32>, gain: f32) -> vec3<f32> {
    let light = clamp(read_input(uv) * gain, vec3<f32>(0.0), vec3<f32>(65504.0));
    let peak = max(light.r, max(light.g, light.b));
    let threshold = max(params.bloom_threshold, 0.0);
    let knee = max(0.5 * threshold, 0.0001);
    let soft = clamp(peak - threshold + knee, 0.0, 2.0 * knee);
    let excess = max(peak - threshold, soft * soft / (4.0 * knee));
    return light * (excess / max(peak, 0.0001));
}

@fragment
fn fs_bright(input: PostVertex) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(input_color));
    let gain = exposure();
    // Четыре билинейные выборки покрывают участок 4×4 исходных пикселей.
    let rgb = (
        bright_sample(input.uv + texel * vec2<f32>(-1.0, -1.0), gain) +
        bright_sample(input.uv + texel * vec2<f32>( 1.0, -1.0), gain) +
        bright_sample(input.uv + texel * vec2<f32>(-1.0,  1.0), gain) +
        bright_sample(input.uv + texel * vec2<f32>( 1.0,  1.0), gain)
    ) * 0.25;
    return vec4<f32>(rgb, 1.0);
}

fn blur(uv: vec2<f32>, axis: vec2<f32>) -> vec4<f32> {
    let texel = axis / vec2<f32>(textureDimensions(input_color));
    let near_offset = texel * 1.3846154;
    let far_offset = texel * 3.2307692;
    // Вес девяти отсчётов Гаусса сводится к пяти билинейным выборкам.
    let rgb = read_input(uv) * 0.2270270
        + (read_input(uv - near_offset) + read_input(uv + near_offset)) * 0.3162162
        + (read_input(uv - far_offset) + read_input(uv + far_offset)) * 0.0702703;
    return vec4<f32>(rgb, 1.0);
}

@fragment
fn fs_blur_x(input: PostVertex) -> @location(0) vec4<f32> {
    return blur(input.uv, vec2<f32>(1.0, 0.0));
}

@fragment
fn fs_blur_y(input: PostVertex) -> @location(0) vec4<f32> {
    return blur(input.uv, vec2<f32>(0.0, 1.0));
}

fn tone_map(hdr: vec3<f32>) -> vec3<f32> {
    let luminance = dot(hdr, vec3<f32>(0.2126, 0.7152, 0.0722));
    let x = luminance * 0.82;
    let tone = clamp(x * (2.51 * x + 0.03) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
    // Кривая ACES применяется к яркости, чтобы сохранить оттенок материалов.
    let film = hdr * (tone / max(luminance, 0.00001));
    let peak = max(film.r, max(film.g, film.b));
    let shoulder = smoothstep(0.77, 1.38, peak) * 0.46;
    return clamp(mix(film, vec3<f32>(tone), shoulder), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn linear_to_srgb(rgb: vec3<f32>) -> vec3<f32> {
    return select(
        rgb * 12.92,
        1.055 * pow(rgb, vec3<f32>(1.0 / 2.4)) - 0.055,
        rgb > vec3<f32>(0.0031308),
    );
}

@fragment
fn fs_composite(input: PostVertex) -> @location(0) vec4<f32> {
    var hdr = clamp(read_input(input.uv) * exposure(), vec3<f32>(0.0), vec3<f32>(65504.0));
    if params.bloom_strength > 0.0 {
        hdr += textureSampleLevel(bloom_color, linear_clamp, input.uv, 0.0).rgb * params.bloom_strength;
    }
    let linear = tone_map(hdr);
    if params.encode_srgb > 0.5 {
        return vec4<f32>(linear_to_srgb(linear), 1.0);
    }
    return vec4<f32>(linear, 1.0);
}
