//! Windows: reading an image file through the system's own imaging (WIC), whatever kind of file it
//! is that the system can read: PNG, JPEG, GIF, BMP, TIFF, ICO and WebP on any Windows that Perora
//! runs on, and more (HEIC, AVIF, camera RAW) where the user has added what reads them. What the
//! system cannot read is not read: there is no list of kinds here to keep up to date.

use std::path::Path;

use windows::Win32::Foundation::{GENERIC_READ, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapDecoder,
    IWICBitmapFrameDecode, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::VT_UI2;
use windows::core::{HSTRING, w};

use crate::geometry::Rect;
use crate::platform::{Frame, ImageError};

/// COM for this thread, for as long as it is held: taken here when the thread had none, and given
/// back as it goes. A thread that has COM already (of another kind) keeps it, and reads with that.
struct Com {
    taken: bool,
}

impl Com {
    fn take() -> Self {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // (Already there in another kind: the imaging works in either, and it is not given back.)
        Self {
            taken: result.is_ok() && result != RPC_E_CHANGED_MODE,
        }
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.taken {
            unsafe { CoUninitialize() };
        }
    }
}

/// Reads the image in a file: its first picture (of an animation, or a file of several), as it is
/// to be seen (a photo taken sideways is turned as its camera said), eight bits of each colour and
/// of opacity. Refused when the system cannot read it, or when it has more than `most` pixels or
/// more than `longest` a side (what can be drawn on, and leave, has its limits).
pub fn read(path: &Path, most: u64, longest: u32) -> Result<Frame, ImageError> {
    let _com = Com::take();
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(unreadable)?;
        let decoder = factory
            .CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(unreadable)?;
        decode(&factory, &decoder, most, longest)
    }
}

/// Reads an image that is in memory (a file's bytes, as a server sent them), as `read` does a file.
pub fn read_bytes(bytes: &[u8], most: u64, longest: u32) -> Result<Frame, ImageError> {
    let _com = Com::take();
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(unreadable)?;
        let stream = factory.CreateStream().map_err(unreadable)?;
        stream.InitializeFromMemory(bytes).map_err(unreadable)?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(unreadable)?;
        decode(&factory, &decoder, most, longest)
    }
}

fn unreadable(error: windows::core::Error) -> ImageError {
    ImageError::Unreadable(error.message())
}

