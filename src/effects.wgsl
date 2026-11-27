struct ParticleOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) center_kind: vec4<f32>,
    @location(2) @interpolate(flat) tint_alpha: vec4<f32>,
}

@vertex
fn vs_particle(
    @builtin(vertex_index) vertex: u32,
    @location(0) center_radius: vec4<f32>,
    @location(1) velocity_kind: vec4<f32>,
    @location(2) tint_alpha: vec4<f32>,
) -> ParticleOutput {
    let quad = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let uv = quad[vertex];
    // The first two rows of a perspective view-projection are camera axes with positive scale.
    let vp = camera.view_projection;
    let right = normalize(vec3<f32>(vp[0].x, vp[1].x, vp[2].x));
    let up = normalize(vec3<f32>(vp[0].y, vp[1].y, vp[2].y));
    let kind = velocity_kind.w;
    let velocity = vec2<f32>(dot(velocity_kind.xyz, right), dot(velocity_kind.xyz, up));
    let speed = length(velocity);
    let heading = select(vec2<f32>(0.0, 1.0), velocity / max(speed, 0.001), speed > 0.001);
    var offset = uv;
    if kind > 1.5 {
        var scale = vec2<f32>(0.72, 1.4);
        if kind < 2.5 {
            scale = vec2<f32>(0.32, 2.7 + min(speed * 0.28, 1.7));
        } else if kind > 3.5 {
            scale = vec2<f32>(0.78, 1.8 + min(speed * 0.16, 1.5));
        }
        let cross_heading = vec2<f32>(heading.y, -heading.x);
        offset = cross_heading * (uv.x * scale.x) + heading * (uv.y * scale.y);
    }
    let world = center_radius.xyz + (right * offset.x + up * offset.y) * max(center_radius.w, 0.001);
    var result: ParticleOutput;
    result.clip = camera.view_projection * vec4<f32>(world, 1.0);
    result.uv = uv;
    result.center_kind = vec4<f32>(center_radius.xyz, kind);
    result.tint_alpha = tint_alpha;
    return result;
}

@fragment
fn fs_particle(input: ParticleOutput) -> @location(0) vec4<f32> {
    let kind = input.center_kind.w;
    let distance = length(input.uv);
    var alpha = 1.0 - smoothstep(0.16, 1.0, distance);
    let tint = srgb_to_linear(input.tint_alpha.rgb);
    var color = tint;
    if kind < 1.5 {
        let seed = input.center_kind.xz * 1.35 + input.center_kind.y * 0.73;
        let grain = surface_noise(input.uv * 2.6 + seed);
        let fluffy_edge = distance + (0.5 - grain) * 0.28;
        alpha = (1.0 - smoothstep(0.12, 0.97, fluffy_edge)) * mix(0.55, 1.0, grain);
        color = tint * mix(0.63, 1.22, grain) * mix(0.7, 1.0, daylight_ambient());
        if kind > 0.5 {
            alpha *= 0.76;
        }
    } else if kind < 2.5 {
        let stripe = 1.0 - smoothstep(0.10, 0.77, abs(input.uv.x));
        let tail = smoothstep(-0.95, -0.18, input.uv.y) * (1.0 - smoothstep(0.42, 1.0, input.uv.y));
        alpha = stripe * tail;
        color = mix(tint, vec3<f32>(0.61, 0.75, 0.88), 0.35);
    } else if kind < 3.5 {
        alpha = 1.0 - smoothstep(0.42, 1.0, distance);
        let highlight = 1.0 - smoothstep(0.0, 0.45, length(input.uv - vec2<f32>(-0.22, 0.30)));
        color = tint * 0.86 + vec3<f32>(0.36, 0.51, 0.61) * highlight;
    } else {
        let tail = smoothstep(-0.98, 0.42, input.uv.y);
        alpha *= tail;
        color = tint * (1.15 + alpha * 0.35);
    }
    let eye_offset = camera.eye.xyz - input.center_kind.xyz;
    let eye_distance = max(length(eye_offset), 0.001);
    let final_color = apply_fog(color, -eye_offset / eye_distance, eye_distance);
    return vec4<f32>(output_color(final_color).rgb, alpha * input.tint_alpha.a);
}
