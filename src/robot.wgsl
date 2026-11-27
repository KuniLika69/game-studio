struct InteriorLight {
    position_radius: vec4<f32>,
    tint_room: vec4<f32>,
    strength: vec4<f32>,
}

struct InteriorRoom {
    low: vec4<f32>,
    high: vec4<f32>,
}

struct NaturalLight {
    sun_direction_strength: vec4<f32>,
    sun_color_enabled: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    ambient: vec4<f32>,
    fog: vec4<f32>,
    weather: vec4<f32>,
    precipitation: vec4<f32>,
    surface_weather: vec4<f32>,
    sky_drift: vec4<f32>,
    basin_count: vec4<u32>,
    basin_bounds: array<vec4<f32>, 48>,
    basin_floor: array<vec4<f32>, 48>,
    track_bounds: vec4<f32>,
    track_positions: array<vec4<f32>, 16>,
    track_directions: array<vec4<f32>, 16>,
}

struct Camera {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    contact_shadows: array<vec4<f32>, 8>,
    atmosphere: vec4<f32>,
    light_view_projection: mat4x4<f32>,
    shadow_map: vec4<f32>,
    far_light_view_projection: mat4x4<f32>,
    shadow_cascade: vec4<f32>,
    interior_lights: array<InteriorLight, 6>,
    interior_rooms: array<InteriorRoom, 4>,
    effect_origin_and_radius: vec4<f32>,
    effect_color_and_strength: vec4<f32>,
    natural: NaturalLight,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var shadow_depth: texture_depth_2d;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;
@group(1) @binding(2) var shadow_far_depth: texture_depth_2d;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) model_x: vec4<f32>,
    @location(3) model_y: vec4<f32>,
    @location(4) model_z: vec4<f32>,
    @location(5) model_t: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) style: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) color: vec3<f32>,
    @location(3) @interpolate(flat) style: vec4<f32>,
    @location(4) local: vec3<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(input.model_x, input.model_y, input.model_z, input.model_t);
    let world = model * vec4<f32>(input.position, 1.0);
    let x = input.model_x.xyz;
    let y = input.model_y.xyz;
    let z = input.model_z.xyz;
    // Removing squared scale from each orthogonal axis gives the inverse-transpose.
    let normal_matrix = mat3x3<f32>(x / dot(x, x), y / dot(y, y), z / dot(z, z));
    var output: VertexOutput;
    output.position = camera.view_projection * world;
    output.world_position = world.xyz;
    output.normal = normal_matrix * input.normal;
    output.color = input.color.rgb;
    output.style = input.style;
    output.local = input.position;
    return output;
}

@vertex
fn vs_shadow(input: VertexInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(input.model_x, input.model_y, input.model_z, input.model_t);
    return camera.light_view_projection * model * vec4<f32>(input.position, 1.0);
}

fn room_at(world: vec3<f32>) -> u32 {
    for (var index = 0u; index < 4u; index += 1u) {
        let room = camera.interior_rooms[index];
        if room.low.w > 0.0 && all(world >= room.low.xyz) && all(world <= room.high.xyz) {
            return index + 1u;
        }
    }
    return 0u;
}

