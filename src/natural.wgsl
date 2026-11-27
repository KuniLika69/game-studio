struct WaterOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) level: vec4<f32>,
    @location(3) @interpolate(flat) tint: vec4<f32>,
    @location(4) @interpolate(flat) side: vec3<f32>,
    @location(5) @interpolate(flat) flow: vec2<f32>,
    @location(6) @interpolate(flat) shore: vec4<f32>,
    @location(7) @interpolate(flat) extent: vec2<f32>,
    @location(8) @interpolate(flat) blend_mode: f32,
}

fn water_waves(point: vec2<f32>, phase: f32) -> vec2<f32> {
    let modulation = sin(dot(point, vec2<f32>(0.41, -0.24)) + phase) * 0.72;
    return vec2<f32>(
        dot(point, vec2<f32>(1.17, 0.67)) - phase + modulation,
        dot(point, vec2<f32>(-0.54, 1.33)) + phase - modulation * 0.83,
    );
}

fn water_height(point: vec2<f32>, phase: f32) -> f32 {
    let wave = water_waves(point, phase);
    return dot(sin(wave), vec2<f32>(0.60, 0.31))
        + 0.09 * sin(dot(point, vec2<f32>(0.23, -2.16)) + phase * 2.0);
}

fn water_depth(point: vec2<f32>, level: vec4<f32>, shore: vec4<f32>) -> f32 {
    let shore_floor = shore.z - dot(point, shore.xy);
    let bottom = select(level.w, max(level.w, shore_floor), shore.w > 0.5);
    return level.x - bottom;
}

@vertex
fn vs_water(
    @builtin(vertex_index) vertex: u32,
    @location(0) bounds: vec4<f32>,
    @location(1) level: vec4<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) flow: vec4<f32>,
    @location(4) shore: vec4<f32>,
) -> WaterOutput {
    let cells = camera.natural.basin_count.y;
    let triangle_quad = array<vec2<u32>, 6>(
        vec2<u32>(0, 0), vec2<u32>(0, 1), vec2<u32>(1, 0),
        vec2<u32>(1, 0), vec2<u32>(0, 1), vec2<u32>(1, 1),
    );
    let surface_quads = cells * cells;
    let cell = vertex / 6u;
    let corner = triangle_quad[vertex % 6u];
    var uv = vec2<f32>(vec2<u32>(cell % cells, cell / cells) + corner) / f32(cells);
    var side = vec3<f32>(0.0);
    var top = 1.0;
    if cell >= surface_quads {
        let perimeter = cell - surface_quads;
        let face = perimeter / cells;
        let along = f32(perimeter % cells + corner.x) / f32(cells);
        let border = array<vec4<f32>, 4>(
            vec4<f32>(0.0, along, -1.0, 0.0),
            vec4<f32>(1.0, along, 1.0, 0.0),
            vec4<f32>(along, 0.0, 0.0, -1.0),
            vec4<f32>(along, 1.0, 0.0, 1.0),
        );
        uv = border[face].xy;
        side = vec3<f32>(border[face].z, 0.0, border[face].w);
        top = f32(corner.y);
    }
    let point = bounds.xy + bounds.zw * (uv - 0.5);
    let wave = water_height(point, level.z);
    let depth = water_depth(point, level, shore);
    let crest = level.x + min(level.y, max(depth, 0.0) * 0.45) * wave;
    let bottom = min(crest, max(level.w, -0.005));
    let world = vec3<f32>(point.x, mix(bottom, crest, top), point.y);
    var output: WaterOutput;
    output.position = camera.view_projection * vec4<f32>(world, 1.0);
    output.world = world;
    output.uv = uv;
    output.level = level;
    output.tint = tint;
    output.side = side;
    output.flow = flow.xy;
    output.shore = shore;
    output.extent = bounds.zw;
    output.blend_mode = flow.z;
    return output;
}

struct ImpactRing {
    slope: vec2<f32>,
    glint: f32,
}

