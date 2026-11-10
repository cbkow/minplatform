//! Windows: the macOS icon as pixels — the SMALL (title-bar) icon of every window the app opens (canon row 248: the title
//! bar carries the macOS icon; the taskbar's is the exe's own icon resource).
/// RGBA, width, height of the 64-px macOS icon.
pub fn rgba() -> Option<(Vec<u8>, u32, u32)> {
    let mut d = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../assets/icons/qcviewMac-64.png")[..]));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16);
    let mut rd = d.read_info().ok()?;
    let mut buf = vec![0u8; rd.output_buffer_size()];
    let info = rd.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some((buf, info.width, info.height))
}
