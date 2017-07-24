extern crate brotli2;
extern crate flate2;
extern crate zopfli;

use structs::*;
use errors::*;
use std::fs::File;
use std::io::{Read, Write};

use std::path::Path;

impl CompressionFormat for CompressionAlgorithm {
    fn extension(&self) -> &'static str {
        match self {
            &CompressionAlgorithm::Brotli => "br",
            &CompressionAlgorithm::GZip => "gz",
            &CompressionAlgorithm::WebP => "webp",
            &CompressionAlgorithm::Zopfli => "gz",
        }
    }
}

impl FileCompressor for CompressionAlgorithm {
    fn compress(&self, src: &Path, dst: &Path, quality: Option<u8>) -> Result<()> {
        match self {
            &CompressionAlgorithm::GZip => gzip_compress(src, dst, quality),
            &CompressionAlgorithm::Brotli => brotli_compress(src, dst),
            &CompressionAlgorithm::WebP => webp_compress(src, dst, quality),
            &CompressionAlgorithm::Zopfli => zopfli_compress(src, dst),
            // _ => bail!("Compression algorithm not implemented!"),
        }
    }
}

fn gzip_compress(src_path: &Path, dst_path: &Path, quality: Option<u8>) -> Result<()> {
    let mut src = File::open(src_path)?;
    let dst = File::create(dst_path)?;

    let level = match quality {
        None => flate2::Compression::Default,
        Some(0) => flate2::Compression::None,
        Some(1) => flate2::Compression::Fast,
        Some(2...6) => flate2::Compression::Default,
        Some(3...9) => flate2::Compression::Best,
        _ => bail!("Invalid --quality parameter specified!"),
    };

    let mut encoder = flate2::write::GzEncoder::new(dst, level);
    let mut buf = [0u8; 1024];
    loop {
        let bytes_read = src.read(&mut buf).chain_err(|| "Error reading from source file!")?;
        match bytes_read {
            0 => break, //end of file
            l => encoder.write_all(&buf[0..l]).chain_err(|| "Fatal gzip encoder error!")?,
        };
    }

    Ok(())
}

fn brotli_compress(src_path: &Path, dst_path: &Path) -> Result<()> {
    let mut src = File::open(src_path)?;
    let dst = File::create(dst_path)?;

    let mut buf = [0u8; 1024];
    let mut encoder = brotli2::write::BrotliEncoder::new(dst, 6);
    loop {
        let bytes_read = src.read(&mut buf).chain_err(|| "Error reading from source file!")?;
        match bytes_read {
            0 => break, //end of file
            l => encoder.write_all(&buf[0..l]).chain_err(|| "Fatal gzip encoder error!")?,
        };
    }

    Ok(())
}

fn zopfli_compress(src_path: &Path, dst_path: &Path) -> Result<()> {
    let mut src = File::open(src_path)?;
    let mut dst = File::create(dst_path)?;

    let mut src_data = Vec::<u8>::new();
    src.read_to_end(&mut src_data)?;

    zopfli::compress(&zopfli::Options::default(), &zopfli::Format::Gzip, &mut src_data, &mut dst)?;

    Ok(())
}

fn webp_compress(src_path: &Path, dst_path: &Path, quality: Option<u8>) -> Result<()> {
    use std::process::Command;

    let output = Command::new("cwebp")
        .arg("-q")
        .arg(quality.unwrap_or(90).to_string())
        .arg(src_path.as_os_str())
        .arg("-o")
        .arg(dst_path.as_os_str())
        .output()
        .chain_err(|| "Error executing cwebp!")?;

    if !output.status.success() {
        bail!("Error compressing via webp: {:?}", output);
    }

    Ok(())
}
