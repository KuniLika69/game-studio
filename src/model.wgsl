struct ModelMaterial {
    base_color: vec4<f32>,
    surface: vec4<f32>,
    emission_alpha: vec4<f32>,
    illustration: vec4<f32>,
}

@group(2) @binding(0) var<uniform> model_material: ModelMaterial;
@group(2) @binding(1) var model_sampler: sampler;
@group(2) @binding(2) var base_color_texture: texture_2d<f32>;
@group(2) @binding(3) var surface_texture: texture_2d<f32>;
@group(2) @binding(4) var normal_texture: texture_2d<f32>;
@group(2) @binding(5) var emissive_texture: texture_2d<f32>;

struct ModelInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) model_x: vec4<f32>,
    @location(5) model_y: vec4<f32>,
    @location(6) model_z: vec4<f32>,
    @location(7) model_t: vec4<f32>,
    @location(8) style: vec4<f32>,
    @location(9) gains: vec4<f32>,
    @location(10) emission: vec4<f32>,
    @location(11) graph_tint_metallic: vec4<f32>,
    @location(12) graph_emission_preview: vec4<f32>,
    @location(13) vegetation: vec4<f32>,
}

struct ModelOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) @interpolate(flat) controls: vec4<f32>,
    @location(5) @interpolate(flat) emission: vec4<f32>,
    @location(6) @interpolate(flat) graph_tint_metallic: vec4<f32>,
    @location(7) @interpolate(flat) graph_emission_preview: vec4<f32>,
    @location(8) @interpolate(flat) receives_weather: f32,
    @location(9) @interpolate(flat) blend_mode: f32,
}

fn model_world(input: ModelInput) -> vec4<f32> {
    let model = mat4x4<f32>(input.model_x, input.model_y, input.model_z, input.model_t);
    let world = model * vec4<f32>(input.position, 1.0);
    let height = clamp((input.position.y - input.vegetation.z) * input.vegetation.w, 0.0, 1.0);
    var lean = input.vegetation.xy * (height * height);
    if dot(lean, lean) > 0.0000001 {
        let flutter = sin(dot(world.xyz, vec3<f32>(5.1, 3.8, -4.2)) + camera.natural.precipitation.w * 8.5);
        let tip = smoothstep(0.35, 1.0, height);
        lean += vec2<f32>(-lean.y, lean.x) * (flutter * tip * 0.17);
    }
    return world + vec4<f32>(lean.x, 0.0, lean.y, 0.0);
}

fn groom_world(input: ModelInput, world: vec4<f32>) -> vec4<f32> {
    if model_material.illustration.y < 0.5 {
        return world;
    }
    let tip = clamp(input.uv.y, 0.0, 1.0);
    let bend = tip * tip;
    let time = camera.natural.precipitation.w;
    let wind = clamp(camera.natural.sky_drift.zw, vec2<f32>(-9.0), vec2<f32>(9.0));
    let phase = dot(world.xyz, vec3<f32>(11.2, -13.5, 9.7)) - time * 6.7;
    let sway = wind * (0.00085 + 0.00035 * sin(phase))
        + vec2<f32>(0.0011 * sin(phase * 1.14), 0.00082 * cos(phase * 1.38));
    let wet = camera.natural.surface_weather.x;
    let delta = vec3<f32>(sway.x, 0.0019 * sin(phase + 1.6) - wet * 0.0034, sway.y);
    return world + vec4<f32>(bend * delta * length(input.model_x.xyz), 0.0);
}

@vertex
fn vs_model(input: ModelInput) -> ModelOutput {
    let world = groom_world(input, model_world(input));
    let x = input.model_x.xyz;
    let y = input.model_y.xyz;
    let z = input.model_z.xyz;
    let normal_matrix = mat3x3<f32>(x / dot(x, x), y / dot(y, y), z / dot(z, z));
    let tangent_matrix = mat3x3<f32>(x, y, z);
    var output: ModelOutput;
    output.position = camera.view_projection * world;
    output.world_position = world.xyz;
    let height = clamp((input.position.y - input.vegetation.z) * input.vegetation.w, 0.0, 1.0);
    let shear = input.vegetation.xy * (2.0 * height * input.vegetation.w);
    let slope_normal = vec3<f32>(input.normal.x, input.normal.y - dot(shear, (normal_matrix * input.normal).xz), input.normal.z);
    output.normal = normal_matrix * slope_normal;
    output.uv = input.uv * input.style.yz;
    output.tangent = vec4<f32>(tangent_matrix * input.tangent.xyz, input.tangent.w);
    output.controls = vec4<f32>(input.style.xw, input.gains.xy);
    output.emission = input.emission;
    output.graph_tint_metallic = input.graph_tint_metallic;
    output.graph_emission_preview = input.graph_emission_preview;
    output.receives_weather = input.gains.z;
    output.blend_mode = input.gains.w;
    return output;
}

