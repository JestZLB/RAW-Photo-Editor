//! WGPU renderer for the GPU-compatible part of the layer graph.
//!
//! Supported today: base image, color layers without clarity/sharpness, AI
//! result layers, crop, opacity and blend modes. Documents with active
//! portrait layers or local-detail color adjustments fall back to the CPU
//! renderer instead of silently producing a different image.

use std::sync::{mpsc, OnceLock};

use bytemuck::{Pod, Zeroable};
use document::{BlendMode, LayerKind, ProjectDocument};
use image_core::DecodedImage;
use wgpu::util::DeviceExt;

use crate::{AssetResolver, RenderError};

const SHADER: &str = include_str!("gpu.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    crop: [f32; 4],
    color_a: [f32; 4],
    color_b: [f32; 4],
    color_c: [f32; 4],
    layer: [f32; 4],
    sizes: [f32; 4],
}

impl Uniforms {
    fn base(crop: [f32; 4], sizes: [f32; 4], base_visible: bool) -> Self {
        Self {
            crop,
            color_a: [0.0; 4],
            color_b: [0.0; 4],
            color_c: [0.0; 4],
            layer: [0.0, 0.0, if base_visible { 1.0 } else { 0.0 }, 0.0],
            sizes,
        }
    }
}

fn blend_index(mode: BlendMode) -> f32 {
    match mode {
        BlendMode::Normal => 0.0,
        BlendMode::Multiply => 1.0,
        BlendMode::Screen => 2.0,
        BlendMode::Overlay => 3.0,
        BlendMode::SoftLight => 4.0,
    }
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    _dummy_texture: wgpu::Texture,
    dummy_view: wgpu::TextureView,
    max_texture: u32,
}

impl GpuContext {
    fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("luma-gpu-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .ok()?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("luma-gpu-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("luma-gpu-bind-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("luma-gpu-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("luma-gpu-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("luma-gpu-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("luma-gpu-dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let dummy_view = dummy_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Some(Self {
            max_texture: device.limits().max_texture_dimension_2d.min(8192),
            device,
            queue,
            bind_group_layout,
            pipeline,
            sampler,
            _dummy_texture: dummy_texture,
            dummy_view,
        })
    }

    fn texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("luma-gpu-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    fn upload(&self, image: &DecodedImage) -> wgpu::Texture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("luma-gpu-input"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let row_bytes = image.width as usize * 4;
        let padded = row_bytes.div_ceil(256) * 256;
        let mut data = vec![0u8; padded * image.height as usize];
        for y in 0..image.height as usize {
            let src = &image.rgba8[y * row_bytes..(y + 1) * row_bytes];
            data[y * padded..y * padded + row_bytes].copy_from_slice(src);
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(image.height),
            },
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
        texture
    }
}

pub fn gpu_available() -> bool {
    context().is_some()
}

fn context() -> Option<&'static GpuContext> {
    static GPU: OnceLock<Option<GpuContext>> = OnceLock::new();
    GPU.get_or_init(GpuContext::new).as_ref()
}

fn effective_crop(
    document: &ProjectDocument,
    source: &DecodedImage,
) -> (document::CropRect, u32, u32) {
    let crop = document.crop.clamped(source.width, source.height);
    (crop, crop.width.max(1), crop.height.max(1))
}

/// Layers whose effects are not implemented by the GPU pipeline must fall back
/// to the CPU renderer. Blur is deliberately listed here: the layer loop in
/// `render_document_gpu` only handles `Color` / `AiResult` and skips every other
/// kind with `_ => continue`, so an unmasked blur layer used to be silently
/// dropped on machines with a GPU while CPU previews showed the blur.
fn layer_requires_cpu(layer: &document::Layer) -> bool {
    if layer.mask.as_ref().is_some_and(|mask| !mask.is_empty()) {
        return true;
    }
    if layer.kind == LayerKind::Portrait
        && layer
            .portrait
            .as_ref()
            .is_some_and(|params| params.has_active_effect())
    {
        return true;
    }
    if matches!(layer.kind, LayerKind::Retouch | LayerKind::Blur) {
        return true;
    }
    if layer.kind == LayerKind::Color {
        if let Some(color) = &layer.color {
            let sanitized = color.sanitized();
            return sanitized.clarity.abs() > 0.0001
                || sanitized.sharpness.abs() > 0.0001
                || sanitized.hue.abs() > 0.0001
                || sanitized.hsl_saturation.abs() > 0.0001
                || sanitized.hsl_lightness.abs() > 0.0001
                || sanitized.uses_tone_curve()
                || sanitized.uses_lut()
                || sanitized.noise_luma != 0.0
                || sanitized.noise_chroma != 0.0
                || sanitized.vignette != 0.0
                || sanitized.grain != 0.0
                || sanitized.dehaze != 0.0
                || sanitized.chromatic != 0.0;
        }
    }
    false
}

