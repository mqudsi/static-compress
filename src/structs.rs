use ::*;
use errors::*;
use std::path::Path;

pub struct Parameters {
    pub compressor: CompressionAlgorithm,
    pub extension: String,
    pub include_filters: Vec<String>,
    pub threads: usize,
}

pub enum CompressionAlgorithm {
    Brotli,
    GZip,
    Zopfli,
}

impl std::str::FromStr for CompressionAlgorithm {
    type Err = errors::Error;
    fn from_str(s: &str) -> Result<Self> {
        let r = match s {
            "gzip" => CompressionAlgorithm::GZip,
            "brotli" => CompressionAlgorithm::Brotli,
            "zopfli" => CompressionAlgorithm::Zopfli,
            _ => bail!("Unsupported compression algorithm option set!"),
        };

        return Ok(r);
    }
}

pub trait DefaultFileCompressor {
    fn compress(&self, source: &Path, destination: &Path) -> Result<()>;
}

pub trait FileCompressor {
    fn compress(&self, source: &Path, destination: &Path, level: u8);
}

pub trait CompressionFormat {
    fn extension(&self) -> &'static str;
}