@vertex
fn vs_model_shadow(input: ModelInput) -> @builtin(position) vec4<f32> {
    return camera.light_view_projection * model_world(input);
}

struct ModelShadowOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_model_shadow_masked(input: ModelInput) -> ModelShadowOutput {
    var output: ModelShadowOutput;
    output.position = camera.light_view_projection * groom_world(input, model_world(input));
    output.uv = input.uv * input.style.yz;
    return output;
}

@fragment
fn fs_model_shadow_masked(input: ModelShadowOutput) {
    let alpha = textureSample(base_color_texture, model_sampler, input.uv).a * model_material.base_color.a;
    if alpha < model_material.emission_alpha.w {
        discard;
    }
}

fn fresnel_schlick(dot_value: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - clamp(dot_value, 0.0, 1.0), 5.0);
}

struct ModelLightLobe {
    fresnel: vec3<f32>,
    specular: vec3<f32>,
}

fn model_light_lobe(
    normal: vec3<f32>, view_dir: vec3<f32>, light_dir: vec3<f32>,
    normal_light: f32, f0: vec3<f32>, roughness: f32,
) -> ModelLightLobe {
    let halfway_delta = view_dir + light_dir;
    let halfway = halfway_delta / max(length(halfway_delta), 0.0001);
    let normal_view = max(dot(normal, view_dir), 0.001);
    let normal_halfway = max(dot(normal, halfway), 0.0);
    let view_halfway = max(dot(view_dir, halfway), 0.0);
    let a2 = pow(roughness, 4.0);
    let d = 1.0 + normal_halfway * normal_halfway * (a2 - 1.0);
    let distribution = a2 / max(3.14159 * d * d, 0.00001);
    let k = (roughness + 1.0) * (roughness + 1.0) * 0.125;
    let visibility = 0.25 / ((normal_view * (1.0 - k) + k) * (normal_light * (1.0 - k) + k));
    let fresnel = fresnel_schlick(view_halfway, f0);
    return ModelLightLobe(fresnel, fresnel * distribution * visibility);
}

fn model_direct_light(
    normal: vec3<f32>, view_dir: vec3<f32>, light_dir: vec3<f32>,
    radiance: vec3<f32>, base: vec3<f32>, metallic: f32, roughness: f32,
) -> vec3<f32> {
    let normal_light = max(dot(normal, light_dir), 0.0);
    let f0 = mix(vec3<f32>(0.04), base, metallic);
    let lobe = model_light_lobe(normal, view_dir, light_dir, normal_light, f0, roughness);
    let diffuse = (vec3<f32>(1.0) - lobe.fresnel) * base * (1.0 - metallic) * (1.0 / 3.14159);
    return (diffuse + lobe.specular) * radiance * normal_light;
}

fn model_local_light(
    world: vec3<f32>, normal: vec3<f32>, geometry_normal: vec3<f32>, view_dir: vec3<f32>,
    room: u32, base: vec3<f32>, metallic: f32, f0: vec3<f32>, roughness: f32,
) -> vec3<f32> {
    var diffuse = vec3<f32>(0.0);
    var specular = vec3<f32>(0.0);
    for (var index = 0u; index < 6u; index += 1u) {
        let sample = sample_local_lamp(camera.interior_lights[index], world, room);
        let facing = max(dot(normal, sample.direction), 0.0);
        diffuse += sample.radiance * (0.25 + 0.75 * facing);
        if facing > 0.0 && dot(geometry_normal, sample.direction) > 0.0 && any(sample.radiance > vec3<f32>(0.0)) {
            // Limit near-mirror lobes to avoid unstable subpixel highlights.
            let lobe = model_light_lobe(normal, view_dir, sample.direction, facing, f0, max(roughness, 0.15));
            specular += lobe.specular * sample.radiance * facing;
        }
    }
    let room_gain = select(1.0, camera.atmosphere.z, room != 0u);
    return (min(diffuse, vec3<f32>(1.6)) * base * (1.0 - metallic * 0.60)
        + min(specular, vec3<f32>(2.5))) * room_gain;
}

