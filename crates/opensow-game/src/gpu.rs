//! GPU presentation (wgpu): the CPU-drawn frame (the game's resolution, `0x00RRGGBB` pixels) is uploaded as a texture and
//! the GPU scales it into the window (aspect kept, black bars, sharp bilinear: see [`SHADER`]) and presents it in step with
//! the display's refresh. This replaces the CPU upscale + softbuffer copy, which at Retina window sizes cost as much as drawing the frame.
//! `OPENSOW_SOFTWARE=1`, or a machine without a usable adapter, keeps the softbuffer path.
//!
//! A mission frame is not drawn on the CPU at all: it is recorded (`opensow_viewer::record`) and replayed on the GPU
//! (`replay.rs`) straight into the frame texture ([`Gpu::present_recording`]).

use std::sync::Arc;
use winit::window::Window;

const SHADER: &str = r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var lin: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// one triangle covering the viewport; uv 0..1 over it
@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    o.uv = p;
    return o;
}

// Sharp bilinear: every game pixel is a solid block, as with nearest neighbour, and only the one window pixel where two
// blocks meet is blended. Plain nearest neighbour at a non-integer scale (1024x768 in 1920x1080 is 1.40625) shows every
// second or third row and column twice; those doubled lines stay put while the map moves under them and read as a grid.
// At whole-number scales (NATIVE, 2x, 3x) every window pixel samples a texel centre, so the output equals nearest neighbour.
@fragment
fn fs(o: Out) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(frame));
    let texel = o.uv * size;
    // window pixels per game pixel, per axis (the uv is linear over the viewport)
    let scale = 1.0 / max(vec2<f32>(abs(dpdx(texel.x)), abs(dpdy(texel.y))), vec2<f32>(1e-6));
    let flat = max(vec2<f32>(0.5) - 0.5 / scale, vec2<f32>(0.0));
    let d = fract(texel) - 0.5;
    let f = (d - clamp(d, -flat, flat)) * scale + 0.5;
    // the texture is BGRA from 0x00RRGGBB little-endian words; the unused top byte is 0, so alpha is forced to 1
    return vec4<f32>(textureSampleLevel(frame, lin, (floor(texel) + f) / size, 0.0).rgb, 1.0);
}
"#;

/// The window's GPU presenter: scales the frame into the window and presents it with vsync.
pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The frame texture, its size and its bind group (remade when the game's resolution changes).
    frame: Option<(wgpu::Texture, (u32, u32), wgpu::BindGroup)>,
    replay: crate::replay::Replay,
}

/// The adapter's own limits (the sprite atlas and the 5K picture are larger than the defaults allow).
fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor { label: Some("opensow"), required_limits: adapter.limits(), ..Default::default() }
}

/// A device without a window, for checking the replay against the CPU.
pub fn headless() -> Option<(wgpu::Device, wgpu::Queue, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    let name = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&device_descriptor(&adapter))).ok()?;
    Some((device, queue, name))
}