fn pcf4_cascade(
    depth_map: texture_depth_2d,
    projection: mat4x4<f32>,
    receiver: vec3<f32>,
) -> f32 {
    let clip = projection * vec4<f32>(receiver, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    if edge <= 0.0 || ndc.z <= 0.0 || ndc.z >= 1.0 {
        return 1.0;
    }

    let radius = camera.shadow_map.x;
    let depth = ndc.z - 0.000082;
    let a = textureSampleCompareLevel(depth_map, shadow_sampler, uv + vec2<f32>( radius,  radius), depth);
    let b = textureSampleCompareLevel(depth_map, shadow_sampler, uv + vec2<f32>( radius, -radius), depth);
    let c = textureSampleCompareLevel(depth_map, shadow_sampler, uv + vec2<f32>(-radius,  radius), depth);
    let d = textureSampleCompareLevel(depth_map, shadow_sampler, uv + vec2<f32>(-radius, -radius), depth);
    return mix(1.0, (a + b + c + d) * 0.25, smoothstep(0.0, 0.07, edge) * camera.shadow_map.z);
}

fn cascaded_shadow(world: vec3<f32>, receiver_bias: vec3<f32>) -> f32 {
    let view_depth = (camera.view_projection * vec4<f32>(world, 1.0)).w;
    let half_width = max(camera.shadow_cascade.z * 0.5, 0.001);
    let start = camera.shadow_cascade.y - half_width;
    let end = camera.shadow_cascade.y + half_width;
    var near = 1.0;
    if view_depth < end {
        near = pcf4_cascade(
            shadow_depth, camera.light_view_projection, world + receiver_bias * camera.shadow_map.y,
        );
        if view_depth <= start {
            return near;
        }
    }
    let far = pcf4_cascade(
        shadow_far_depth, camera.far_light_view_projection, world + receiver_bias * camera.shadow_cascade.x,
    );
    if view_depth >= end {
        return far;
    }
    // В полосе перекрытия читаем обе карты, чтобы скрыть границу каскадов.
    return mix(near, far, smoothstep(start, end, view_depth));
}

fn directional_shadow(world: vec3<f32>, normal: vec3<f32>, room_index: u32) -> f32 {
    if camera.natural.sun_direction_strength.w <= 0.005 {
        return 1.0;
    }
    let light_direction = camera.natural.sun_direction_strength.xyz;
    if room_index != 0u {
        let room = camera.interior_rooms[room_index - 1u];
        let at_roof = world + light_direction * ((room.high.y - world.y) / light_direction.y);
        // The continuous roof still blocks sunlight when individual map taps straddle a roof edge.
        if all(at_roof.xz >= room.low.xz - 0.28) && all(at_roof.xz <= room.high.xz + 0.28) {
            return 0.03;
        }
    }
    let light_angle = max(dot(normal, light_direction), 0.0);
    if camera.shadow_cascade.w > 0.5 {
        return cascaded_shadow(world, normal * (0.59 + 1.05 * (1.0 - light_angle)));
    }
    let receiver_offset = camera.shadow_map.y * (0.59 + 1.05 * (1.0 - light_angle));
    let clip = camera.light_view_projection * vec4<f32>(world + normal * receiver_offset, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    if edge <= 0.0 || ndc.z <= 0.0 || ndc.z >= 1.0 {
        return 1.0;
    }

    let radius = camera.shadow_map.x * 1.45;
    let depth = ndc.z - 0.000082;
    let center = textureSampleCompareLevel(shadow_depth, shadow_sampler, uv, depth);
    let a = textureSampleCompareLevel(shadow_depth, shadow_sampler, uv + vec2<f32>(radius, radius), depth);
    let b = textureSampleCompareLevel(shadow_depth, shadow_sampler, uv + vec2<f32>(radius, -radius), depth);
    let c = textureSampleCompareLevel(shadow_depth, shadow_sampler, uv + vec2<f32>(-radius, radius), depth);
    let d = textureSampleCompareLevel(shadow_depth, shadow_sampler, uv + vec2<f32>(-radius, -radius), depth);
    let filtered = (center * 2.0 + a + b + c + d) / 6.0;
    return mix(1.0, filtered, smoothstep(0.0, 0.07, edge) * camera.shadow_map.z);
}

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return select(rgb / 12.92, pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb > vec3<f32>(0.04045));
}

fn output_color(linear: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(clamp(linear, vec3<f32>(0.0), vec3<f32>(65504.0)), 1.0);
}

fn daylight_ambient() -> f32 {
    let authored = mix(0.30, camera.atmosphere.y, 0.70);
    if camera.natural.sun_color_enabled.w > 0.5 {
        let brightness = dot(camera.natural.ambient.xyz, vec3<f32>(0.2126, 0.7152, 0.0722));
        return authored * clamp(brightness * 2.25, 0.12, 1.0);
    }
    return authored;
}

fn primary_sun(legacy: vec3<f32>) -> vec3<f32> {
    let direct = camera.atmosphere.y * camera.natural.sun_direction_strength.w;
    if camera.natural.sun_color_enabled.w > 0.5 {
        return max(legacy.r, max(legacy.g, legacy.b)) * camera.natural.sun_color_enabled.xyz * direct;
    }
    return legacy * direct;
}

fn sky_color(direction: vec3<f32>) -> vec3<f32> {
    let playing = camera.atmosphere.x;
    let height = pow(max(0.0, direction.y), 0.58);
    let to_sun = max(0.0, dot(direction, camera.natural.sun_direction_strength.xyz));
    let sun_haze = pow(to_sun, 13.0);
    if camera.natural.sun_color_enabled.w > 0.5 {
        let natural_sky = mix(camera.natural.sky_horizon.xyz, camera.natural.sky_top.xyz, height);
        let glow = camera.natural.sun_color_enabled.xyz * camera.natural.sun_direction_strength.w * sun_haze * 0.20;
        return (natural_sky + glow) * mix(0.25, camera.atmosphere.y, 0.75);
    }
    let horizon = mix(vec3<f32>(0.28, 0.34, 0.41), vec3<f32>(0.65, 0.72, 0.77), playing);
    let zenith = mix(vec3<f32>(0.10, 0.15, 0.23), vec3<f32>(0.20, 0.45, 0.68), playing);
    let sky = srgb_to_linear(mix(horizon, zenith, height));
    return (sky + vec3<f32>(0.21, 0.126, 0.054) * sun_haze * (0.25 + playing * 0.75)) * daylight_ambient();
}

fn snow_print(world: vec3<f32>) -> f32 {
    if camera.natural.basin_count.z == 0u || any(world.xz < camera.natural.track_bounds.xy)
        || any(world.xz > camera.natural.track_bounds.zw) {
        return 0.0;
    }
    var pressed = 0.0;
    for (var index = 0u; index < camera.natural.basin_count.z; index += 1u) {
        let stamp = camera.natural.track_positions[index];
        let size = abs(stamp.w);
        let delta = world.xz - stamp.xz;
        if abs(world.y - stamp.y) > 0.080 || dot(delta, delta) > size * size * 1.82 {
            continue;
        }
        let direction = camera.natural.track_directions[index];
        let p = vec2<f32>(dot(delta, vec2<f32>(direction.y, -direction.x)), dot(delta, direction.xy)) / size;
        var shape = 0.0;
        if stamp.w < 0.0 {
            let pad = length((p + vec2<f32>(0.0, 0.22)) / vec2<f32>(0.53, 0.49));
            let inner = length((abs(p - vec2<f32>(0.0, 0.42)) - vec2<f32>(0.18, 0.0)) / vec2<f32>(0.19, 0.24));
            let outer = length((abs(p - vec2<f32>(0.0, 0.20)) - vec2<f32>(0.50, 0.0)) / vec2<f32>(0.18, 0.23));
            shape = 1.0 - smoothstep(0.70, 1.09, min(pad, min(inner, outer)));
        } else {
            let heel = length((p + vec2<f32>(0.0, 0.36)) / vec2<f32>(0.38, 0.47));
            let toe = length((p - vec2<f32>(0.0, 0.29)) / vec2<f32>(0.46, 0.63));
            let tread = 0.84 + smoothstep(-0.26, 0.29, sin(p.y * 19.0 + abs(p.x) * 4.6)) * 0.16;
            shape = (1.0 - smoothstep(0.77, 1.08, min(heel, toe))) * tread;
        }
        pressed = max(pressed, shape * direction.z);
    }
    return pressed;
}

fn snow_cover(world: vec3<f32>, normal: vec3<f32>, room: u32) -> f32 {
    let snow = camera.natural.surface_weather.y;
    if snow <= 0.002 || room != 0u || normal.y < 0.27 {
        return 0.0;
    }
    let air = camera.natural.sky_drift.zw;
    let wind = air / max(length(air), 0.3);
    let lee = dot(normal.xz, wind) * clamp(length(air) * 0.035, 0.0, 0.31);
    let slope = smoothstep(0.27, 0.89, normal.y + lee);
    let along = dot(world.xz, wind);
    let across = dot(world.xz, vec2<f32>(wind.y, -wind.x));
    let drifting = surface_noise(vec2<f32>(along * 0.39, across * 0.94) + vec2<f32>(8.5, 5.3));
    let pattern = surface_noise(world.xz * 1.37 + 3.6) * 0.41 + drifting * 0.35
        + surface_noise(world.xz * 8.1 + world.y * 0.58) * 0.24;
    let deposit = smoothstep(0.61 - snow * 0.34, 0.81 - snow * 0.22, pattern);
    let undisturbed = clamp(slope * snow * (0.25 + 0.74 * deposit), 0.0, 1.0);
    let imprint = snow_print(world) * smoothstep(0.35, 0.72, normal.y);
    return undisturbed * (1.0 - imprint * 0.84);
}

fn snow_color(world: vec3<f32>) -> vec3<f32> {
    let grain_fade = 1.0 - smoothstep(3.4, 16.0, distance(camera.eye.xyz, world));
    let crystal = surface_noise(world.xz * 82.7 + world.y * 3.8);
    let soft_drift = surface_noise(world.xz * 2.66);
    return mix(vec3<f32>(0.72, 0.80, 0.88), vec3<f32>(0.89, 0.92, 0.96), soft_drift)
        * (0.95 + crystal * 0.11 * grain_fade);
}

fn weather_wetness(world: vec3<f32>, normal: vec3<f32>, room: u32) -> f32 {
    let rain = camera.natural.surface_weather.x;
    if rain <= 0.002 || room != 0u {
        return 0.0;
    }
    let intermittency = 0.64 + 0.36 * smoothstep(0.20, 0.81, surface_noise(world.xz * 2.1 + world.y * vec2<f32>(0.18, 2.66)));
    let air = camera.natural.sky_drift.zw;
    let wind = air / max(length(air), 0.3);
    let windward = clamp(-dot(normal.xz, wind), 0.0, 1.0) * min(length(air) * 0.075, 0.5);
    return rain * intermittency * max(smoothstep(0.05, 0.90, normal.y), windward);
}

fn apply_fog(color: vec3<f32>, direction: vec3<f32>, distance: f32) -> vec3<f32> {
    let playing = camera.atmosphere.x;
    let far = smoothstep(mix(21.0, 18.0, playing), mix(83.0, 74.0, playing), distance);
    if camera.natural.sun_color_enabled.w > 0.5 {
        let weather_precip = max(camera.natural.weather.x, camera.natural.precipitation.x);
        let near_clear = mix(2.0, 5.0, weather_precip);
        let veil = mix(1.0, 0.68 + 0.32 * smoothstep(17.0, 64.0, distance), weather_precip);
        var depth = max(distance - near_clear, 0.0) * camera.natural.fog.w * veil;
        let mist_height = camera.natural.ambient.w;
        if mist_height > 0.0 && depth > 0.0 {
            let eye_y = camera.eye.y + direction.y * 2.0;
            let fragment_y = camera.eye.y + direction.y * distance;
            let low = min(eye_y, fragment_y);
            let high = max(eye_y, fragment_y);
            let span = high - low;
            let above = max(high, 0.0) - max(low, 0.0);
            let height_range = 0.69314718 * above / max(0.05, mist_height);
            // Integrate the exponential atmosphere between the two endpoints.
            var average = exp2(-max(low, 0.0) / max(0.05, mist_height));
            if height_range > 0.001 {
                average *= (1.0 - exp(-height_range)) / height_range;
            }
            if span > 0.001 {
                average = mix(1.0, average, above / span);
            }
            depth *= mix(0.24, 1.86, average);
        }
        let weather = 1.0 - exp(-depth);
        let tint = camera.natural.fog.xyz * mix(0.25, camera.atmosphere.y, 0.75);
        return mix(mix(color, tint, weather), sky_color(direction), far);
    }
    if far > 0.0 {
        return mix(color, sky_color(direction), far);
    }
    return color;
}

fn hash_cell(point: vec2<f32>) -> f32 {
    var p = fract(point.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
    p += vec3<f32>(dot(p, p.yzx + 33.33));
    return fract((p.x + p.y) * p.z);
}

fn surface_noise(point: vec2<f32>) -> f32 {
    let cell = floor(point);
    let fraction = fract(point);
    let blend = fraction * fraction * (3.0 - 2.0 * fraction);
    return mix(
        mix(hash_cell(cell), hash_cell(cell + vec2<f32>(1.0, 0.0)), blend.x),
        mix(hash_cell(cell + vec2<f32>(0.0, 1.0)), hash_cell(cell + vec2<f32>(1.0)), blend.x),
        blend.y,
    );
}

fn cloud_density(point: vec2<f32>) -> f32 {
    let rotated = vec2<f32>(point.x * 0.87 + point.y * 0.49, point.y * 0.87 - point.x * 0.49);
    return surface_noise(point) * 0.56 + surface_noise(rotated * 2.14 + 12.8) * 0.27
        + surface_noise(rotated * 4.68 - 7.4) * 0.12 + surface_noise(point * 10.11 + 9.3) * 0.05;
}

fn clouded_sky(ray: vec3<f32>, sky: vec3<f32>) -> vec3<f32> {
    if ray.y < 0.018 {
        return sky;
    }
    let cover = camera.natural.precipitation.z;
    let storm = max(camera.natural.weather.x, camera.natural.precipitation.x * 0.65);
    let drift = camera.natural.sky_drift.xy;
    let point = ray.xz * (1.98 / (ray.y + 0.20)) + vec2<f32>(3.1, 0.8) + drift;
    let density = cloud_density(point);
    let limit = mix(0.63, 0.36, cover);
    let cloud = smoothstep(limit - 0.07, limit + 0.12, density);
    let soft_horizon = smoothstep(0.018, 0.16, ray.y);
    let toward_sun = camera.natural.sun_direction_strength.xyz;
    let to_light = normalize(toward_sun.xz + vec2<f32>(0.001));
    let lit_side = clamp((density - surface_noise(point + to_light * 0.38)) * 1.34 + 0.45, 0.0, 1.0);
    let haze = pow(max(0.0, dot(ray, toward_sun)), 8.0);
    let silver = smoothstep(limit - 0.02, limit + 0.075, density) * (1.0 - smoothstep(limit + 0.075, limit + 0.24, density));
    let horizon = camera.natural.sky_horizon.xyz;
    let upper = camera.natural.sky_top.xyz;
    let daylight = clamp(camera.natural.sun_direction_strength.w * 1.9, 0.0, 1.0);
    let neutral = mix(upper, horizon, 0.65);
    let bottom = neutral * mix(0.57, 0.28, storm) * (0.83 + lit_side * 0.37);
    let top = neutral * (0.97 + lit_side * 0.29) + camera.natural.sun_color_enabled.xyz * daylight * (0.13 + haze * silver * 0.49);
    let cloud_light = mix(bottom, top, (0.25 + lit_side * 0.51) * (1.0 - storm * 0.56));
    let cirrus = pow(surface_noise(ray.xz * (2.7 / (ray.y + 0.46)) * vec2<f32>(2.2, 0.79) + drift * 0.35 + 16.8), 4.0);
    let high_air = mix(sky, horizon * 1.08, cirrus * daylight * (1.0 - storm * 0.61) * soft_horizon * 0.52);
    return mix(high_air, cloud_light, cloud * soft_horizon * mix(0.85, 0.97, storm));
}

struct SkyOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) vertex: u32) -> SkyOutput {
    let triangle = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var output: SkyOutput;
    output.ndc = triangle[vertex];
    output.position = vec4<f32>(output.ndc, 1.0, 1.0);
    return output;
}