fn material_blend_output(color: vec3<f32>, alpha: f32, mode: f32) -> vec4<f32> {
    let lit = output_color(color).rgb;
    // Multiply и Screen работают с цветом покрытия, умноженным на его альфа-канал.
    if mode >= 2.0 {
        return vec4<f32>(clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0)) * alpha, alpha);
    }
    return vec4<f32>(lit, alpha);
}

@fragment
fn fs_model(input: ModelOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let rgba = textureSample(base_color_texture, model_sampler, input.uv) * model_material.base_color;
    var base = rgba.rgb * input.graph_tint_metallic.rgb;
    let surface = textureSample(surface_texture, model_sampler, input.uv).rgb;
    let map = textureSample(normal_texture, model_sampler, input.uv).rgb * 2.0 - 1.0;
    let emissive_map = textureSample(emissive_texture, model_sampler, input.uv).rgb;
    let blend = model_material.illustration.z > 0.5 || input.blend_mode > 0.5;
    let alpha = select(1.0, rgba.a, blend);
    if rgba.a < model_material.emission_alpha.w || alpha <= 0.0 {
        discard;
    }
    let source_metallic = model_material.surface.x * surface.b;
    var metallic = clamp(select(source_metallic, input.graph_tint_metallic.w, input.graph_tint_metallic.w >= 0.0), 0.0, 1.0);
    let original_roughness = model_material.surface.y * surface.g;
    var roughness = clamp(select(original_roughness, input.controls.y, input.controls.y >= 0.0), 0.085, 1.0);
    let room = room_at(input.world_position);
    let guard_fur = model_material.illustration.y > 0.5;
    let geometry_normal = normalize(select(-input.normal, input.normal, front || guard_fur));
    if input.receives_weather > 0.5 && input.graph_emission_preview.w < 0.5 {
        let wet = weather_wetness(input.world_position, geometry_normal, room);
        let snow = snow_cover(input.world_position, geometry_normal, room);
        base *= 1.0 - wet * 0.32;
        if snow > 0.0 {
            base = mix(base, snow_color(input.world_position), snow);
        }
        roughness = mix(mix(roughness, min(roughness, 0.26), wet * 0.72), 0.89, snow);
        metallic *= 1.0 - snow;
    }
    if guard_fur {
        let fiber_depth = smoothstep(0.0, 0.77, input.uv.y);
        let fur_wet = camera.natural.surface_weather.x * select(0.0, 1.0, room == 0u);
        base *= (0.58 + fiber_depth * 0.44) * (1.0 - fur_wet * 0.23);
        roughness = mix(0.84, 0.49, fiber_depth * 0.45 + fur_wet * 0.30);
    }
    var normal = geometry_normal;
    if model_material.surface.w > 0.5 {
        let tangent = normalize(input.tangent.xyz - dot(input.tangent.xyz, normal) * normal);
        let bitangent = cross(normal, tangent) * input.tangent.w;
        let strength = model_material.surface.z * input.controls.z;
        normal = normalize(tangent * map.x * strength + bitangent * map.y * strength + normal * max(0.0001, map.z));
    }
    let eye_offset = camera.eye.xyz - input.world_position;
    let eye_distance = length(eye_offset);
    let view_dir = eye_offset / max(eye_distance, 0.001);
    let f0 = mix(vec3<f32>(0.04), base, metallic);
    let ambient_fresnel = fresnel_schlick(max(dot(normal, view_dir), 0.0), f0);
    let preview = input.graph_emission_preview.w;
    let reflected_ray = reflect(-view_dir, normal);
    var ambient = ambient_day(normal, room);
    var reflected = sky_color(reflected_ray);
    var reflection_weight = select(0.66, 0.12, room != 0u);
    var key = camera.natural.sun_direction_strength.xyz;
    var key_radiance = primary_sun(vec3<f32>(3.45, 3.15, 2.71));
    let fill = normalize(vec3<f32>(0.64, 0.22, -0.55));
    var fill_radiance = vec3<f32>(0.47, 0.65, 0.81) * daylight_ambient();
    if preview > 0.5 {
        let indoor = preview > 1.5;
        let up = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
        ambient = mix(vec3<f32>(0.24, 0.29, 0.35), vec3<f32>(0.48, 0.55, 0.62), up);
        reflected = mix(vec3<f32>(0.25, 0.29, 0.34), vec3<f32>(0.50, 0.66, 0.89), smoothstep(-0.1, 0.7, reflected_ray.y));
        key = normalize(vec3<f32>(-0.45, 0.78, 0.43));
        key_radiance = vec3<f32>(3.6, 3.37, 2.92);
        fill_radiance = vec3<f32>(0.57, 0.72, 0.88);
        reflection_weight = 0.72;
        if indoor {
            ambient = mix(vec3<f32>(0.145, 0.104, 0.077), vec3<f32>(0.28, 0.202, 0.14), up);
            reflected = mix(vec3<f32>(0.105, 0.086, 0.075), vec3<f32>(0.82, 0.56, 0.31), smoothstep(0.25, 0.88, reflected_ray.y));
            key = normalize(vec3<f32>(0.34, 0.89, 0.29));
            key_radiance = vec3<f32>(2.7, 1.52, 0.73);
            fill_radiance = vec3<f32>(0.37, 0.24, 0.15);
            reflection_weight = 0.58;
        }
    } else if dot(normal, key) > 0.0 {
        key_radiance *= directional_shadow(input.world_position, geometry_normal, room);
    }
    var color = ambient * base * (1.0 - metallic * 0.77);
    if preview < 0.5 {
        color += model_local_light(input.world_position, normal, geometry_normal, view_dir, room, base, metallic, f0, roughness);
        color += effect_light(input.world_position) * (base * 0.62 + vec3<f32>(0.055));
        if room == 0u && camera.natural.surface_weather.x > 0.02 && geometry_normal.y > 0.2 && roughness < 0.64 {
            let film = camera.natural.surface_weather.x * (1.0 - smoothstep(0.19, 0.64, roughness));
            let grazing = 0.08 + 0.32 * pow(1.0 - clamp(dot(normal, view_dir), 0.0, 1.0), 3.0);
            color += reflected_lamps(input.world_position, reflected_ray, roughness) * film * grazing;
        }
    }
    let reflection = mix(ambient, reflected, (1.0 - roughness) * reflection_weight);
    color += reflection * ambient_fresnel * mix(0.67, 0.24, roughness);
    let direct = model_direct_light(normal, view_dir, key, key_radiance, base, metallic, roughness);
    let fill_light = model_direct_light(normal, view_dir, fill, fill_radiance, base, metallic, roughness);
    let illustration = model_material.illustration.x;
    if illustration > 0.0 {
        let facing = dot(normal, key);
        let bands = 0.37 * smoothstep(-0.23, 0.13, facing) + 0.63 * smoothstep(0.38, 0.69, facing);
        let illustrated = base * (key_radiance * (bands * 0.28) + fill_radiance * 0.15);
        color += mix(direct + fill_light, illustrated, illustration);
    } else {
        color += direct;
        color += fill_light;
    }
    color *= contact_visibility(input.world_position, geometry_normal);
    let emission = select(model_material.emission_alpha.rgb, srgb_to_linear(input.emission.rgb), input.emission.w > 0.5);
    color += emissive_map * emission * input.controls.w + input.graph_emission_preview.rgb;
    let rim = pow(1.0 - clamp(dot(normal, view_dir), 0.0, 1.0), 3.4);
    if guard_fur {
        let scatter = smoothstep(-0.18, 0.69, dot(-view_dir, key));
        let tips = smoothstep(0.22, 0.94, input.uv.y);
        color += sqrt(max(base, vec3<f32>(0.0))) * (0.13 * ambient + 0.21 * key_radiance * scatter) * rim * tips;
    }
    color += srgb_to_linear(vec3<f32>(0.21, 0.81, 1.0)) * (0.07 + rim * 0.72) * input.controls.x;
    return material_blend_output(apply_fog(color, -view_dir, eye_distance), alpha, input.blend_mode);
}