fn rain_ring(point: vec2<f32>, time: f32, density: f32) -> ImpactRing {
    let cell = floor(point);
    let seed = hash_cell(cell);
    let center = vec2<f32>(0.28 + hash_cell(cell + 12.7) * 0.44, 0.28 + seed * 0.44);
    let delta = fract(point) - center;
    let distance = length(delta);
    let cycle = time * (1.15 + seed * 0.86) + seed * 7.7;
    let age = fract(cycle);
    let impulse = smoothstep(0.03, 0.11, age) * pow(1.0 - age, 1.55);
    let live = smoothstep(1.001 - density * 0.94, 1.024 - density * 0.83, hash_cell(cell + floor(cycle) * 0.94));
    let band = distance - age * (0.21 + seed * 0.045);
    let crest = exp(-band * band * 1500.0) * impulse * live;
    let flicker = exp(-distance * distance * 6500.0) * (1.0 - smoothstep(0.035, 0.12, age)) * live;
    return ImpactRing((delta / max(distance, 0.001)) * crest * sin(band * 149.0), crest * 0.33 + flicker);
}

@fragment
fn fs_water(input: WaterOutput) -> @location(0) vec4<f32> {
    let point = input.world.xz;
    let phase = input.level.z;
    let wave = water_waves(point, phase);
    let wave_height = water_height(point, phase);
    let signed_depth = water_depth(point, input.level, input.shore);
    let depth = max(0.0, signed_depth);
    let main_slope = min(input.level.y, depth * 0.45) * 17.0 * (vec2<f32>(
        water_height(point + vec2<f32>(0.0588, 0.0), phase),
        water_height(point + vec2<f32>(0.0, 0.0588), phase),
    ) - wave_height);
    let moving = input.world.xz - input.flow;
    let texture_motion = camera.natural.sky_drift.xy * 1.3;
    let ripple_phase = vec2<f32>(3.0, -2.0) * phase + vec2<f32>(
        dot(moving, vec2<f32>(12.1, 6.9)) + 4.7 * surface_noise(moving * 1.87 + texture_motion),
        dot(moving, vec2<f32>(-5.2, 16.2)) + 5.3 * surface_noise(moving * 2.47 - texture_motion),
    );
    let ripple_damping = 1.0 - smoothstep(vec2<f32>(0.7), vec2<f32>(2.6), fwidth(ripple_phase));
    let ripple = cos(ripple_phase) * vec2<f32>(0.022, 0.014) * ripple_damping;
    let wet_air = camera.natural.weather.x;
    let pixel_size = length(fwidth(point));
    let ring_fade = 1.0 - smoothstep(0.023, 0.11, pixel_size);
    var drops = ImpactRing(vec2<f32>(0.0), 0.0);
    if wet_air > 0.002 && ring_fade > 0.0 {
        let time = camera.natural.precipitation.w;
        let a = rain_ring(moving * 2.65, time, wet_air);
        let b = rain_ring(moving.yx * 4.0 + vec2<f32>(0.47, 0.19), time * 1.08, wet_air);
        drops.slope = (a.slope * 0.19 + b.slope.yx * 0.095) * ring_fade * wet_air;
        drops.glint = (a.glint + b.glint * 0.72) * ring_fade * wet_air;
    }
    let slope = main_slope + ripple.x * vec2<f32>(0.85, 0.48) + ripple.y * vec2<f32>(-0.34, 0.94) + drops.slope;
    let side = dot(input.side, input.side) > 0.5;
    let rounded = input.tint.w > 0.5;
    let has_shore = input.shore.w > 0.5;
    let local = input.uv * 2.0 - 1.0;
    let radius = length(local);
    if (rounded && radius > 1.0) || (side && (rounded || has_shore)) || signed_depth <= 0.0 {
        discard;
    }
    let normal = select(normalize(vec3<f32>(-slope.x, 1.0, -slope.y)), input.side, side);
    let eye_offset = camera.eye.xyz - input.world;
    let distance = max(length(eye_offset), 0.001);
    let view = eye_offset / distance;
    if side && dot(view, normal) <= 0.0 {
        discard;
    }
    let outside = room_at(input.world);
    let sky_ray = reflect(-view, normal);
    let facing = clamp(dot(normal, view), 0.0, 1.0);
    let fresnel = 0.022 + 0.978 * pow(1.0 - facing, 5.0);
    let optical_depth = depth / sqrt(0.46 + facing * facing * 0.54);
    let transmittance = exp(-optical_depth * 2.7);
    let rectangular_distance = (vec2<f32>(1.0) - abs(local)) * input.extent * 0.5;
    let ellipse_gradient = length(local / max(radius, 0.001) * 2.0 / input.extent);
    let edge_metres = select(min(rectangular_distance.x, rectangular_distance.y), (1.0 - radius) / max(ellipse_gradient, 0.001), rounded);
    let foam_noise = surface_noise(moving * 4.9) * 0.59 + surface_noise(moving * 11.7 + 8.29) * 0.41;
    let foam_pattern = smoothstep(0.39, 0.70, foam_noise + 0.16 * sin(wave.x + wave.y));
    let swash_depth = max(0.002, 0.048 + wave_height * 0.034 + (foam_noise - 0.5) * 0.034);
    let shore_band = smoothstep(0.007, 0.025, depth) * (1.0 - smoothstep(swash_depth, swash_depth + 0.09, depth));
    let pool_band = (1.0 - smoothstep(0.014, 0.11, edge_metres)) * smoothstep(0.045, 0.30, depth) * 0.34;
    let shoreline = select(0.0, foam_pattern * select(pool_band, shore_band, has_shore), !side);
    var reflect_sky = sky_color(sky_ray);
    if camera.natural.sun_color_enabled.w > 0.5 {
        reflect_sky = clouded_sky(sky_ray, reflect_sky);
    }
    if !side && depth < 0.16 {
        let facade = window_reflection(input.world + normal * 0.025, sky_ray);
        reflect_sky = mix(reflect_sky, facade, 0.70)
            + reflected_lamps(input.world, sky_ray, 0.24) * 0.38;
    }
    let visibility = directional_shadow(input.world, normal, outside);
    let sun = camera.natural.sun_direction_strength.xyz;
    let half_direction = sun + view;
    let half_vector = half_direction / max(length(half_direction), 0.001);
    let sparkle = pow(max(dot(normal, half_vector), 0.0), mix(86.0, 36.0, wet_air)) * visibility;
    let variation = surface_noise(moving * 0.68 + texture_motion * 0.28);
    let deep_color = srgb_to_linear(input.tint.xyz) * vec3<f32>(0.59, 0.91, 1.02);
    let shore_color = deep_color * vec3<f32>(1.9, 1.32, 0.87) + vec3<f32>(0.022, 0.021, 0.008);
    var body = mix(deep_color, shore_color, transmittance) * (0.86 + variation * 0.24) * (ambient_day(normal, outside) * 1.42
        + primary_sun(vec3<f32>(0.44)) * visibility * max(dot(normal, sun), 0.0));
    if has_shore && depth < 0.42 && !side {
        let sunlight = max(camera.natural.sun_direction_strength.w * visibility, 0.0);
        let lattice = abs(sin(moving.x * 8.2 + phase + water_waves(moving, phase).y) + sin(moving.y * 8.1 - phase * 2.0));
        body += vec3<f32>(0.10, 0.15, 0.14) * pow(max(0.0, 1.0 - lattice), 9.0) * sunlight * smoothstep(0.02, 0.10, depth) * transmittance * (1.0 - smoothstep(0.02, 0.11, pixel_size));
    }
    var color = mix(body, reflect_sky, fresnel * (0.71 + visibility * 0.29));
    if side {
        color = mix(body, reflect_sky * 0.38, 0.18 + fresnel * 0.16);
    }
    color += primary_sun(vec3<f32>(1.6)) * sparkle * (0.55 + fresnel * 0.45);
    let foam_color = (ambient_day(normal, outside) * 1.18 + primary_sun(vec3<f32>(0.32))) * vec3<f32>(0.86, 0.94, 0.95);
    color = mix(color, foam_color, shoreline * 0.73);
    color += reflect_sky * (drops.glint * 0.22);
    color = apply_fog(color, -view, distance);
    let feather = select(1.0, smoothstep(0.0, min(0.98, max(0.14, 0.65 * pixel_size)), edge_metres), rounded);
    let thin_edge = select(1.0, smoothstep(0.0, 0.025 + pixel_size * 0.09, depth), has_shore);
    let alpha = clamp(0.19 + (1.0 - transmittance) * 0.72 + fresnel * 0.23 + shoreline * 0.52, 0.0, 0.98) * feather * thin_edge;
    return material_blend_output(color, alpha, input.blend_mode);
}

