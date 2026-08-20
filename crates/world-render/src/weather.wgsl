// What the weather does to a surface (plan 14.1): rain darkens and polishes it,
// drops ring on the water that has gathered, and snow lies on whatever faces up.
//
// One include, two materials: the terrain's splat extension and the object
// extension (`weather.rs`) both hand their `PbrInput` through `weather_pbr` before
// they light it, so a mod's building gets wet and snowy without knowing that
// weather exists.
//
// The time comes from the view's own globals rather than the uniform, so the
// uniform only has to be written when the weather actually changes.

#define_import_path world_render::weather

#import bevy_pbr::pbr_types::PbrInput

/// What the whole world's weather is, in two vectors. Both materials bind this
/// same layout (`WeatherParams` in Rust).
struct Weather {
    /// x = surface water 0…1, y = lying snow 0…1, z = rain rate [mm/h],
    /// w = how much of the sun the clouds take away, 0…1.
    state: vec4<f32>,
    /// xy = wind in render space [m/s], zw = reserved.
    wind: vec4<f32>,
}

/// Fresh snow — the same colour the seasonal ground textures are tinted with.
const SNOW_COLOR = vec3(0.86, 0.88, 0.93);

/// Fresnel reflectance of water at normal incidence is 0.02, which is
/// `sqrt(0.02 / 0.16)` in the reflectance parametrisation of `StandardMaterial`.
const WATER_REFLECTANCE = 0.354;

/// Cell size of the ripple grid [m]. One drop lands per cell and interval.
const RIPPLE_CELL = 0.35;

/// How long one ring takes to expand and fade [s].
const RIPPLE_LIFE = 0.75;

fn hash21(p: vec2<f32>) -> f32 {
    var h = fract(p.xyx * vec3(0.1031, 0.1030, 0.0973));
    h += dot(h, h.yzx + 33.33);
    return fract((h.x + h.y) * h.z);
}