@fragment
fn fs_sky(input: SkyOutput) -> @location(0) vec4<f32> {
    let world = camera.inverse_view_projection * vec4<f32>(input.ndc, 1.0, 1.0);
    let ray = normalize(world.xyz - camera.eye.xyz * world.w);
    var sky = sky_color(ray);
    if camera.atmosphere.x > 0.5 && ray.y > 0.012 {
        let cloud_plane = ray.xz * (1.72 / (ray.y + 0.27)) + vec2<f32>(3.1, 0.8) + camera.natural.sky_drift.xy;
        let density = surface_noise(cloud_plane) * 0.75
            + surface_noise(cloud_plane * 2.16 + 12.8) * 0.19
            + surface_noise(cloud_plane * 5.1 - 7.4) * 0.06;
        let cover = camera.natural.precipitation.z;
        let clouds = smoothstep(0.53 - 0.34 * cover, 0.74 - 0.25 * cover, density) * smoothstep(0.020, 0.125, ray.y);
        let facing_sun = dot(ray, camera.natural.sun_direction_strength.xyz);
        let sunlight = pow(max(0.0, facing_sun), 6.0);
        let sun_disk = smoothstep(0.99943, 0.99981, facing_sun);
        if camera.natural.sun_color_enabled.w > 0.5 {
            sky = clouded_sky(ray, sky);
        } else {
            let cloud_light = vec3<f32>(0.68, 0.72, 0.75) + vec3<f32>(0.30, 0.23, 0.15) * sunlight;
            sky = mix(sky, cloud_light * daylight_ambient(), clouds * 0.72);
        }
        sky += primary_sun(vec3<f32>(1.75, 1.40, 0.95)) * sun_disk * (1.0 - cover * 0.88);
    }
    return output_color(sky);
}

