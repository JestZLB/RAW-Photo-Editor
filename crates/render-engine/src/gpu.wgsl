struct Uniforms {
  crop: vec4<f32>,
  color_a: vec4<f32>,
  color_b: vec4<f32>,
  color_c: vec4<f32>,
  layer: vec4<f32>,
  sizes: vec4<f32>,
};

@group(0) @binding(0) var current_tex: texture_2d<f32>;
@group(0) @binding(1) var current_sampler: sampler;
@group(0) @binding(2) var layer_tex: texture_2d<f32>;
@group(0) @binding(3) var layer_sampler: sampler;
@group(0) @binding(4) var<uniform> u: Uniforms;

struct VertexOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
  let positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  let uvs = array<vec2<f32>, 3>(
    vec2<f32>(0.0, 1.0),
    vec2<f32>(2.0, 1.0),
    vec2<f32>(0.0, -1.0),
  );
  var out: VertexOut;
  out.position = vec4<f32>(positions[index], 0.0, 1.0);
  out.uv = uvs[index];
  return out;
}

fn srgb_to_linear(value: vec3<f32>) -> vec3<f32> {
  let low = value / 12.92;
  let high = pow((value + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
  return select(high, low, value <= vec3<f32>(0.04045));
}

fn linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
  let low = value * 12.92;
  let high = 1.055 * pow(value, vec3<f32>(1.0 / 2.4)) - 0.055;
  return select(high, low, value <= vec3<f32>(0.0031308));
}

fn smoothstep01(edge0: f32, edge1: f32, x: f32) -> f32 {
  let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

fn adjust_color(input: vec3<f32>) -> vec3<f32> {
  let exposure = u.color_a.x;
  let contrast = u.color_a.y;
  let highlights = u.color_a.z;
  let shadows = u.color_a.w;
  let whites = u.color_b.x;
  let blacks = u.color_b.y;
  let temperature = u.color_b.z;
  let tint = u.color_b.w;
  let saturation = u.color_c.x;
  let vibrance = u.color_c.y;

  let exposure_gain = exp2(exposure);
  let gains = vec3<f32>(
    (1.0 + 0.25 * temperature / 100.0) * (1.0 + 0.05 * tint / 100.0),
    1.0 - 0.15 * tint / 100.0,
    (1.0 - 0.25 * temperature / 100.0) * (1.0 + 0.05 * tint / 100.0),
  );
  var linear = input * gains * exposure_gain;

  var display = clamp(linear_to_srgb(linear), vec3<f32>(0.0), vec3<f32>(1.0));
  let contrast_factor = 1.0 + contrast / 100.0;
  display = (display - vec3<f32>(0.5)) * contrast_factor + vec3<f32>(0.5);

  let luma = dot(display, vec3<f32>(0.2126, 0.7152, 0.0722));
  let shadow_mask = 1.0 - smoothstep01(0.0, 0.65, luma);
  let highlight_mask = smoothstep01(0.35, 1.0, luma);
  let black_mask = 1.0 - smoothstep01(0.0, 0.4, luma);
  let white_mask = smoothstep01(0.6, 1.0, luma);
  let offset = shadows / 100.0 * 0.35 * shadow_mask
    + highlights / 100.0 * 0.35 * highlight_mask
    + blacks / 100.0 * 0.25 * black_mask
    + whites / 100.0 * 0.25 * white_mask;
  display = clamp(display + vec3<f32>(offset), vec3<f32>(0.0), vec3<f32>(1.0));
  linear = srgb_to_linear(display);

  let linear_luma = dot(linear, vec3<f32>(0.2126, 0.7152, 0.0722));
  let maximum = max(linear.r, max(linear.g, linear.b));
  let minimum = min(linear.r, min(linear.g, linear.b));
  let chroma = select((maximum - minimum) / maximum, 0.0, maximum <= 1e-6);
  let vibrance_gain = 1.0 + vibrance / 100.0 * (1.0 - clamp(chroma, 0.0, 1.0)) * 0.8;
  let factor = max((1.0 + saturation / 100.0) * vibrance_gain, 0.0);
  linear = linear_luma + (linear - vec3<f32>(linear_luma)) * factor;
  return clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn blend_channel(mode: i32, base: vec3<f32>, source: vec3<f32>) -> vec3<f32> {
  if (mode == 1) {
    return clamp(base, vec3<f32>(0.0), vec3<f32>(1.0)) * clamp(source, vec3<f32>(0.0), vec3<f32>(1.0));
  }
  if (mode == 2) {
    let b = clamp(base, vec3<f32>(0.0), vec3<f32>(1.0));
    let s = clamp(source, vec3<f32>(0.0), vec3<f32>(1.0));
    return b + s - b * s;
  }
  if (mode == 3) {
    let b = clamp(base, vec3<f32>(0.0), vec3<f32>(1.0));
    let s = clamp(source, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = 2.0 * b * s;
    let high = vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - b) * (vec3<f32>(1.0) - s);
    return select(high, low, b <= vec3<f32>(0.5));
  }
  if (mode == 4) {
    let b = clamp(base, vec3<f32>(0.0), vec3<f32>(1.0));
    let s = clamp(source, vec3<f32>(0.0), vec3<f32>(1.0));
    let dark = b - (vec3<f32>(1.0) - 2.0 * s) * b * (vec3<f32>(1.0) - b);
    let d = select(sqrt(b), ((16.0 * b - 12.0) * b + 4.0) * b, b <= vec3<f32>(0.25));
    let light = b + (2.0 * s - vec3<f32>(1.0)) * (d - b);
    return select(light, dark, s <= vec3<f32>(0.5));
  }
  return source;
}

fn composite(base: vec4<f32>, source: vec4<f32>, opacity: f32, mode: i32) -> vec4<f32> {
  let sa = clamp(source.a, 0.0, 1.0) * clamp(opacity, 0.0, 1.0);
  if (sa <= 0.0) {
    return base;
  }
  let da = clamp(base.a, 0.0, 1.0);
  let oa = sa + da * (1.0 - sa);
  if (oa <= 0.0) {
    return vec4<f32>(0.0);
  }
  let blended = select(source.rgb, blend_channel(mode, base.rgb, source.rgb), da > 1e-6);
  return vec4<f32>((blended * sa + base.rgb * da * (1.0 - sa)) / oa, oa);
}

fn cover_uv(uv: vec2<f32>) -> vec2<f32> {
  let output_aspect = u.sizes.z / max(u.sizes.w, 1.0);
  let image_aspect = u.sizes.x / max(u.sizes.y, 1.0);
  var mapped = uv;
  if (image_aspect > output_aspect) {
    let scale = output_aspect / image_aspect;
    mapped.x = (1.0 - scale) * 0.5 + uv.x * scale;
  } else {
    let scale = image_aspect / output_aspect;
    mapped.y = (1.0 - scale) * 0.5 + uv.y * scale;
  }
  return mapped;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
  let kind = i32(u.layer.y + 0.5);
  let mode = i32(u.layer.x + 0.5);
  if (kind == 0) {
    if (u.layer.z < 0.5) {
      return vec4<f32>(0.0);
    }
    let uv = u.crop.xy + input.uv * u.crop.zw;
    return textureSample(current_tex, current_sampler, uv);
  }

  let base_sample = textureSample(current_tex, current_sampler, input.uv);
  let base = vec4<f32>(srgb_to_linear(base_sample.rgb), base_sample.a);
  if (kind == 1) {
    let processed = vec4<f32>(adjust_color(base.rgb), base.a);
    let out = composite(base, processed, u.layer.w, mode);
    return vec4<f32>(linear_to_srgb(out.rgb), out.a);
  }

  let layer_sample = textureSample(layer_tex, layer_sampler, cover_uv(input.uv));
  let layer_color = vec4<f32>(srgb_to_linear(layer_sample.rgb), layer_sample.a);
  let out = composite(base, layer_color, u.layer.w, mode);
  return vec4<f32>(linear_to_srgb(out.rgb), out.a);
}
