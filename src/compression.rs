use ::*;
use structs::*;
use errors::*;

use std::path::Path;

impl CompressionFormat for CompressionAlgorithm {
    fn extension(&self) -> &'static str {
        match self {
            &CompressionAlgorithm::Brotli => "br",
            &CompressionAlgorithm::GZip => "gz",
            &CompressionAlgorithm::Zopfli => "gz",
        }
    }
}

impl DefaultFileCompressor for CompressionAlgorithm {
    fn compress(&self, src: &Path, dst: &Path) -> Result<()> {
        Ok(())
    }
}
