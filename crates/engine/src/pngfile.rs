//! A minimal PNG writer: 8-bit RGB, one zlib IDAT. The compression is `flate2`, which QymCAD's own dependencies
//! already build (ADR 0004); the container (signature, chunks, CRC-32) is the few lines below.

use std::io::Write;

/// Encode `rgb` (`width * height * 3` bytes, rows top to bottom) as a PNG file.
pub(crate) fn encode_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let stride = width as usize * 3;
    assert_eq!(rgb.len(), stride * height as usize, "pixel buffer size");
    // every scanline with filter 2 (Up): flat shaded faces become runs of zeros, which deflate well
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(2u8);
        let row = &rgb[y * stride..(y + 1) * stride];
        if y == 0 {
            raw.extend_from_slice(row);
        } else {
            let prev = &rgb[(y - 1) * stride..y * stride];
            raw.extend(row.iter().zip(prev).map(|(a, b)| a.wrapping_sub(*b)));
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).expect("writing to a Vec cannot fail");
    let idat = z.finish().expect("writing to a Vec cannot fail");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth 8, colour type 2 (RGB), deflate, adaptive filters, no interlace
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// CRC-32 (IEEE 802.3, reflected, polynomial 0xEDB88320), as PNG chunks use it. Bitwise: images are small.
fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082); // the constant tail of every PNG
    }

    #[test]
    fn decodes_with_an_independent_decoder() {
        let (w, h) = (7u32, 5u32);
        let rgb: Vec<u8> = (0..w * h * 3).map(|i| (i * 37 % 251) as u8).collect();
        let file = encode_rgb(w, h, &rgb);
        let mut dec = ::png::Decoder::new(std::io::Cursor::new(file)).read_info().unwrap();
        let mut buf = vec![0; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height, info.color_type, info.bit_depth), (w, h, ::png::ColorType::Rgb, ::png::BitDepth::Eight));
        assert_eq!(&buf[..info.buffer_size()], &rgb[..]);
    }
}
