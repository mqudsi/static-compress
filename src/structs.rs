use ::*;
use errors::*;
use std::path::Path;

pub struct Parameters {
    pub compressor: CompressionAlgorithm,
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

trait DefaultFileCompressor {
    fn compress(source: AsRef<Path>, destination: AsRef<Path>) -> Result<()>;
}

trait FileCompressor {
    fn compress(source: AsRef<Path>, destination: AsRef<Path>, level: u8);
}
