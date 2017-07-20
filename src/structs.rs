use ::*;
use errors::*;
use pretty_bytes::converter::convert;
use separator::Separatable;
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

pub struct Statistics {
    total_compressed: u64,
    total_compressed_now: u64,
    total_file_count: u32,
    total_file_count_now: u32,
    total_uncompressed: u64,
    total_uncompressed_now: u64,
}

impl Statistics {
    pub fn new() -> Statistics {
        Statistics {
            total_compressed: 0,
            total_compressed_now: 0,
            total_file_count: 0,
            total_file_count_now: 0,
            total_uncompressed: 0,
            total_uncompressed_now: 0,
        }
    }

    pub fn update(&mut self, uncompressed_size: u64, compressed_size: u64, newly_compressed: bool) {
        if newly_compressed {
            self.total_compressed_now += compressed_size;
            self.total_file_count_now += 1;
            self.total_uncompressed_now += uncompressed_size;
        }

        self.total_compressed += compressed_size;
        self.total_file_count += 1;
        self.total_uncompressed += uncompressed_size;
    }

    pub fn merge(&mut self, other: &Statistics) {
        self.total_compressed += other.total_compressed;
        self.total_compressed_now += other.total_compressed_now;
        self.total_file_count += other.total_file_count;
        self.total_file_count_now += other.total_file_count_now;
        self.total_uncompressed += other.total_uncompressed;
        self.total_uncompressed_now += other.total_uncompressed_now;
    }

    pub fn savings_ratio(&self) -> f32 {
        return self.total_compressed as f32 / self.total_uncompressed as f32;
    }

    pub fn savings_ratio_now(&self) -> f32 {
        return self.total_compressed_now as f32 / self.total_uncompressed_now as f32;
    }
}

impl std::fmt::Display for Statistics {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "");
        let table = table!(["", "This Run", "Total"],
                           ["Count", self.total_file_count_now.separated_string(), self.total_file_count.separated_string()],
                           ["Compressed Size", convert(self.total_compressed_now as f64), convert(self.total_compressed as f64)],
                           ["Uncompressed Size", convert(self.total_uncompressed_now as f64), convert(self.total_uncompressed as f64)],
                           ["Total Savings", format!("{:.2}%", 100f32 - 100f32 * self.savings_ratio_now()), format!("{:.2}%", 100f32 - 100f32 * self.savings_ratio())]);

        writeln!(f, "{}", table);
        Ok(())
    }
}
