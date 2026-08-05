pub(crate) fn encode_rgba8(
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(pixels)?;
    }
    Ok(bytes)
}