/// Returns `None` when this document should use the CPU renderer.
pub fn render_document_gpu(
    document: &ProjectDocument,
    source: &DecodedImage,
    max_dimension: Option<u32>,
    resolver: &dyn AssetResolver,
) -> Option<Result<DecodedImage, RenderError>> {
    let context = context()?;
    if source.width == 0 || source.height == 0 {
        return None;
    }
    if document
        .layers
        .iter()
        .filter(|layer| layer.visible && layer.opacity > 0.0)
        .any(layer_requires_cpu)
    {
        return None;
    }

    let (crop, crop_width, crop_height) = effective_crop(document, source);
    if crop.angle.abs() > 0.001 {
        // Straightening is implemented in the CPU reference renderer only.
        return None;
    }
    let scale = match max_dimension.filter(|max| *max > 0) {
        Some(max) => (max as f64 / crop_width.max(crop_height) as f64).min(1.0),
        None => 1.0,
    };
    let target_width = ((crop_width as f64 * scale).round() as u32).max(1);
    let target_height = ((crop_height as f64 * scale).round() as u32).max(1);
    if target_width > context.max_texture || target_height > context.max_texture {
        return None;
    }

    let proxy_scale =
        (target_width.max(target_height) as f64 / crop_width.max(crop_height) as f64).min(1.0);
    // Keep the source texture modest for tiny-crop previews; exports (no
    // max_dimension) still upload the full source.
    let proxy_cap = max_dimension
        .filter(|max| *max > 0)
        .map(|max| max.max(crate::MAX_PREVIEW_RENDER_LONG_EDGE))
        .unwrap_or(u32::MAX);
    let proxy_long = ((source.width.max(source.height) as f64 * proxy_scale).ceil() as u32)
        .clamp(1, source.width.max(source.height))
        .min(context.max_texture)
        .min(proxy_cap);
    let proxy = source.resize_to_fit(proxy_long).ok()?;
    let base_texture = context.upload(&proxy);
    let base_view = base_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let target_a = context.texture(target_width, target_height);
    let target_b = context.texture(target_width, target_height);
    let view_a = target_a.create_view(&wgpu::TextureViewDescriptor::default());
    let view_b = target_b.create_view(&wgpu::TextureViewDescriptor::default());

    let crop_uniform = [
        crop.x as f32 / source.width as f32,
        crop.y as f32 / source.height as f32,
        crop.width as f32 / source.width as f32,
        crop.height as f32 / source.height as f32,
    ];
    let base_sizes = [1.0, 1.0, target_width as f32, target_height as f32];
    let base_visible = document
        .layers
        .iter()
        .find(|layer| layer.kind == LayerKind::Base)
        .map(|layer| layer.visible)
        .unwrap_or(false);

    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("luma-gpu-encoder"),
        });
    let mut buffers: Vec<wgpu::Buffer> = Vec::new();
    let mut ai_textures: Vec<wgpu::Texture> = Vec::new();
    let mut read_index = 0usize;

    let base_uniforms = Uniforms::base(crop_uniform, base_sizes, base_visible);
    let base_buffer = context
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("luma-gpu-uniforms-base"),
            contents: bytemuck::bytes_of(&base_uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let base_bind_group = create_bind_group(context, &base_view, &context.dummy_view, &base_buffer);
    run_pass(context, &mut encoder, &base_bind_group, &view_a);
    buffers.push(base_buffer);

    for layer in &document.layers {
        if layer.kind == LayerKind::Base || !layer.visible || layer.opacity <= 0.0 {
            continue;
        }
        let current_view = if read_index == 0 { &view_a } else { &view_b };
        let output_view = if read_index == 0 { &view_b } else { &view_a };
        let (uniforms, layer_view) = match layer.kind {
            LayerKind::Color => {
                let Some(color) = &layer.color else {
                    continue;
                };
                let color = color.sanitized();
                (
                    Uniforms {
                        crop: [0.0; 4],
                        color_a: [
                            color.exposure,
                            color.contrast,
                            color.highlights,
                            color.shadows,
                        ],
                        color_b: [color.whites, color.blacks, color.temperature, color.tint],
                        color_c: [color.saturation, color.vibrance, 0.0, 0.0],
                        layer: [blend_index(layer.blend_mode), 1.0, 0.0, layer.opacity],
                        sizes: [1.0, 1.0, target_width as f32, target_height as f32],
                    },
                    None,
                )
            }
            LayerKind::AiResult => {
                let Some(asset_id) = layer.asset_id else {
                    return Some(Err(RenderError::MissingAssetId));
                };
                let decoded = match resolver.load_ai_result(asset_id) {
                    Ok(decoded) => decoded,
                    Err(error) => return Some(Err(error)),
                };
                let image = if decoded.width.max(decoded.height) > context.max_texture {
                    match decoded.resize_to_fit(context.max_texture) {
                        Ok(image) => image,
                        Err(_) => return None,
                    }
                } else {
                    decoded
                };
                let texture = context.upload(&image);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let uniforms = Uniforms {
                    crop: [0.0; 4],
                    color_a: [0.0; 4],
                    color_b: [0.0; 4],
                    color_c: [0.0; 4],
                    layer: [blend_index(layer.blend_mode), 2.0, 0.0, layer.opacity],
                    sizes: [
                        image.width as f32,
                        image.height as f32,
                        target_width as f32,
                        target_height as f32,
                    ],
                };
                ai_textures.push(texture);
                (uniforms, Some(view))
            }
            _ => continue,
        };
        let buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("luma-gpu-uniforms-layer"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let layer_view = layer_view.as_ref().unwrap_or(&context.dummy_view);
        let bind_group = create_bind_group(context, current_view, layer_view, &buffer);
        run_pass(context, &mut encoder, &bind_group, output_view);
        buffers.push(buffer);
        read_index = 1 - read_index;
    }

    let final_texture = if read_index == 0 {
        &target_a
    } else {
        &target_b
    };
    let bytes_per_row = (target_width * 4).div_ceil(256) * 256;
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("luma-gpu-readback"),
        size: (bytes_per_row * target_height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: final_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(target_height),
            },
        },
        wgpu::Extent3d {
            width: target_width,
            height: target_height,
            depth_or_array_layers: 1,
        },
    );
    context.queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    context.device.poll(wgpu::Maintain::Wait);
    match receiver.recv() {
        Ok(Ok(())) => {}
        _ => return None,
    }
    let mapped = slice.get_mapped_range();
    let mut rgba8 = Vec::with_capacity((target_width * target_height * 4) as usize);
    for y in 0..target_height as usize {
        let start = y * bytes_per_row as usize;
        let end = start + target_width as usize * 4;
        rgba8.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    readback.unmap();
    let image = match DecodedImage::new(target_width, target_height, rgba8) {
        Ok(image) => image,
        Err(_) => return None,
    };
    Some(Ok(image))
}