/// The pipeline that draws the frame texture into a `format` target, its bind group layout and its sampler.
fn present_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("frame"),
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
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("present"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("present"),
        layout: Some(&pl),
        vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            targets: &[Some(format.into())],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    // linear: the shader picks the sample points so that only block edges blend
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("sharp bilinear"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    (pipeline, layout, sampler)
}

impl Gpu {
    /// A presenter for `window`, or `None` (with the reason on stderr) when no adapter or surface can be had.
    pub fn new(window: Arc<Window>) -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let size = window.inner_size();
        let surface = instance.create_surface(window).map_err(|e| eprintln!("gpu: no surface ({e}); software presentation")).ok()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| eprintln!("gpu: no adapter ({e}); software presentation"))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&device_descriptor(&adapter)))
            .map_err(|e| eprintln!("gpu: no device ({e}); software presentation"))
            .ok()?;
        let caps = surface.get_capabilities(&adapter);
        // a non-sRGB format: the frame's bytes are already display colours and must reach the screen unchanged
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let mut config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1))?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let (pipeline, layout, sampler) = present_pipeline(&device, format);
        eprintln!("gpu: presenting with {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
        let replay = crate::replay::Replay::new(&device);
        Some(Gpu { surface, device, queue, config, pipeline, layout, sampler, frame: None, replay })
    }

    /// The frame texture for a `size` picture (made again when the size changes).
    fn frame_texture(&mut self, size: (u32, u32)) {
        if self.frame.as_ref().map(|f| f.1) == Some(size) {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.frame = Some((texture, size, group));
    }

    /// Replays the recorded frame `rec` on the GPU and presents it like [`Gpu::present`].
    pub fn present_recording(&mut self, rec: &opensow_viewer::record::Recording, ww: usize, wh: usize, rect: (usize, usize, usize, usize)) -> bool {
        self.resize(ww as u32, wh as u32);
        let size = (rec.w as u32, rec.h as u32);
        self.frame_texture(size);
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("replay") });
        {
            let Gpu { device, queue, replay, frame, .. } = self;
            let (picture, stride) = replay.encode(device, queue, &mut enc, rec);
            let texture = &frame.as_ref().unwrap().0;
            // the picture's words are 0x00RRGGBB (bit 24 = the screen mask, landing in the unused alpha byte): BGRA bytes
            enc.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: picture,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride as u32 * 4), rows_per_image: Some(size.1) },
                },
                wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            );
        }
        self.draw_frame(enc, rect)
    }

    fn resize(&mut self, w: u32, h: u32) {
        if (self.config.width, self.config.height) != (w, h) {
            (self.config.width, self.config.height) = (w, h);
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Uploads `frame` (`iw` x `ih`) and draws it into the `ww` x `wh` window inside `rect` (x, y, w, h). Returns false when
    /// no surface texture could be had this time (the frame is skipped; the next one tries again).
    pub fn present(&mut self, frame: &[u32], iw: usize, ih: usize, ww: usize, wh: usize, rect: (usize, usize, usize, usize)) -> bool {
        self.resize(ww as u32, wh as u32);
        let size = (iw as u32, ih as u32);
        self.frame_texture(size);
        let Some((texture, _, _)) = self.frame.as_ref() else { return false };
        // 0x00RRGGBB as little-endian bytes is B, G, R, 0: the texture's BGRA layout
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(frame.as_ptr() as *const u8, frame.len() * 4) };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: Some(size.1) },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("present") });
        self.draw_frame(enc, rect)
    }

    /// Draws the frame texture into the window inside `rect` after the work already in `enc`, and presents.
    fn draw_frame(&mut self, mut enc: wgpu::CommandEncoder, rect: (usize, usize, usize, usize)) -> bool {
        let Some((_, _, group)) = self.frame.as_ref() else { return false };
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.queue.submit([enc.finish()]);
                return false;
            }
            _ => {
                self.queue.submit([enc.finish()]);
                return false;
            }
        };
        let view = target.texture.create_view(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            let (x, y, w, h) = rect;
            pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(target);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hardware device for the offscreen tests, or `None`: CI machines have no adapter (Linux) or only a software one
    /// (Windows: WARP, where the test process crashed with an access violation), so the tests skip there as they do
    /// without a GPU.
    fn hardware_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return None;
        }
        pollster::block_on(adapter.request_device(&device_descriptor(&adapter))).ok()
    }

    /// Draws the `iw` x `ih` frame `src` (`0x00RRGGBB`) into a `ww` x `wh` target with the present pipeline; the target's words back.
    fn present_offscreen(device: &wgpu::Device, queue: &wgpu::Queue, src: &[u32], iw: u32, ih: u32, ww: u32, wh: u32) -> Vec<u32> {
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let (pipeline, layout, sampler) = present_pipeline(device, format);
        let extent = |w, h| wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let texture = |w, h, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: extent(w, h),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let frame = texture(iw, ih, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let bytes: Vec<u8> = src.iter().flat_map(|w| w.to_le_bytes()).collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &frame, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(iw * 4), rows_per_image: Some(ih) },
            extent(iw, ih),
        );
        let target = texture(ww, wh, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let view = frame.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let stride = (ww * 4).next_multiple_of(256);
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (stride * wh) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let tv = target.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &out,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(wh) },
            },
            extent(ww, wh),
        );
        queue.submit([enc.finish()]);
        out.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = out.slice(..).get_mapped_range().unwrap();
        (0..wh as usize)
            .flat_map(|y| (0..ww as usize).map(move |x| (y, x)))
            .map(|(y, x)| u32::from_le_bytes(data[y * stride as usize + x * 4..][..4].try_into().unwrap()) & 0xffffff)
            .collect()
    }

    #[test]
    fn whole_number_scales_equal_nearest_neighbour() {
        let Some((device, queue)) = hardware_device() else { return };
        let (iw, ih) = (7u32, 5u32);
        let src: Vec<u32> = (0..iw * ih).map(|i| i.wrapping_mul(0x9e3779b1) >> 8).collect();
        for k in [1u32, 2, 3] {
            let got = present_offscreen(&device, &queue, &src, iw, ih, iw * k, ih * k);
            let want: Vec<u32> = (0..ih * k).flat_map(|y| (0..iw * k).map(move |x| (y / k * iw + x / k) as usize)).map(|i| src[i]).collect();
            assert_eq!(got, want, "scale {k}");
        }
    }

    /// The bug the window showed: at 1.40625 (1024x768 in a 1920x1080 window) nearest neighbour gave every game pixel one or two
    /// window pixels, and the doubled lines read as a grid while the map moved. Now each one covers its true width.
    #[test]
    fn fractional_scales_give_every_game_pixel_the_same_width() {
        let Some((device, queue)) = hardware_device() else { return };
        let (iw, ih) = (16u32, 2u32);
        for ww in [24u32, 22, 37] {
            let scale = ww as f32 / iw as f32;
            for lit in 0..iw {
                let src: Vec<u32> = (0..iw * ih).map(|i| if i % iw == lit { 0xffffff } else { 0 }).collect();
                let got = present_offscreen(&device, &queue, &src, iw, ih, ww, ih);
                let width: f32 = got[..ww as usize].iter().map(|&p| (p & 255) as f32 / 255.0).sum();
                assert!((width - scale).abs() < 0.05, "window {ww}: game column {lit} covers {width} px, not {scale}");
            }
        }
    }
}
