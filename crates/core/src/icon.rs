//! The icon of the program: PNG files of the `icons` folder, built into the executables.

/// An image as RGBA bytes, row by row.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

const TRAY_PNG: &[u8] = include_bytes!("../../../icons/icon-32.png");
const WINDOW_PNG: &[u8] = include_bytes!("../../../icons/icon-256.png");

fn decode(bytes: &[u8]) -> Rgba {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("the built-in icon is a valid PNG");
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).expect("the built-in icon can be decoded");
    buffer.truncate(info.buffer_size());
    let data = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        other => panic!("the built-in icon has an unexpected colour type {other:?}"),
    };
    Rgba { width: info.width, height: info.height, data }
}

/// The icon of the tray (32 x 32).
pub fn tray() -> Rgba {
    decode(TRAY_PNG)
}

/// The icon of the windows and of the About box (256 x 256).
pub fn window() -> Rgba {
    decode(WINDOW_PNG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_icons_decode_with_transparent_corners() {
        for (icon, side) in [(tray(), 32), (window(), 256)] {
            assert_eq!((icon.width, icon.height), (side, side));
            assert_eq!(icon.data.len(), (side * side * 4) as usize);
            assert_eq!(icon.data[3], 0, "the corner is transparent");
            let centre = ((side / 2 * side + side / 2) * 4 + 3) as usize;
            assert_eq!(icon.data[centre], 255, "the middle is opaque");
        }
    }
}
