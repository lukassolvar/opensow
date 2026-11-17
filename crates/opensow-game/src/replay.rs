//! The GPU replay of a recorded frame (`opensow_viewer::record`): one compute thread per screen pixel walks the commands of its
//! tile in order and applies them with the software loops' integer arithmetic, so the picture is the same as the CPU's. The
//! sprite atlas and the map picture stay on the GPU; per frame only the commands, their sample maps and the tile lists are
//! uploaded. A lens command reads the picture as it was before it, so the commands are replayed in segments (one dispatch
//! each) and the picture is copied aside before every segment that starts with a lens.

use opensow_viewer::record::{self, Recording, TILE};

const SHADER: &str = r#"
struct U {
    w: u32, h: u32, stride: u32, tiles_x: u32,
    tiles: u32, seg: u32, cmds: u32, maps: u32,
    offsets: u32, indices: u32, local: u32, pad: u32,
};
@group(0) @binding(0) var<storage, read_write> state: array<u32>;
@group(0) @binding(1) var<storage, read> prev: array<u32>;
@group(0) @binding(2) var<storage, read> data: array<u32>;
@group(0) @binding(3) var<storage, read> atlas: array<u32>;
@group(0) @binding(4) var<storage, read> ground: array<u32>;
@group(0) @binding(5) var<uniform> u: U;

const MASK: u32 = 0x1000000u;

