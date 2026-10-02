#import bevy_ui::ui_vertex_output::UiVertexOutput

struct BrandUniform {
    paint: vec4<f32>,
    // mode (0 wordmark, 1 blob), palette (0 white to 3 mixed), opacity, dock recoil
    effect: vec4<f32>,
    // Independent openness followed by horizontal offsets in source-canvas pixels
    eyes: vec4<f32>,
    origins: vec4<f32>,
}

@group(1) @binding(0) var<uniform> brand: BrandUniform;
@group(1) @binding(1) var co1: texture_2d<f32>;
@group(1) @binding(2) var co1_sampler: sampler;
@group(1) @binding(3) var co2: texture_2d<f32>;
@group(1) @binding(4) var co2_sampler: sampler;
@group(1) @binding(5) var beat: texture_2d<f32>;
@group(1) @binding(6) var beat_sampler: sampler;
@group(1) @binding(7) var eyes_blue: texture_2d<f32>;
@group(1) @binding(8) var eyes_blue_sampler: sampler;
@group(1) @binding(9) var eyes_pink: texture_2d<f32>;
@group(1) @binding(10) var eyes_pink_sampler: sampler;

fn srgb(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), c > vec3(0.04045));
}

fn ellipse(p: vec2<f32>, center: vec2<f32>, radius: vec2<f32>) -> f32 {
    let d = length((p - center) / radius);
    return 1.0 - smoothstep(0.7, 1.0, d);
}

fn coverage(mask: texture_2d<f32>, filtering: sampler, uv: vec2<f32>) -> f32 {
    // Four pixel-footprint samples retain smooth contours as the title shrinks
    let offset = fwidth(uv) * 0.25;
    return 0.25 * (textureSample(mask, filtering, uv + offset).a
        + textureSample(mask, filtering, uv - offset).a
        + textureSample(mask, filtering, uv + offset * vec2(1.0, -1.0)).a
        + textureSample(mask, filtering, uv + offset * vec2(-1.0, 1.0)).a);
}

fn spread(p: vec2<f32>, origin: vec2<f32>, progress: f32, reach: f32) -> f32 {
    // Exact endpoint values keep the final material stable at every resolution
    if progress <= 0.0 { return 0.0; }
    if progress >= 1.0 { return 1.0; }
    let distance = length((p - origin) * vec2(4.666667, 1.0));
    let ripple = 0.025 * sin(p.x * 81.0 + p.y * 39.0) * sin(p.y * 57.0);
    return 1.0 - smoothstep(progress * reach - 0.10, progress * reach + 0.10, distance + ripple);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv;
    // Area-preserving shear after arrival; the UI rectangle keeps its final size
    uv.x -= 0.018 * brand.effect.w * (uv.y - 0.5);
    let blue = srgb(mix(vec3(0.01, 0.92, 0.97), vec3(0.015, 0.57, 1.0), uv.y));
    let pink = srgb(mix(vec3(1.0, 0.53, 0.73), vec3(0.98, 0.19, 0.56), uv.y));
    if brand.effect.x > 0.5 {
        let p = uv * 2.0 - 1.0;
        let d = length(p);
        let aa = max(fwidth(d), 0.008);
        let alpha = 1.0 - smoothstep(0.88 - aa, 0.88 + aa, d);
        let palette = brand.effect.y;
        let white_blue = mix(vec3(0.96), blue, clamp(palette, 0.0, 1.0));
        let two_tone = mix(blue, pink, smoothstep(0.35, 0.73, uv.y));
        var color = mix(white_blue, two_tone, clamp(palette - 1.0, 0.0, 1.0));
        let white_band = 1.0 - smoothstep(0.08, 0.28, abs(uv.x + uv.y * 0.4 - 0.72));
        color = mix(color, vec3(0.98), white_band * clamp(palette - 2.0, 0.0, 1.0));
        color *= 1.0 - 0.18 * smoothstep(0.3, 0.95, d);
        let shine = ellipse(uv, vec2(0.32, 0.24), vec2(0.12, 0.22));
        color = mix(color, vec3(1.0), shine * 0.8);
        return vec4(color, alpha * brand.effect.z);
    }

    let a = coverage(co1, co1_sampler, uv);
    let b = coverage(co2, co2_sampler, uv);
    let c = coverage(beat, beat_sampler, uv);
    let blue_eye_uv = vec2(uv.x - brand.eyes.z / 840.0,
        (uv.y - 0.59) / max(brand.eyes.x, 0.001) + 0.59);
    let pink_eye_uv = vec2(uv.x - brand.eyes.w / 840.0,
        (uv.y - 0.59) / max(brand.eyes.y, 0.001) + 0.59);
    let eyes = max(coverage(eyes_blue, eyes_blue_sampler, blue_eye_uv) * brand.eyes.x,
                   coverage(eyes_pink, eyes_pink_sampler, pink_eye_uv) * brand.eyes.y);
    let p1 = spread(uv, brand.origins.xy, brand.paint.x, 1.2);
    let p2 = spread(uv, brand.origins.zw, brand.paint.y, 1.2);
    let p3 = spread(uv, vec2(0.57, 0.16), brand.paint.z, 2.4);
    let highlights = ellipse(uv, vec2(0.050, 0.175), vec2(0.024, 0.061))
        + ellipse(uv, vec2(0.161, 0.387), vec2(0.019, 0.044))
        + ellipse(uv, vec2(0.322, 0.175), vec2(0.024, 0.061))
        + ellipse(uv, vec2(0.426, 0.387), vec2(0.019, 0.044));
    let co1_color = mix(vec3(0.96), mix(blue, vec3(1.0), highlights * 0.62), p1);
    let co2_color = mix(vec3(0.96), mix(pink, vec3(1.0), highlights * 0.62), p2);
    let lower = smoothstep(0.51, 0.98, uv.y) * 0.46;
    let tint = mix(srgb(vec3(0.10, 0.89, 0.98)), srgb(vec3(1.0, 0.38, 0.71)),
                   smoothstep(0.66, 0.94, uv.x));
    let beat_color = mix(vec3(0.96), mix(vec3(0.96), tint, lower), p3);
    let coverage = a + b + c + eyes;
    let rgb = (co1_color * a + co2_color * b + beat_color * c + vec3(0.98) * eyes)
        / max(coverage, 0.0001);
    return vec4(rgb, clamp(coverage, 0.0, 1.0) * brand.effect.z);
}