/// Value noise in 0…1 — the ragged edge of a snow cover, and nothing finer.
fn noise21(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2(1.0, 0.0));
    let c = hash21(i + vec2(0.0, 1.0));
    let d = hash21(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

/// Slope of the water surface where drops are landing, as a gradient in the
/// ground plane. One drop per cell per interval, the ring expanding and dying —
/// so this is the splash and the puddle surface in one, and it costs no texture.
fn ripple_slope(xz: vec2<f32>, time: f32, rate: f32) -> vec2<f32> {
    if rate <= 0.0 {
        return vec2(0.0);
    }
    var slope = vec2(0.0);
    // Two grids at different scales, so the cells never read as a grid.
    for (var layer = 0u; layer < 2u; layer++) {
        let scale = 1.0 / (RIPPLE_CELL * (1.0 + f32(layer)));
        let p = xz * scale + f32(layer) * 17.3;
        let cell = floor(p);
        let local = fract(p) - 0.5;
        let seed = hash21(cell + f32(layer) * 71.0);
        // Heavier rain drops more often; drizzle leaves single rings.
        let interval = RIPPLE_LIFE * mix(4.0, 1.0, clamp(rate / 8.0, 0.0, 1.0));
        let age = fract((time + seed * interval) / interval) * interval;
        if age > RIPPLE_LIFE {
            continue;
        }
        let phase = age / RIPPLE_LIFE;
        let d = length(local) + 1e-4;
        // An annulus of radius `phase`, thinning and fading as it runs out. The
        // amplitude is small on purpose: a raindrop dents the water, it does not
        // blister it.
        let ring = exp(-64.0 * (d - phase * 0.45) * (d - phase * 0.45));
        let fade = (1.0 - phase) * (1.0 - phase);
        slope += (local / d) * ring * fade * 0.10;
    }
    return slope;
}

/// The dapple of the clouds on the ground, 1 = full sun … 0.5 = under a cloud.
///
/// Not a shadow map: the same threshold-on-noise the march draws the clouds with,
/// evaluated on the ground plane and drifting with the same wind. It is what makes
/// a landscape look like the sky above it is moving, and it costs two noise
/// lookups.
// ponytail: the pattern is not the shadow *of* the clouds overhead — no sun-angle
// offset, no parallax against the layer's height. Nobody can check the mapping;
// what would show is a shadow that stood still, and this one does not.
fn cloud_shade(xz: vec2<f32>, weather: Weather, time: f32) -> f32 {
    let cover = weather.state.w;
    if cover <= 0.001 {
        return 1.0;
    }
    let p = (xz + weather.wind.xy * time) / 1400.0;
    let field = noise21(p) * 0.65 + noise21(p * 2.7) * 0.35;
    let shadow = smoothstep(1.0 - cover - 0.2, 1.0 - cover + 0.25, field);
    // Damped: under a cloud the rest of the sky still lights the ground.
    return 1.0 - 0.5 * shadow;
}

/// The weather laid over a finished `PbrInput`, before it is lit.
///
/// `time` is the view's clock, `xz` and the normal come out of the input itself,
/// so a caller only has to pass what it was given.
fn weather_pbr(weather: Weather, time: f32, input: PbrInput) -> PbrInput {
    var out = input;
    let up = clamp(input.world_normal.y, 0.0, 1.0);
    let xz = input.world_position.xz;

    // --- Wet (Lagarde's model) -------------------------------------------------
    // Water runs off a wall and stands on a sleeper, so a vertical face is never
    // as wet as a horizontal one.
    let wet = weather.state.x * mix(0.3, 1.0, up);
    if wet > 0.001 {
        // A wet surface is darker (the film traps light in the microsurface),
        // smoother (it fills that microsurface in), and reflects like water.
        out.material.base_color = vec4(
            out.material.base_color.rgb * mix(1.0, 0.38, wet),
            out.material.base_color.a,
        );
        // A wet surface is smoother, but ballast and grass soak the water up —
        // only what is truly running wet, and lying flat, turns into a mirror.
        let polish = wet * wet * up;
        out.material.perceptual_roughness = mix(
            out.material.perceptual_roughness,
            0.10,
            polish,
        );
        out.material.reflectance = mix(out.material.reflectance, vec3(WATER_REFLECTANCE), wet);

        // Rings where the drops land, on water that is standing rather than running.
        let ripple = ripple_slope(xz, time, weather.state.z) * polish;
        out.N = normalize(out.N + vec3(ripple.x, 0.0, ripple.y));
    }

    // --- Snow ------------------------------------------------------------------
    // It does not stick to a steep face, and its edge is ragged rather than a
    // contour line — the noise is what keeps a hillside from looking painted.
    let slope = smoothstep(0.30, 0.75, up);
    let ragged = noise21(xz * 1.7) * 0.35;
    let cover = smoothstep(0.0, 0.6, slope * weather.state.y * (0.75 + ragged));
    if cover > 0.001 {
        // Snow is not one colour: it drifts, it is trodden, and it catches the
        // light in grains. Without this variation a field reads as white paper.
        let grain = noise21(xz * 9.0) * 0.5 + noise21(xz * 0.35) * 0.5;
        out.material.base_color = vec4(
            mix(out.material.base_color.rgb, SNOW_COLOR * (0.86 + 0.14 * grain), cover),
            out.material.base_color.a,
        );
        out.material.perceptual_roughness = mix(
            out.material.perceptual_roughness,
            0.45 + 0.35 * grain,
            cover,
        );
        out.material.metallic = out.material.metallic * (1.0 - cover);
        // Snow fills in what it lies on: the surface flattens towards the sky.
        out.N = normalize(mix(out.N, vec3(0.0, 1.0, 0.0), cover * 0.8));
    }

    let shade = cloud_shade(xz, weather, time);
    out.diffuse_occlusion *= shade;
    out.specular_occlusion *= shade;
    return out;
}