fn grid_line(xz: vec2<f32>, pixel_width: vec2<f32>) -> f32 {
    let distance_to_line = abs(fract(xz + 0.5) - 0.5);
    let width = max(pixel_width, vec2<f32>(0.0001));
    let coverage = 1.0 - smoothstep(width * 0.16, width * 0.85, distance_to_line);
    return max(coverage.x, coverage.y);
}

struct LocalLightSample {
    direction: vec3<f32>,
    radiance: vec3<f32>,
}

fn sample_local_lamp(lamp: InteriorLight, world: vec3<f32>, point_room: u32) -> LocalLightSample {
    let radius = lamp.position_radius.w;
    if radius <= 0.0 || u32(lamp.tint_room.w) != point_room {
        return LocalLightSample(vec3<f32>(0.0), vec3<f32>(0.0));
    }
    let to_lamp = lamp.position_radius.xyz - world;
    let distance_squared = dot(to_lamp, to_lamp);
    let taper = max(1.0 - distance_squared / (radius * radius), 0.0);
    let falloff = taper * taper / (1.0 + distance_squared * 0.165);
    return LocalLightSample(
        to_lamp * inverseSqrt(max(distance_squared, 0.000001)),
        srgb_to_linear(lamp.tint_room.rgb) * lamp.strength.x * 2.48 * falloff,
    );
}

