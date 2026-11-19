struct Uniforms {
    yuv_to_rgb: mat4x4<f32>,
    placement: vec4<f32>,
    uv_row_u: vec4<f32>,
    uv_row_v: vec4<f32>,
    texel: vec4<f32>,
    grade: vec4<f32>,
    detail: vec4<f32>,
    flags: vec4<u32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

const TRANSFER_PQ: u32 = 1u;
const TRANSFER_HLG: u32 = 2u;
const REFERENCE_WHITE_NITS: f32 = 203.0;
const TONE_MAP_EXPOSURE: f32 = 1.6;

@group(0) @binding(0) var<uniform> params: Uniforms;
@group(0) @binding(1) var plane_sampler: sampler;

@group(1) @binding(0) var current_y: texture_2d<f32>;
@group(1) @binding(1) var current_u: texture_2d<f32>;
@group(1) @binding(2) var current_v: texture_2d<f32>;

@group(2) @binding(0) var blend_y: texture_2d<f32>;
@group(2) @binding(1) var blend_u: texture_2d<f32>;
@group(2) @binding(2) var blend_v: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let ndc = (corner * 2.0 - 1.0) * params.placement.xy + params.placement.zw;
    let source = vec3<f32>(corner, 1.0);
    var out: VertexOutput;
    out.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = vec2<f32>(dot(params.uv_row_u.xyz, source), dot(params.uv_row_v.xyz, source));
    return out;
}

fn luma_at(plane: texture_2d<f32>, uv: vec2<f32>) -> f32 {
    return textureSampleLevel(plane, plane_sampler, uv, 0.0).r;
}

fn sample_yuv(
    plane_y: texture_2d<f32>,
    plane_u: texture_2d<f32>,
    plane_v: texture_2d<f32>,
    uv: vec2<f32>,
) -> vec3<f32> {
    let step = params.texel.xy;
    var luma = luma_at(plane_y, uv);
    if params.flags.z != 0u {
        let above = luma_at(plane_y, uv - vec2<f32>(0.0, step.y));
        let below = luma_at(plane_y, uv + vec2<f32>(0.0, step.y));
        luma = 0.5 * luma + 0.25 * (above + below);
    }
    let sharpen = params.detail.y;
    if sharpen > 0.0 {
        let blur = 0.25 * (
            luma_at(plane_y, uv - vec2<f32>(step.x, 0.0))
            + luma_at(plane_y, uv + vec2<f32>(step.x, 0.0))
            + luma_at(plane_y, uv - vec2<f32>(0.0, step.y))
            + luma_at(plane_y, uv + vec2<f32>(0.0, step.y))
        );
        luma = luma + (luma - blur) * sharpen;
    }
    var chroma = textureSampleLevel(plane_u, plane_sampler, uv, 0.0).rg;
    if params.flags.x != 0u {
        chroma.y = textureSampleLevel(plane_v, plane_sampler, uv, 0.0).r;
    }
    return vec3<f32>(luma, chroma) * params.detail.w;
}

fn pq_to_linear(encoded: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let power = pow(max(encoded, vec3<f32>(0.0)), vec3<f32>(1.0 / m2));
    let ratio = max(power - c1, vec3<f32>(0.0)) / (c2 - c3 * power);
    return pow(ratio, vec3<f32>(1.0 / m1)) * (10000.0 / REFERENCE_WHITE_NITS);
}

fn hlg_channel(value: f32) -> f32 {
    let a = 0.17883277;
    let b = 0.28466892;
    let c = 0.55991073;
    if value <= 0.5 {
        return value * value / 3.0;
    }
    return (exp((value - c) / a) + b) / 12.0;
}

fn hlg_to_linear(encoded: vec3<f32>) -> vec3<f32> {
    let scene = vec3<f32>(hlg_channel(encoded.r), hlg_channel(encoded.g), hlg_channel(encoded.b));
    let luminance = dot(scene, vec3<f32>(0.2627, 0.6780, 0.0593));
    let display = scene * pow(max(luminance, 1e-6), 0.2);
    return display * (1000.0 / REFERENCE_WHITE_NITS);
}

fn bt2020_to_bt709(color: vec3<f32>) -> vec3<f32> {
    let matrix = mat3x3<f32>(
        vec3<f32>(1.6605, -0.1246, -0.0182),
        vec3<f32>(-0.5876, 1.1329, -0.1006),
        vec3<f32>(-0.0728, -0.0083, 1.1187),
    );
    return matrix * color;
}

fn aces_rrt_odt(value: vec3<f32>) -> vec3<f32> {
    let numerator = value * (value + 0.0245786) - 0.000090537;
    let denominator = value * (0.983729 * value + 0.4329510) + 0.238081;
    return numerator / denominator;
}

fn aces_filmic(color: vec3<f32>) -> vec3<f32> {
    let input = mat3x3<f32>(
        vec3<f32>(0.59719, 0.07600, 0.02840),
        vec3<f32>(0.35458, 0.90834, 0.13383),
        vec3<f32>(0.04823, 0.01566, 0.83777),
    );
    let output = mat3x3<f32>(
        vec3<f32>(1.60475, -0.10208, -0.00327),
        vec3<f32>(-0.53108, 1.10813, -0.07276),
        vec3<f32>(-0.07367, -0.00605, 1.07602),
    );
    return clamp(output * aces_rrt_odt(input * color), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    let low = color * 12.92;
    let high = 1.055 * pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, color <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, color <= vec3<f32>(0.04045));
}

fn rotate_hue(color: vec3<f32>, angle: f32) -> vec3<f32> {
    let axis = vec3<f32>(0.57735027);
    let cosine = cos(angle);
    return color * cosine + cross(axis, color) * sin(angle) + axis * dot(axis, color) * (1.0 - cosine);
}

fn grade(color: vec3<f32>) -> vec3<f32> {
    var graded = (color - 0.5) * params.grade.y + 0.5 + params.grade.x;
    let luminance = dot(graded, vec3<f32>(0.2126, 0.7152, 0.0722));
    graded = mix(vec3<f32>(luminance), graded, params.grade.z);
    if params.detail.x != 0.0 {
        graded = rotate_hue(graded, params.detail.x);
    }
    graded = clamp(graded, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(graded, vec3<f32>(1.0 / params.grade.w));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var yuv = sample_yuv(current_y, current_u, current_v, in.uv);
    let blend = params.detail.z;
    if blend > 0.0 {
        yuv = mix(yuv, sample_yuv(blend_y, blend_u, blend_v, in.uv), blend);
    }
    var color = (params.yuv_to_rgb * vec4<f32>(yuv, 1.0)).rgb;

    let transfer = params.flags.y;
    if transfer == TRANSFER_PQ || transfer == TRANSFER_HLG {
        var scene = hlg_to_linear(color);
        if transfer == TRANSFER_PQ {
            scene = pq_to_linear(color);
        }
        scene = max(bt2020_to_bt709(scene), vec3<f32>(0.0)) * TONE_MAP_EXPOSURE;
        color = linear_to_srgb(aces_filmic(scene));
    }

    color = grade(color);
    if params.flags.w != 0u {
        color = srgb_to_linear(color);
    }
    return vec4<f32>(color, 1.0);
}