fn create_bind_group(
    context: &GpuContext,
    current: &wgpu::TextureView,
    layer: &wgpu::TextureView,
    uniforms: &wgpu::Buffer,
) -> wgpu::BindGroup {
    context
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("luma-gpu-bind-group"),
            layout: &context.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(current),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&context.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(layer),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&context.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        })
}

fn run_pass(
    context: &GpuContext,
    encoder: &mut wgpu::CommandEncoder,
    bind_group: &wgpu::BindGroup,
    output: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("luma-gpu-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&context.pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lut;
    use document::{ColorParams, CropRect, Layer, ProjectDocument, SourceAsset};
    use image_core::DecodedImage;
    use std::collections::HashMap;
    use uuid::Uuid;

    #[derive(Default)]
    struct EmptyResolver;

    impl AssetResolver for EmptyResolver {
        fn load_ai_result(&self, asset_id: Uuid) -> Result<DecodedImage, RenderError> {
            Err(RenderError::MissingAsset {
                asset_id: asset_id.to_string(),
            })
        }

        fn load_lut(&self, asset_id: Uuid) -> Result<Lut, RenderError> {
            Err(RenderError::LutLoad {
                asset_id: asset_id.to_string(),
                message: "no lut assets in this resolver".into(),
            })
        }
    }

    #[derive(Default)]
    struct StaticResolver {
        assets: HashMap<Uuid, DecodedImage>,
    }

    impl AssetResolver for StaticResolver {
        fn load_ai_result(&self, asset_id: Uuid) -> Result<DecodedImage, RenderError> {
            self.assets
                .get(&asset_id)
                .cloned()
                .ok_or(RenderError::MissingAsset {
                    asset_id: asset_id.to_string(),
                })
        }

        fn load_lut(&self, asset_id: Uuid) -> Result<Lut, RenderError> {
            Err(RenderError::LutLoad {
                asset_id: asset_id.to_string(),
                message: "no lut assets in this resolver".into(),
            })
        }
    }

    fn base_doc() -> ProjectDocument {
        let mut doc = ProjectDocument::new("gpu parity", 32, 32);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "source.png".into(),
            width: 32,
            height: 32,
            format: "png".into(),
            original_path: "source.png".into(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        doc.layers.push(Layer::base("原始照片", None));
        doc
    }

    fn max_channel_diff(a: &DecodedImage, b: &DecodedImage) -> i32 {
        a.rgba8
            .iter()
            .zip(b.rgba8.iter())
            .map(|(left, right)| (*left as i32 - *right as i32).abs())
            .max()
            .unwrap_or(0)
    }

    fn source() -> DecodedImage {
        let mut rgba = Vec::new();
        for y in 0..32u32 {
            for x in 0..32u32 {
                rgba.extend_from_slice(&[
                    (40 + x * 4) as u8,
                    (30 + y * 4) as u8,
                    (20 + x + y) as u8,
                    255,
                ]);
            }
        }
        DecodedImage::new(32, 32, rgba).unwrap()
    }

    #[test]
    fn gpu_color_pipeline_matches_cpu_within_tolerance() {
        if !gpu_available() {
            eprintln!("no WGPU adapter; skipping GPU parity test");
            return;
        }
        let mut doc = base_doc();
        let mut color = Layer::color();
        color.color = Some(ColorParams {
            exposure: 0.35,
            contrast: 18.0,
            highlights: -12.0,
            shadows: 16.0,
            temperature: 8.0,
            saturation: 10.0,
            vibrance: 6.0,
            ..Default::default()
        });
        doc.layers.push(color);
        let image = source();
        let resolver = EmptyResolver;

        let cpu = crate::render_document(&doc, &image, Some(32), &resolver)
            .unwrap()
            .to_srgb8();
        let gpu = render_document_gpu(&doc, &image, Some(32), &resolver)
            .expect("expected GPU path")
            .expect("GPU render failed");
        assert_eq!((gpu.width, gpu.height), (cpu.width, cpu.height));
        let max_diff = max_channel_diff(&cpu, &gpu);
        assert!(max_diff <= 6, "CPU/GPU max channel diff: {max_diff}");
    }

    #[test]
    fn gpu_ai_layer_and_crop_match_cpu() {
        if !gpu_available() {
            eprintln!("no WGPU adapter; skipping GPU parity test");
            return;
        }
        let mut doc = base_doc();
        doc.set_crop(CropRect {
            x: 8,
            y: 4,
            width: 16,
            height: 20,
            angle: 0.0,
        });
        let asset_id = Uuid::new_v4();
        let mut rgba = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                rgba.extend_from_slice(&[
                    (30 + x * 8) as u8,
                    (20 + y * 10) as u8,
                    (60 + (x + y) * 4) as u8,
                    220,
                ]);
            }
        }
        let mut resolver = StaticResolver::default();
        resolver
            .assets
            .insert(asset_id, DecodedImage::new(16, 16, rgba).unwrap());
        let mut ai = Layer::ai_result(asset_id);
        ai.opacity = 0.7;
        ai.blend_mode = document::BlendMode::Multiply;
        doc.layers.push(ai);
        let image = source();

        let cpu = crate::render_document(&doc, &image, Some(16), &resolver)
            .unwrap()
            .to_srgb8();
        let gpu = render_document_gpu(&doc, &image, Some(16), &resolver)
            .expect("expected GPU path")
            .expect("GPU render failed");
        assert_eq!((gpu.width, gpu.height), (cpu.width, cpu.height));
        let max_diff = max_channel_diff(&cpu, &gpu);
        assert!(max_diff <= 12, "CPU/GPU max channel diff: {max_diff}");
    }

    #[test]
    fn gpu_falls_back_for_retouch_layers() {
        let mut doc = base_doc();
        doc.layers.push(Layer::retouch());
        let resolver = EmptyResolver;
        assert!(render_document_gpu(&doc, &source(), Some(16), &resolver).is_none());
    }

    #[test]
    fn gpu_falls_back_for_active_portrait_layers() {
        let mut doc = base_doc();
        let mut portrait = Layer::portrait();
        portrait.portrait = Some(document::PortraitParams {
            smooth: 40.0,
            ..Default::default()
        });
        doc.layers.push(portrait);
        let resolver = EmptyResolver;
        assert!(render_document_gpu(&doc, &source(), Some(16), &resolver).is_none());
    }

    #[test]
    fn blur_layer_forces_cpu_fallback_even_without_a_mask() {
        // Regression: the GPU layer loop ignores unknown kinds with
        // `_ => continue`. An unmasked blur layer used to slip past every
        // fallback check and produce a preview with no blur at all.
        assert!(
            layer_requires_cpu(&Layer::blur()),
            "blur must never be rendered by the GPU path"
        );
        // Guard against over-broad fallback: a plain colour layer is still
        // GPU-safe, which is the whole point of this path.
        let plain = Layer::color();
        assert!(!layer_requires_cpu(&plain));
    }
}