fn ch(q: u32, sh: u32) -> u32 { return (q >> sh) & 255u; }
fn pack(r: u32, g: u32, b: u32) -> u32 { return (r << 16u) | (g << 8u) | b; }
fn mixc(d: u32, s: u32, a: u32) -> u32 { return (d * (255u - a) + s * a) / 255u; }
fn blend(d: u32, s: u32, a: u32) -> u32 {
    return pack(mixc(ch(d, 16u), ch(s, 16u), a), mixc(ch(d, 8u), ch(s, 8u), a), mixc(ch(d, 0u), ch(s, 0u), a));
}
fn to565(q: u32) -> u32 { return (((q >> 19u) & 31u) << 11u) | (((q >> 10u) & 63u) << 5u) | ((q >> 3u) & 31u); }
fn from565(o: u32) -> u32 {
    let r = (o >> 11u) & 31u;
    let g = (o >> 5u) & 63u;
    let b = o & 31u;
    return pack((r << 3u) | (r >> 2u), (g << 2u) | (g >> 4u), (b << 3u) | (b >> 2u));
}
// effects::Blit::apply565
fn apply(mode: u32, d: u32, s: u32) -> u32 {
    let dr = i32((d >> 11u) & 31u); let dg = i32((d >> 5u) & 63u); let db = i32(d & 31u);
    let sr = i32((s >> 11u) & 31u); let sg = i32((s >> 5u) & 63u); let sb = i32(s & 31u);
    var r = sr; var g = sg; var b = sb;
    switch mode {
        case 1u: { r = dr + sr; g = dg + sg; b = db + sb; }
        case 2u: { r = dr + (sr >> 1u); g = dg + (sg >> 1u); b = db + (sb >> 1u); }
        case 3u: { r = (dr + sr) >> 1u; g = (dg + sg) >> 1u; b = (db + sb) >> 1u; }
        case 4u: { r = dr - (sr >> 2u); g = dg - (sg >> 2u); b = db - (sb >> 2u); }
        case 5u: { r = dr - (sr >> 1u); g = dg - (sg >> 1u); b = db - (sb >> 1u); }
        case 6u: { r = dr - 6; g = dg - 12; b = db - 6; }
        case 7u: { r = dr + 1; g = dg + 2; b = db + 1; }
        default: {}
    }
    return (u32(clamp(r, 0, 31)) << 11u) | (u32(clamp(g, 0, 63)) << 5u) | u32(clamp(b, 0, 31));
}
fn rgb8(px: u32) -> u32 { return from565(px); }

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    if (gid.x >= u.w || gid.y >= u.h) { return; }
    let t = u.offsets + u.seg * u.tiles + wid.y * u.tiles_x + wid.x;
    let first = data[t];
    let last = data[t + 1u];
    let si = gid.y * u.stride + gid.x;
    var q = state[si];
    let x = i32(gid.x);
    let y = i32(gid.y);
    for (var k = first; k < last; k++) {
        let c = u.cmds + data[u.indices + k] * 16u;
        let opc = data[c];
        let xs = bitcast<i32>(data[c + 1u]);
        let ys = bitcast<i32>(data[c + 2u]);
        let xe = bitcast<i32>(data[c + 3u]);
        let ye = bitcast<i32>(data[c + 4u]);
        if (x < xs || x >= xe || y < ys || y >= ye) { continue; }
        let p0 = data[c + 9u];
        let p1 = data[c + 10u];
        let keep = q & MASK;
        if (opc == 0u) {
            if (data[c + 11u] == 0u || (gid.y & 1u) == 0u) { q = keep | blend(q, p0, p1); }
            continue;
        }
        if (opc == 17u) {
            q = keep | pack(min(ch(q, 16u) + ch(p0, 16u), 255u), min(ch(q, 8u) + ch(p0, 8u), 255u), min(ch(q, 0u) + ch(p0, 0u), 255u));
            continue;
        }
        if (opc == 18u) {
            if ((gid.y & 1u) == 1u) {
                q = keep;
            } else {
                let p = to565(q);
                q = keep | from565((p & 0xf800u) | ((((p >> 1u) & 0x380u) | (p & 0x1cu)) >> 2u));
            }
            continue;
        }
        if (opc == 1u) {
            q = keep | pack(ch(q, 16u) - min(ch(q, 16u), p0), ch(q, 8u) - min(ch(q, 8u), p0), ch(q, 0u) - min(ch(q, 0u), p0));
            continue;
        }
        let col = bitcast<i32>(data[u.maps + data[c + 5u] + u32(x - xs)]);
        let row = bitcast<i32>(data[u.maps + data[c + 6u] + u32(y - ys)]);
        let base = data[c + 7u];
        let stride = data[c + 8u];
        if (opc == 2u || opc == 3u) {
            if (col < 0 || row < 0) { continue; }
            let gw = ground[u32(row) * stride + u32(col)];
            if (opc == 3u) {
                q = q | (gw & MASK);
            } else if (p0 == 0u || (gw & MASK) != 0u) {
                q = keep | (gw & 0xffffffu);
            }
            continue;
        }
        if (opc == 16u) {
            if (col < 0 || row < 0) { continue; }
            let pi = base + u32(row) * stride + u32(col);
            var v = 0u;
            if (p0 == 0u) { v = atlas[pi]; } else { v = data[u.local + pi]; }
            if (p1 != 0u) { v = (v >> 1u) & 0x7f7f7fu; } else { v = v & 0xffffffu; }
            q = keep | v;
            continue;
        }
        let sidx = base + u32(row) * stride + u32(col);
        if (opc == 15u) {
            let a = data[u.local + sidx];
            if (a > 0u && !(p1 != 0u && keep != 0u)) { q = keep | blend(q, p0, a); }
            continue;
        }
        let s = atlas[sidx];
        if ((s & 0x10000u) == 0u) { continue; }
        let px = s & 0xffffu;
        let masked = p1 != 0u && keep != 0u;
        switch opc {
            case 4u: { q = keep | rgb8(px); }
            case 5u: { if (!masked) { q = keep | blend(q, 0u, p0); } }
            case 6u: { if (!masked) { q = keep | from565(apply(p0, to565(q), px)); } }
            case 7u: {
                if (!masked) {
                    let qx = min(gid.x + 2u * p0, u.w - 1u);
                    let qy = min(gid.y + 3u * p0, u.h - 1u);
                    q = keep | from565(apply(7u, to565(prev[qy * u.stride + qx]), px));
                }
            }
            case 8u: {
                let mw = i32(p1 & 0xffffu);
                let mh = i32(p1 >> 16u);
                let mx = col + bitcast<i32>(data[c + 11u]);
                let my = row + bitcast<i32>(data[c + 12u]);
                if (mx >= 0 && my >= 0 && mx < mw && my < mh && (atlas[p0 + u32(my * mw + mx)] & 0x10000u) != 0u) {
                    q = keep | rgb8(px);
                }
            }
            case 9u: { q = q | MASK; }
            case 10u: {
                let c8 = rgb8(px);
                let r = ch(c8, 16u); let g = ch(c8, 8u); let b = ch(c8, 0u);
                q = keep | pack(r + (255u - r) * p0 / 255u, g + (255u - g) * p0 / 255u, b + (255u - b) * p0 / 255u);
            }
            case 11u: {
                let c8 = rgb8(px);
                let r = ch(c8, 16u); let g = ch(c8, 8u); let b = ch(c8, 0u);
                let a = min((g - min(g, max(r, b))) * 2u, 255u);
                if (a != 0u) { q = keep | blend(q, c8, a); }
            }
            case 12u: {
                var c8 = rgb8(px);
                if (p0 != 0u) { c8 = pack(ch(c8, 16u) / 2u, ch(c8, 8u) / 2u, ch(c8, 0u) / 2u); }
                q = keep | pack(min(ch(q, 16u) + ch(c8, 16u), 255u), min(ch(q, 8u) + ch(c8, 8u), 255u), min(ch(q, 0u) + ch(c8, 0u), 255u));
            }
            case 13u: { q = keep | blend(q, 0u, p0); }
            case 14u: { q = keep | p0; }
            default: {}
        }
    }
    state[si] = q;
}
"#;