fn rain_layer(screen: vec2<f32>, time: f32, wind: f32, intensity: f32, size: vec2<f32>, speed: f32, soft: f32) -> f32 {
    let angle = 0.066 + wind * 1.42 + sin(time * 1.46 + speed) * 0.016;
    let x = (screen.x - angle * screen.y) / size.x;
    let column = floor(x);
    let lane = hash_cell(vec2<f32>(column, 13.6 + speed));
    let stride = size.y * (0.78 + lane * 0.46);
    let y = (screen.y - time * speed * (0.79 + lane * 0.40)) / stride;
    let seed = hash_cell(vec2<f32>(column, floor(y)));
    let side = abs(fract(x) - (0.18 + seed * 0.64)) * size.x;
    let top = stride * (0.09 + 0.17 * lane);
    let length = (0.12 + seed * 0.26) * stride;
    let down = (fract(y) * stride - top) / length;
    let head = smoothstep(0.01, 0.79, down) * (1.0 - smoothstep(0.86, 1.0, down));
    let width = (0.30 + seed * 0.38) * soft;
    let coverage = smoothstep(1.0 - intensity * 0.77, 1.009 - intensity * 0.58, seed);
    return coverage * (1.0 - smoothstep(width * 0.44, width + 0.53, side)) * head * intensity;
}