fn indoor_light(world: vec3<f32>, normal: vec3<f32>, point_room: u32) -> vec3<f32> {
    var color = vec3<f32>(0.0);
    for (var index = 0u; index < 6u; index += 1u) {
        let sample = sample_local_lamp(camera.interior_lights[index], world, point_room);
        let facing = max(dot(normal, sample.direction), 0.0);
        color += sample.radiance * (0.25 + 0.75 * facing);
    }
    return min(color, vec3<f32>(1.6)) * select(1.0, camera.atmosphere.z, point_room != 0u);
}

fn reflected_lamps(world: vec3<f32>, ray: vec3<f32>, roughness: f32) -> vec3<f32> {
    var light = vec3<f32>(0.0);
    for (var index = 0u; index < 6u; index += 1u) {
        let lamp = camera.interior_lights[index];
        let radius = lamp.position_radius.w;
        if radius > 0.0 && lamp.tint_room.w == 0.0 {
            let offset = lamp.position_radius.xyz - world;
            let distance = length(offset);
            let alignment = max(dot(ray, offset / max(distance, 0.001)), 0.0);
            let spread = mix(114.0, 12.0, roughness);
            let glow = pow(alignment, spread) + 0.16 * pow(alignment, spread * 0.21);
            let falloff = 1.0 - smoothstep(radius * 0.49, radius * 1.9, distance);
            light += srgb_to_linear(lamp.tint_room.rgb) * lamp.strength.x * glow * falloff;
        }
    }
    return min(light, vec3<f32>(2.5));
}

fn effect_light(world: vec3<f32>) -> vec3<f32> {
    let radius = camera.effect_origin_and_radius.w;
    if radius <= 0.0 || camera.effect_color_and_strength.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    let local = (world - camera.effect_origin_and_radius.xyz) * vec3<f32>(1.0, 0.79, 1.0);
    let reach = max(1.0 - dot(local, local) / (radius * radius), 0.0);
    return srgb_to_linear(camera.effect_color_and_strength.rgb)
        * camera.effect_color_and_strength.w * reach * reach;
}

fn ambient_day(normal: vec3<f32>, room: u32) -> vec3<f32> {
    let hemisphere = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
    if camera.natural.sun_color_enabled.w > 0.5 {
        let outside = camera.natural.ambient.xyz * mix(0.48, 1.14, hemisphere)
            * mix(0.30, camera.atmosphere.y, 0.70);
        if room != 0u {
            // Indirect outdoor light falls off indoors; authored lamps remain independent.
            return outside * 0.33 + vec3<f32>(0.12, 0.104, 0.09) * mix(0.66, 1.0, hemisphere);
        }
        return outside;
    }
    if room != 0u {
        return mix(vec3<f32>(0.29, 0.265, 0.23), vec3<f32>(0.43, 0.40, 0.36), hemisphere);
    }
    return mix(vec3<f32>(0.158, 0.175, 0.205), vec3<f32>(0.39, 0.44, 0.49), hemisphere) * daylight_ambient();
}