/// Bytes between the per-segment uniform blocks (the dynamic-offset alignment).
const UNIFORM_SLOT: u64 = 256;

struct Sized {
    buffer: wgpu::Buffer,
    bytes: u64,
}

/// The GPU replay: a compute shader that redraws a recorded frame pixel by pixel.
pub struct Replay {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The picture (`stride` words per row, bit 24 = screen mask) and the copy a lens reads.
    state: Option<(wgpu::Buffer, wgpu::Buffer, (usize, usize, usize))>,
    data: Option<Sized>,
    uniforms: Option<Sized>,
    atlas: Option<Sized>,
    /// The atlas generation and word count already on the GPU.
    atlas_synced: (u64, usize),
    ground: Option<Sized>,
    ground_synced: u64,
    max_buffer: u64,
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn bytes_of(words: &[u32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(words.as_ptr() as *const u8, std::mem::size_of_val(words)) }
}

/// A buffer of at least `bytes` (grown by half again when it must be remade).
fn ensure(device: &wgpu::Device, slot: &mut Option<Sized>, bytes: u64, usage: wgpu::BufferUsages, label: &str, max: u64) -> bool {
    let bytes = bytes.max(16).next_multiple_of(16);
    if slot.as_ref().is_some_and(|s| s.bytes >= bytes) {
        return false;
    }
    let size = (bytes + bytes / 2).min(max.max(bytes)).next_multiple_of(16);
    *slot = Some(Sized { buffer: device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false }), bytes: size });
    true
}

