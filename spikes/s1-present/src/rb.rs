//! Readbacks + half-float helpers shared by the snapshot / verify paths.
pub fn read_buffer(device: &wgpu::Device, queue: &wgpu::Queue, src: &wgpu::Buffer, size: u64) -> (Vec<u8>, wgpu::Buffer) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(src, 0, &buf, 0, size);
    queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| { tx.send(r).ok(); });
    device.poll(wgpu::PollType::wait_indefinitely()).ok();
    rx.recv().unwrap().expect("map");
    let data = slice.get_mapped_range().expect("mapped range").to_vec();
    buf.unmap();
    (data, buf)
}

pub fn read_rgba16f(device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<f32> {
    let pitch = (w * 8).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("tex-readback"), size: (pitch * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    queue.submit([enc.finish()]);
    let (data, _) = read_buffer(device, queue, &buf, (pitch * h) as u64);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize { let row = &data[y * pitch as usize..y * pitch as usize + (w * 8) as usize]; out.extend(row.chunks(2).map(|b| h2f(u16::from_le_bytes([b[0], b[1]])))); }
    out
}

pub fn h2f(h: u16) -> f32 {
    let (s, e, m) = (((h >> 15) & 1) as u32, ((h >> 10) & 0x1f) as u32, (h & 0x3ff) as u32);
    let bits = if e == 0 { if m == 0 { s << 31 } else { let (mut e2, mut m2) = (113u32, m); while m2 & 0x400 == 0 { m2 <<= 1; e2 -= 1; } (s << 31) | (e2 << 23) | ((m2 & 0x3ff) << 13) } }
        else if e == 31 { (s << 31) | 0x7f80_0000 | (m << 13) } else { (s << 31) | ((e + 112) << 23) | (m << 13) };
    f32::from_bits(bits)
}
pub fn f2h(f: f32) -> u16 {
    let b = f.to_bits(); let s = ((b >> 16) & 0x8000) as u16; let e = ((b >> 23) & 0xff) as i32; let m = b & 0x7f_ffff;
    if e == 255 { return s | 0x7c00 | if m != 0 { 0x200 } else { 0 } }
    let e2 = e - 127 + 15;
    if e2 >= 31 { return s | 0x7c00 }
    if e2 <= 0 { if e2 < -10 { return s } let m2 = (m | 0x80_0000) >> (1 - e2); return s | ((m2 + 0x1000) >> 13) as u16 }
    let v = ((e2 as u32) << 10) | (m >> 13); let round = if (m & 0x1fff) > 0x1000 || ((m & 0x1fff) == 0x1000 && (v & 1) == 1) { 1 } else { 0 };
    s | (v + round) as u16
}