fn contact_visibility(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let upward = smoothstep(0.54, 0.94, normal.y);
    if upward <= 0.0 {
        return 1.0;
    }
    var darkness = 0.0;
    for (var index = 0u; index < 8u; index += 1u) {
        let contact = camera.contact_shadows[index];
        let delta = contact.xyz - world;
        if contact.w > 0.0 && delta.y >= -0.055 && delta.y < 0.68 {
            let lift = max(0.0, delta.y);
            let radius = contact.w + lift * 0.38;
            let distance_squared = dot(delta.xz, delta.xz) / (radius * radius);
            if distance_squared < 2.2 {
                let fade = smoothstep(-0.055, -0.009, delta.y) * (1.0 - smoothstep(0.06, 0.68, lift));
                darkness = max(darkness, exp2(-3.45 * distance_squared) * fade);
            }
        }
    }
    return 1.0 - darkness * upward * 0.23;
}

fn window_reflection(origin: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    let sky_gap = smoothstep(-0.25, 0.16, ray.y);
    let paving = mix(vec3<f32>(0.086, 0.093, 0.095), vec3<f32>(0.148, 0.152, 0.159), sky_gap) * daylight_ambient();
    let reflected_sky = sky_color(ray)
        + vec3<f32>(0.18, 0.19, 0.20) * smoothstep(0.48, 0.81, surface_noise(ray.xz * (2.8 / (0.65 + abs(ray.y))) + 8.1));
    let backdrop = mix(paving, reflected_sky * 0.94, sky_gap);
    var reflected = backdrop;

    // Use the existing nearby building bounds as simplified reflected silhouettes.
    let ray_sign = select(vec3<f32>(-1.0), vec3<f32>(1.0), ray >= vec3<f32>(0.0));
    let reciprocal = ray_sign / max(abs(ray), vec3<f32>(0.0001));
    var closest = 29.0;
    for (var index = 0u; index < 4u; index += 1u) {
        let room = camera.interior_rooms[index];
        if room.low.w > 0.0 {
            let low = (room.low.xyz - origin) * reciprocal;
            let high = (room.high.xyz - origin) * reciprocal;
            let entry = min(low, high);
            let exit = max(low, high);
            let near = max(entry.x, max(entry.y, entry.z));
            let far = min(exit.x, min(exit.y, exit.z));
            if near > 0.075 && near < min(closest, far) {
                closest = near;
                let tint = 0.83 + 0.19 * hash_cell(room.low.xz);
                let silhouette = vec3<f32>(0.115, 0.119, 0.124) * tint * daylight_ambient();
                reflected = mix(backdrop, silhouette, 0.67 * (1.0 - smoothstep(16.0, 29.0, near)));
            }
        }
    }
    return reflected;
}