/// The first picture of what the decoder reads, as `read` gives it.
unsafe fn decode(
    factory: &IWICImagingFactory,
    decoder: &IWICBitmapDecoder,
    most: u64,
    longest: u32,
) -> Result<Frame, ImageError> {
    unsafe {
        let picture = decoder.GetFrame(0).map_err(unreadable)?;
        let (mut width, mut height) = (0u32, 0u32);
        picture
            .GetSize(&mut width, &mut height)
            .map_err(unreadable)?;
        if width == 0 || height == 0 {
            return Err(ImageError::Unreadable(String::new()));
        }
        if u64::from(width) * u64::from(height) > most || width > longest || height > longest {
            return Err(ImageError::TooLarge { width, height });
        }
        let converter = factory.CreateFormatConverter().map_err(unreadable)?;
        converter
            .Initialize(
                &picture,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(unreadable)?;
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        converter
            .CopyPixels(std::ptr::null(), width * 4, &mut rgba)
            .map_err(unreadable)?;
        let frame = Frame {
            area: Rect::new(0, 0, width, height),
            rgba,
        };
        Ok(frame.oriented(orientation(&picture)))
    }
}

/// How the camera says the picture is to be turned (EXIF orientation, 1 to 8); 1 (as it is) when
/// the file says nothing of it.
fn orientation(picture: &IWICBitmapFrameDecode) -> u16 {
    unsafe {
        let Ok(reader) = picture.GetMetadataQueryReader() else {
            return 1;
        };
        let mut value = PROPVARIANT::default();
        let said = reader
            .GetMetadataByName(w!("System.Photo.Orientation"), &mut value)
            .is_ok()
            && value.Anonymous.Anonymous.vt == VT_UI2;
        let turned = if said {
            value.Anonymous.Anonymous.Anonymous.uiVal
        } else {
            1
        };
        let _ = PropVariantClear(&mut value);
        turned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Frame {
        // Three across, two down, each pixel its own colour and opacity.
        let rgba = (0u8..24).map(|byte| byte * 10).collect();
        Frame {
            area: Rect::new(0, 0, 3, 2),
            rgba,
        }
    }

    fn written(name: &str, file: &[u8]) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("perora-imaging-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, file).unwrap();
        path
    }

    #[test]
    fn reads_a_png_in_memory_as_a_file() {
        let original = picture();
        let read = read_bytes(&original.to_png().unwrap(), 1 << 20, 1 << 14).unwrap();
        assert_eq!(read.area, Rect::new(0, 0, 3, 2));
        assert_eq!(read.rgba, original.rgba);
        assert!(matches!(
            read_bytes(b"not an image", 1 << 20, 1 << 14),
            Err(ImageError::Unreadable(_))
        ));
    }

    #[test]
    fn reads_a_png_as_it_was_written() {
        let original = picture();
        let path = written("picture.png", &original.to_png().unwrap());
        let read = read(&path, 1 << 20, 1 << 14).unwrap();
        assert_eq!(read.area, Rect::new(0, 0, 3, 2));
        assert_eq!(read.rgba, original.rgba);
    }

    #[test]
    fn reads_a_bmp() {
        let original = picture();
        let path = written("picture.bmp", &original.to_bmp());
        let read = read(&path, 1 << 20, 1 << 14).unwrap();
        assert_eq!(read.area, Rect::new(0, 0, 3, 2));
        // (A BMP of this kind holds no opacity: the colours are what is compared.)
        let colours = |frame: &Frame| -> Vec<u8> {
            frame
                .rgba
                .chunks(4)
                .flat_map(|pixel| pixel[..3].to_vec())
                .collect()
        };
        assert_eq!(colours(&read), colours(&original));
    }

    #[test]
    fn a_photo_is_turned_as_its_camera_said() {
        // Four across, two down: red on the left half, blue on the right.
        let (width, height) = (4u16, 2u16);
        let rgb: Vec<u8> = (0..height)
            .flat_map(|_| (0..width).flat_map(|x| if x < 2 { [255, 0, 0] } else { [0, 0, 255] }))
            .collect();
        let mut plain = Vec::new();
        jpeg_encoder::Encoder::new(&mut plain, 100)
            .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        // What a camera writes after the start of the file: EXIF, saying "turn a quarter clockwise"
        // (orientation 6), as one entry of the first directory of a little-endian TIFF.
        let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0".to_vec();
        exif.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        exif.extend_from_slice(&[0, 0, 0, 0]);
        let mut photo = plain[..2].to_vec();
        photo.extend_from_slice(&[0xFF, 0xE1]);
        photo.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        photo.extend_from_slice(&exif);
        photo.extend_from_slice(&plain[2..]);

        let path = written("photo.jpg", &photo);
        let turned = read(&path, 1 << 20, 1 << 14).unwrap();
        // Turned: two across, four down, red on top.
        assert_eq!(turned.area, Rect::new(0, 0, 2, 4));
        let reddish = |pixel: &[u8]| pixel[0] > 200 && pixel[2] < 60;
        let pixels: Vec<&[u8]> = turned.rgba.chunks(4).collect();
        assert!(pixels[..4].iter().all(|pixel| reddish(pixel)));
        assert!(pixels[4..].iter().all(|pixel| !reddish(pixel)));

        // The same picture without that: as it is.
        let path = written("plain.jpg", &plain);
        assert_eq!(
            read(&path, 1 << 20, 1 << 14).unwrap().area,
            Rect::new(0, 0, 4, 2)
        );
    }

    #[test]
    fn refuses_what_is_too_large() {
        let path = written("large.png", &picture().to_png().unwrap());
        assert!(matches!(
            read(&path, 5, 1 << 14),
            Err(ImageError::TooLarge {
                width: 3,
                height: 2
            })
        ));
        assert!(matches!(
            read(&path, 1 << 20, 2),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn refuses_what_is_not_an_image() {
        let path = written("not.png", b"not a picture, whatever its name says");
        assert!(matches!(
            read(&path, 1 << 20, 1 << 14),
            Err(ImageError::Unreadable(_))
        ));
        let gone = std::env::temp_dir().join("perora-imaging-nothing-here.png");
        assert!(matches!(
            read(&gone, 1 << 20, 1 << 14),
            Err(ImageError::Unreadable(_))
        ));
    }
}