impl Replay {
    /// Compiles the shader and builds the pipeline.
    pub fn new(device: &wgpu::Device) -> Replay {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("replay"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("replay"),
            entries: &[
                storage(0, false),
                storage(1, true),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(48),
                    },
                    count: None,
                },
            ],
        });
        let pl =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("replay"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("replay"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let limits = device.limits();
        let max_buffer = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
        record::set_atlas_limit((max_buffer / 4) as usize);
        Replay {
            pipeline,
            layout,
            state: None,
            data: None,
            uniforms: None,
            atlas: None,
            atlas_synced: (u64::MAX, 0),
            ground: None,
            ground_synced: u64::MAX,
            max_buffer,
        }
    }

    /// Words per picture row: rows start on 256-byte boundaries, as a buffer-to-texture copy needs.
    pub fn stride(w: usize) -> usize {
        w.next_multiple_of(64)
    }

    /// Encodes the replay of `rec`; returns the picture buffer and its row stride in words.
    pub fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, rec: &Recording) -> (&wgpu::Buffer, usize) {
        let (w, h) = (rec.w, rec.h);
        let stride = Self::stride(w);
        if self.state.as_ref().map(|s| s.2) != Some((w, h, stride)) {
            let size = (stride * h * 4) as u64;
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
            let a = device.create_buffer(&wgpu::BufferDescriptor { label: Some("picture"), size, usage, mapped_at_creation: false });
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: Some("picture before lens"), size, usage, mapped_at_creation: false });
            self.state = Some((a, b, (w, h, stride)));
        }
        let storage_dst = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        // the sources: only what is new since the last frame
        record::with_sources(|atlas, ground| {
            let words = atlas.data.len();
            let regrown = ensure(device, &mut self.atlas, (words * 4) as u64, storage_dst, "atlas", self.max_buffer);
            let (gen, synced) = self.atlas_synced;
            let from = if regrown || gen != atlas.generation || synced > words { 0 } else { synced };
            if words > from {
                queue.write_buffer(&self.atlas.as_ref().unwrap().buffer, (from * 4) as u64, bytes_of(&atlas.data[from..words]));
            }
            self.atlas_synced = (atlas.generation, words);
            let regrown = ensure(device, &mut self.ground, (ground.data.len() * 4) as u64, storage_dst, "ground", self.max_buffer);
            if (regrown || self.ground_synced != ground.generation) && !ground.data.is_empty() {
                queue.write_buffer(&self.ground.as_ref().unwrap().buffer, 0, bytes_of(&ground.data));
            }
            self.ground_synced = ground.generation;
        });
        // the frame's data: commands, sample maps, tile lists, coverage grids
        let bins = rec.bins();
        let cmd_words = rec.cmds.len() * 16;
        let mut data: Vec<u32> = Vec::with_capacity(cmd_words + rec.maps.len() + bins.offsets.len() + bins.indices.len() + rec.local.len());
        let cmds_at = 0u32;
        data.extend_from_slice(bytes_as_words(&rec.cmds));
        let maps_at = data.len() as u32;
        data.extend(rec.maps.iter().map(|&m| m as u32));
        let offsets_at = data.len() as u32;
        data.extend_from_slice(&bins.offsets);
        let indices_at = data.len() as u32;
        data.extend_from_slice(&bins.indices);
        let local_at = data.len() as u32;
        data.extend_from_slice(&rec.local);
        ensure(device, &mut self.data, (data.len() * 4) as u64, storage_dst, "frame data", self.max_buffer);
        queue.write_buffer(&self.data.as_ref().unwrap().buffer, 0, bytes_of(&data));
        let tiles = (bins.tiles_x * bins.tiles_y) as u32;
        let mut uniforms = vec![0u32; bins.segments.len() * (UNIFORM_SLOT as usize / 4)];
        for s in 0..bins.segments.len() {
            let u = [w as u32, h as u32, stride as u32, bins.tiles_x as u32, tiles, s as u32, cmds_at, maps_at, offsets_at, indices_at, local_at, 0];
            uniforms[s * UNIFORM_SLOT as usize / 4..][..12].copy_from_slice(&u);
        }
        ensure(
            device,
            &mut self.uniforms,
            (uniforms.len() * 4) as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "segments",
            self.max_buffer,
        );
        queue.write_buffer(&self.uniforms.as_ref().unwrap().buffer, 0, bytes_of(&uniforms));

        let (picture, before, _) = self.state.as_ref().unwrap();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("replay"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: whole_buffer(picture) },
                wgpu::BindGroupEntry { binding: 1, resource: whole_buffer(before) },
                wgpu::BindGroupEntry { binding: 2, resource: whole_buffer(&self.data.as_ref().unwrap().buffer) },
                wgpu::BindGroupEntry { binding: 3, resource: whole_buffer(&self.atlas.as_ref().unwrap().buffer) },
                wgpu::BindGroupEntry { binding: 4, resource: whole_buffer(&self.ground.as_ref().unwrap().buffer) },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniforms.as_ref().unwrap().buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(48),
                    }),
                },
            ],
        });
        enc.clear_buffer(picture, 0, None);
        for (s, &first) in bins.segments.iter().enumerate() {
            if rec.cmds.get(first).is_some_and(|c| c.op == record::op::LENS) {
                enc.copy_buffer_to_buffer(picture, 0, before, 0, None);
            }
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("replay"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[(s as u64 * UNIFORM_SLOT) as u32]);
            pass.dispatch_workgroups(bins.tiles_x as u32, bins.tiles_y as u32, 1);
        }
        debug_assert_eq!(TILE, 16, "the shader's workgroup size");
        (picture, stride)
    }

    /// Replays `rec` and reads the picture back (`w * h` words `0x00RRGGBB`): for checking against the CPU.
    pub fn readback(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rec: &Recording) -> Vec<u32> {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("replay check") });
        let (picture, stride) = self.encode(device, queue, &mut enc, rec);
        let size = (stride * rec.h * 4) as u64;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_buffer_to_buffer(picture, 0, &read, 0, size);
        queue.submit([enc.finish()]);
        read.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let view = read.get_mapped_range(..).expect("mapped");
        let words: &[u32] = unsafe { std::slice::from_raw_parts(view.as_ptr() as *const u32, view.len() / 4) };
        let mut out = Vec::with_capacity(rec.w * rec.h);
        for y in 0..rec.h {
            out.extend(words[y * stride..y * stride + rec.w].iter().map(|&q| q & 0xff_ffff));
        }
        out
    }
}

fn whole_buffer(b: &wgpu::Buffer) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: b, offset: 0, size: None })
}

fn bytes_as_words(cmds: &[record::Cmd]) -> &[u32] {
    // `Cmd` is `repr(C)`, sixteen 4-byte fields
    unsafe { std::slice::from_raw_parts(cmds.as_ptr() as *const u32, cmds.len() * 16) }
}
