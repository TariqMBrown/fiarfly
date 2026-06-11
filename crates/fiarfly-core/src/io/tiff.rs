//! TIFF stack loader.

use crate::{FiarflyError, Frame, ImageStack};
use ndarray::s;
use std::fs::File;
use std::io::BufReader;
use tiff::decoder::{Decoder, DecodingResult};

/// Load all frames of a multi-page TIFF into an `ImageStack`.
pub fn load_tiff(path: &std::path::Path) -> Result<ImageStack, FiarflyError> {
    TiffReader::open(path)?.load_all()
}

/// Streaming TIFF reader — opens a new file handle per request so it is `Send`.
pub struct TiffReader {
    pub path: std::path::PathBuf,
    pub num_frames: usize,
    pub height: usize,
    pub width: usize,
    #[allow(dead_code)]
    bit_depth: u32,
}

impl TiffReader {
    pub fn open(path: &std::path::Path) -> Result<Self, FiarflyError> {
        let mut decoder = Self::make_decoder(path)?;

        let (width, height) = decoder.dimensions()?;
        let bit_depth = match decoder.colortype()? {
            tiff::ColorType::Gray(n) | tiff::ColorType::GrayA(n) => n as u32,
            _ => 16,
        };

        // Count frames by advancing until the decoder errors.
        let mut num_frames = 1usize;
        while decoder.next_image().is_ok() {
            num_frames += 1;
        }

        Ok(Self {
            path: path.to_path_buf(),
            num_frames,
            height: height as usize,
            width: width as usize,
            bit_depth,
        })
    }

    pub fn get_frame(&self, idx: usize) -> Result<Frame, FiarflyError> {
        let mut decoder = Self::make_decoder(&self.path)?;
        for _ in 0..idx {
            decoder.next_image()?;
        }
        let result = decoder.read_image()?;
        self.decode(result)
    }

    pub fn load_all(&self) -> Result<ImageStack, FiarflyError> {
        let mut decoder = Self::make_decoder(&self.path)?;
        let mut stack = ImageStack::zeros([self.num_frames, self.height, self.width]);
        for i in 0..self.num_frames {
            if i > 0 {
                decoder.next_image()?;
            }
            let result = decoder.read_image()?;
            let frame = self.decode(result)?;
            stack.slice_mut(s![i, .., ..]).assign(&frame);
        }
        Ok(stack)
    }

    fn make_decoder(path: &std::path::Path) -> Result<Decoder<BufReader<File>>, FiarflyError> {
        let file = File::open(path)?;
        Ok(Decoder::new(BufReader::new(file))?)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TIFF writer
// ─────────────────────────────────────────────────────────────────────────────

/// Write an `ImageStack` to a multi-page TIFF (U16, scaled from the [0,1]
/// float representation).  Compatible with Fiji/ImageJ and most analysis tools.
pub fn save_stack(stack: &ImageStack, path: &std::path::Path) -> Result<(), FiarflyError> {
    use std::io::BufWriter;
    use tiff::encoder::{colortype::Gray16, TiffEncoder};

    let (n_frames, height, width) = stack.dim();
    let file = std::fs::File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut encoder =
        TiffEncoder::new(&mut writer).map_err(|e| FiarflyError::Export(e.to_string()))?;

    for i in 0..n_frames {
        let frame = stack.slice(ndarray::s![i, .., ..]);
        let pixels: Vec<u16> = frame
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
            .collect();
        encoder
            .write_image::<Gray16>(width as u32, height as u32, &pixels)
            .map_err(|e| FiarflyError::Export(e.to_string()))?;
    }
    Ok(())
}

impl TiffReader {
    fn decode(&self, result: DecodingResult) -> Result<Frame, FiarflyError> {
        let pixels: Vec<f32> = match result {
            DecodingResult::U8(v) => v.into_iter().map(|x| x as f32 / 255.0).collect(),
            DecodingResult::U16(v) => v.into_iter().map(|x| x as f32 / 65535.0).collect(),
            DecodingResult::U32(v) => v.into_iter().map(|x| x as f32 / u32::MAX as f32).collect(),
            DecodingResult::F32(v) => v,
            DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
            _ => {
                return Err(FiarflyError::UnsupportedFormat(
                    "Pixel type not supported. Need U8, U16, U32, F32, or F64 grayscale TIFF."
                        .into(),
                ))
            }
        };

        ndarray::Array2::from_shape_vec((self.height, self.width), pixels)
            .map_err(|e| FiarflyError::DimensionMismatch(e.to_string()))
    }
}