@fragment
fn fs_rain(input: SkyOutput) -> @location(0) vec4<f32> {
    let screen = input.position.xy;
    let intensity = camera.natural.weather.y;
    let snow = camera.natural.precipitation.y;
    let time = camera.natural.precipitation.w;
    let wind = clamp(camera.natural.weather.w, -0.36, 0.36);
    var rain_alpha = 0.0;
    var rain_glint = 0.0;
    if intensity > 0.001 {
        let far = rain_layer(screen + vec2<f32>(139.0, 169.0), time, wind, intensity, vec2<f32>(8.8, 44.0), 345.0, 0.77);
        let middle = rain_layer(screen + vec2<f32>(89.0, 54.0), time, wind * 0.91, intensity, vec2<f32>(18.0, 93.0), 590.0, 1.11);
        let near = rain_layer(screen, time, wind * 0.80, intensity * 0.79, vec2<f32>(47.0, 179.0), 930.0, 1.84);
        rain_alpha = far * 0.14 + middle * 0.37 + near * 0.46;
        rain_glint = middle * 0.42 + near * 0.58;
    }
    var flakes = 0.0;
    if snow > 0.001 {
        let far = snow_layer(screen + vec2<f32>(75.0, 201.0), time, wind, snow, 26.0, 23.0, 0.81, 0.45);
        let middle = snow_layer(screen, time, wind, snow, 43.0, 40.0, 1.68, 0.72);
        let near = snow_layer(screen + vec2<f32>(157.0, 89.0), time, wind * 0.85, snow, 92.0, 72.0, 3.52, 1.04);
        flakes = (far * 0.31 + middle * 0.66 + near * 0.79) * snow;
    }
    let tint = mix(camera.natural.sky_horizon.xyz, vec3<f32>(0.62, 0.75, 0.88), 0.53 + rain_glint * 0.36);
    let color = mix(tint, vec3<f32>(0.94, 0.97, 1.0), flakes / max(flakes + rain_alpha, 0.001));
    return vec4<f32>(output_color(color).rgb, clamp(flakes + rain_alpha, 0.0, 0.92));
}

fn snow_layer(screen: vec2<f32>, time: f32, wind: f32, density: f32, spacing: f32, speed: f32, size: f32, soft: f32) -> f32 {
    let travel = vec2<f32>(wind * speed * time * 2.7 + sin(time * 1.3 + spacing) * spacing * 0.14, speed * time);
    let p = (screen - travel) / spacing;
    let cell = floor(p);
    let seed = hash_cell(cell + spacing);
    let offset = vec2<f32>(hash_cell(cell + 18.4), hash_cell(cell * 1.83 + 5.91));
    let sway = sin(time * (1.1 + seed) + seed * 5.7) * 0.13;
    let center = vec2<f32>(0.24 + offset.x * 0.52 + sway, 0.16 + offset.y * 0.68);
    let offset_px = (fract(p) - center) * spacing;
    let angle = time * (0.96 + seed * 1.41) + seed * 16.2;
    let ellipse = vec2<f32>(dot(offset_px, vec2<f32>(cos(angle), sin(angle))), dot(offset_px, vec2<f32>(-sin(angle), cos(angle))));
    let distance = length(ellipse * vec2<f32>(1.0 + seed * 0.27, 0.69 + seed * 0.35));
    let radius = size * (0.51 + seed * 0.63);
    let coverage = smoothstep(1.0 - density * 0.65, 1.001 - density * 0.43, seed);
    let flake = 1.0 - smoothstep(radius * 0.31, radius + soft, distance);
    return flake * coverage * (0.60 + 0.40 * abs(cos(angle)));
}