fn architecture_color(
    base: vec3<f32>, world: vec3<f32>, normal: vec3<f32>, pixel: vec3<f32>, local: vec3<f32>, tag: f32, room: u32,
) -> vec3<f32> {
    // Axial projection follows solid faces and needs no extra stored UVs.
    let wall_axis = select(world.x, world.z, abs(normal.x) > abs(normal.z));
    let wall_pixel = select(pixel.x, pixel.z, abs(normal.x) > abs(normal.z));
    if tag == 2.0 {
        if room != 0u {
            let wall = vec2<f32>(wall_axis, world.y);
            let room_floor = camera.interior_rooms[room - 1u].low.y;
            let plaster = mix(vec3<f32>(0.45, 0.41, 0.34), base, 0.53);
            let roughness = surface_noise(wall * 3.2) * 0.11
                + (surface_noise(wall * 39.0) - 0.5) * 0.20 * (1.0 - smoothstep(0.018, 0.067, max(wall_pixel, pixel.y)));
            let skirting = smoothstep(room_floor + 0.065, room_floor + 0.090, world.y);
            return plaster * (0.87 + roughness) * mix(0.59, 1.0, skirting);
        }
        if abs(normal.y) > 0.66 {
            return base * (0.86 + 0.17 * surface_noise(world.xz * 5.4));
        }
        let row = world.y * 7.85;
        let uv = vec2<f32>(wall_axis * 2.95 + floor(row) * 0.5, row);
        let span = max(vec2<f32>(wall_pixel * 2.95, pixel.y * 7.85), vec2<f32>(0.001));
        let mortar_distance = abs(fract(uv + 0.5) - 0.5);
        let width = max(span.x, span.y);
        let wall = vec2<f32>(wall_axis, world.y);
        let weather = surface_noise(wall * vec2<f32>(1.2, 0.68)) * 0.74
            + surface_noise(wall * vec2<f32>(7.1, 2.4) - 3.0) * 0.26;
        let fine_weight = 1.0 - smoothstep(0.014, 0.16, width);
        let specks = (surface_noise(wall * 48.0 + 1.15) - 0.5) * fine_weight;
        let edges = 1.0 - smoothstep(vec2<f32>(0.019), vec2<f32>(0.043) + span * 0.57, mortar_distance + specks * 0.022);
        let detail = 1.0 - smoothstep(0.16, 0.62, width);
        let low_dirt = 0.84 + 0.16 * smoothstep(0.009, 0.43, world.y + (weather - 0.5) * 0.29);
        let phase = fract(row);
        let top_edge = smoothstep(0.86 - span.y, 0.975, phase);
        let lower_edge = 1.0 - smoothstep(0.03, 0.17 + span.y, phase);
        let bevel = (top_edge * 0.12 - lower_edge * 0.12) * detail;
        let blocks = base * (0.78 + weather * 0.31 + hash_cell(floor(uv)) * 0.22 + specks * 0.21 + bevel);
        let mortar = mix(base * 0.70, vec3<f32>(0.21, 0.20, 0.18), 0.19) * (0.79 + weather * 0.36);
        return mix(base * (0.89 + weather * 0.22), mix(blocks, mortar, max(edges.x, edges.y)), detail) * low_dirt;
    }
    if tag == 3.0 || tag == 4.0 {
        let slab_frequency = select(vec2<f32>(0.86), vec2<f32>(1.73), room != 0u);
        let frequency = select(slab_frequency, vec2<f32>(2.4, 4.7), tag == 4.0);
        let uv = world.xz * frequency;
        let span = max(pixel.xz * frequency, vec2<f32>(0.001));
        let width = max(span.x, span.y);
        let grout = grid_line(uv, max(span, vec2<f32>(0.033)));
        let detail = 1.0 - smoothstep(0.19, 0.60, width);
        let stone = surface_noise(world.xz * 9.5) - 0.5;
        let large = surface_noise(world.xz * 1.29 + vec2<f32>(14.0, 3.0));
        let tile = base * (0.71 + hash_cell(floor(uv)) * 0.23 + large * 0.29
            + 0.29 * stone * (1.0 - smoothstep(0.08, 0.35, width)));
        return mix(base * (0.91 + large * 0.14), mix(tile, base * 0.48, grout * 0.75), detail);
    }
    if tag == 7.0 {
        let horizontal = abs(normal.y) > 0.65;
        let projected = select(vec2<f32>(wall_axis, world.y), world.xz, horizontal);
        let cross_pixel = select(wall_pixel, pixel.x, horizontal);
        let drift = (surface_noise(projected * vec2<f32>(2.9, 1.2)) - 0.5) * 0.052;
        let aligned = vec2<f32>(projected.x + drift, projected.y);
        let streaks = (surface_noise(aligned * vec2<f32>(23.0, 1.9) + 9.1) - 0.5) * (1.0 - smoothstep(0.015, 0.082, cross_pixel));
        let fibers = (surface_noise(aligned * vec2<f32>(98.0, 4.7)) - 0.5) * (1.0 - smoothstep(0.006, 0.030, cross_pixel));
        let stain = surface_noise(projected * vec2<f32>(5.1, 2.05)) - 0.5;
        return base * (0.94 + stain * 0.20 + streaks * 0.34 + fibers * 0.14);
    }
    if tag == 9.0 {
        let grain = surface_noise(world.xz * 63.0) - 0.5;
        let close = 1.0 - smoothstep(0.009, 0.034, max(pixel.x, pixel.z));
        let stripe = max(abs(local.x), abs(local.z));
        let binding = smoothstep(0.453, 0.469, stripe) - smoothstep(0.485, 0.497, stripe);
        return base * (0.92 + grain * close * 0.39 + binding * 0.27);
    }
    return base;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let rgb = srgb_to_linear(input.color);
    let xz = input.world_position.xz;
    let world_pixel = fwidth(input.world_position);
    let pixel_width = world_pixel.xz;
    let eye_offset = camera.eye.xyz - input.world_position;
    let eye_distance = length(eye_offset);
    let view_dir = eye_offset / max(eye_distance, 0.001);
    let playing = camera.atmosphere.x;
    let room = room_at(input.world_position);
    let effect = effect_light(input.world_position);
    var color = rgb;

    if input.style.x > 0.5 {
        for (var index = 0u; index < camera.natural.basin_count.x; index += 1u) {
            let basin = camera.natural.basin_bounds[index];
            let floor_shape = camera.natural.basin_floor[index];
            let local_basin = (xz - basin.xy) / basin.zw;
            let in_outline = select(all(abs(local_basin) < vec2<f32>(1.0)), dot(local_basin, local_basin) < 1.0, floor_shape.w > 1.5);
            let below_plane = floor_shape.w < 0.5 || dot(xz, floor_shape.xy) > floor_shape.z;
            if in_outline && below_plane {
                discard;
            }
        }
        let fine = grid_line(xz * 2.0, pixel_width * 2.0) * (1.0 - smoothstep(5.0, 24.0, eye_distance));
        let coarse = grid_line(xz * 0.25, pixel_width * 0.25) * (1.0 - smoothstep(9.0, 48.0, eye_distance));
        let lines = max(fine * 0.40, coarse * 0.74) * (1.0 - playing);
        let line_color = srgb_to_linear(mix(vec3<f32>(0.38, 0.49, 0.58), vec3<f32>(0.29, 0.34, 0.37), playing));
        var plane_color = rgb;
        if playing > 0.5 {
            let coarse_ground = surface_noise(xz * 0.29 + vec2<f32>(9.0, 2.3));
            let ground_detail = (surface_noise(xz * 12.6) - 0.5) * (1.0 - smoothstep(8.0, 24.0, eye_distance));
            plane_color *= 0.87 + 0.26 * coarse_ground + 0.15 * ground_detail;
        }
        let ground_snow = snow_cover(input.world_position, vec3<f32>(0.0, 1.0, 0.0), room);
        if ground_snow > 0.0 {
            plane_color = mix(plane_color, snow_color(input.world_position), ground_snow);
        }
        color = mix(plane_color, line_color, lines);
        let width = max(pixel_width * 1.3, vec2<f32>(0.001));
        let axes = (1.0 - smoothstep(width * 0.18, width, abs(xz))) * (1.0 - playing);
        let axis_fade = 1.0 - smoothstep(12.0, 44.0, eye_distance);
        color = mix(color, srgb_to_linear(vec3<f32>(0.23, 0.61, 0.60)), axes.x * axis_fade * 0.60);
        color = mix(color, srgb_to_linear(vec3<f32>(0.69, 0.38, 0.32)), axes.y * axis_fade * 0.56);
        let up = vec3<f32>(0.0, 1.0, 0.0);
        let visibility = directional_shadow(input.world_position, up, room);
        if camera.natural.sun_color_enabled.w > 0.5 {
            let sun_height = max(camera.natural.sun_direction_strength.y, 0.0);
            let wet = weather_wetness(input.world_position, up, room);
            let outdoor = ambient_day(up, room) + primary_sun(vec3<f32>(0.94)) * visibility * sun_height;
            let wet_reflection = sky_color(reflect(-view_dir, up)) * wet * pow(1.0 - max(view_dir.y, 0.0), 4.0) * 0.12;
            color = color * outdoor * (1.0 - wet * 0.27) + wet_reflection;
            color *= contact_visibility(input.world_position, up);
        } else {
            color *= mix(0.49, 1.0, visibility) * contact_visibility(input.world_position, up) * daylight_ambient();
        }
        color += effect * 0.22;
    } else {
        let normal = normalize(input.normal);
        let surface_kind = input.style.w;
        let environment = min(surface_kind, 1.0);
        var albedo = architecture_color(rgb, input.world_position, normal, world_pixel, input.local, surface_kind, room);
        if environment > 0.5 && surface_kind != 5.0 && surface_kind != 8.0 {
            let snow = snow_cover(input.world_position, normal, room);
            let wet = weather_wetness(input.world_position, normal, room);
            albedo *= 1.0 - wet * 0.30;
            if snow > 0.0 {
                albedo = mix(albedo, snow_color(input.world_position), snow);
            }
        }
        let key_dir = camera.natural.sun_direction_strength.xyz;
        let fill_dir = normalize(vec3<f32>(0.64, 0.22, -0.55));
        let key_diffuse = max(dot(normal, key_dir), 0.0);
        var key = key_diffuse;
        if key_diffuse > 0.0 {
            key *= directional_shadow(input.world_position, normal, room);
        }
        let fill = max(dot(normal, fill_dir), 0.0);
        let rim = pow(1.0 - clamp(dot(normal, view_dir), 0.0, 1.0), 3.4);
        let halfway_dir = key_dir + view_dir;
        let halfway = halfway_dir / max(length(halfway_dir), 0.001);
        let specular = pow(max(dot(normal, halfway), 0.0), mix(39.0, 18.0, environment)) * key;
        let bounce = ambient_day(normal, room);
        let direct = primary_sun(vec3<f32>(1.10, 0.994, 0.82)) * key + vec3<f32>(0.19, 0.245, 0.30) * fill * daylight_ambient();
        let ground_contact = mix(1.0, 0.88 + 0.12 * smoothstep(0.02, 0.70, input.world_position.y), environment);
        color = albedo * (bounce + direct + input.style.z + indoor_light(input.world_position, normal, room)) * ground_contact;
        color += effect * (albedo * 0.55 + vec3<f32>(0.065));
        color += primary_sun(vec3<f32>(0.83, 0.91, 1.0)) * specular * mix(0.22, 0.093, environment);
        if surface_kind == 5.0 {
            let reflected_ray = reflect(-view_dir, normal);
            let reflection = window_reflection(input.world_position + normal * 0.045, reflected_ray);
            let facing = clamp(dot(normal, view_dir), 0.0, 1.0);
            let fresnel = 0.40 + 0.56 * pow(1.0 - facing, 3.0);
            let blue_glass = reflection * vec3<f32>(0.72, 0.88, 0.94);
            color = mix(color * 0.61 + vec3<f32>(0.012, 0.022, 0.029), blue_glass, fresnel);
        } else if surface_kind == 6.0 {
            let reflected_ray = reflect(-view_dir, normal);
            let sky_weight = smoothstep(-0.19, 0.21, reflected_ray.y) * select(1.0, 0.24, room != 0u);
            let reflected = mix(vec3<f32>(0.08), sky_color(reflected_ray), sky_weight);
            color += reflected * mix(vec3<f32>(0.35), normalize(albedo + 0.035), 0.38) * (0.06 + rim * 0.21);
        } else if surface_kind == 8.0 {
            color += rgb * 1.05 * select(1.0, camera.atmosphere.z, room != 0u);
        }
        color *= contact_visibility(input.world_position, normal);
        let accent = srgb_to_linear(vec3<f32>(0.21, 0.81, 1.0));
        color += accent * rim * (0.043 + input.style.y * 0.72);
        color += accent * input.style.y * 0.08;
    }

    return output_color(apply_fog(color, -view_dir, eye_distance));
}
