//! Compression utilities for PRoot Distro

use anyhow::Result;
use bzip2::{read::BzDecoder, write::BzEncoder, Compression as BzipCompression};
use flate2::{read::GzDecoder, write::GzEncoder, Compression as FlateCompression};
use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use tar::{Archive, Builder};
use xz2::read::XzDecoder;
use xz2::write::XzEncoder;

/// Compression format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    Gzip,
    Bzip2,
    Xz,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
}

/// Compressor utility
pub struct Compressor {
    format: CompressionFormat,
    level: u32,
}

impl Compressor {
    pub fn new(format: CompressionFormat, level: u32) -> Self {
        Self { format, level }
    }

    pub fn compress_file(&self, input: &Path, output: &Path) -> Result<()> {
        match self.format {
            CompressionFormat::Gzip => self.compress_gzip(input, output),
            CompressionFormat::Bzip2 => self.compress_bzip2(input, output),
            CompressionFormat::Xz => self.compress_xz(input, output),
            CompressionFormat::Tar => self.create_tar(input, output),
            CompressionFormat::TarGz => self.create_tar_gz(input, output),
            CompressionFormat::TarBz2 => self.create_tar_bz2(input, output),
            CompressionFormat::TarXz => self.create_tar_xz(input, output),
        }
    }

    pub fn decompress_file(&self, input: &Path, output: &Path) -> Result<()> {
        match self.format {
            CompressionFormat::Gzip => self.decompress_gzip(input, output),
            CompressionFormat::Bzip2 => self.decompress_bzip2(input, output),
            CompressionFormat::Xz => self.decompress_xz(input, output),
            CompressionFormat::Tar => self.extract_tar(input, output),
            CompressionFormat::TarGz => self.extract_tar_gz(input, output),
            CompressionFormat::TarBz2 => self.extract_tar_bz2(input, output),
            CompressionFormat::TarXz => self.extract_tar_xz(input, output),
        }
    }

    fn compress_gzip(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        let output_file = fs::File::create(output)?;

        let mut encoder = GzEncoder::new(output_file, FlateCompression::new(self.level));
        let mut reader = BufReader::new(input_file);
        std::io::copy(&mut reader, &mut encoder)?;
        encoder.finish()?;
        Ok(())
    }

    fn compress_bzip2(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        let output_file = fs::File::create(output)?;

        let mut encoder = BzEncoder::new(output_file, BzipCompression::new(self.level));
        let mut reader = BufReader::new(input_file);
        std::io::copy(&mut reader, &mut encoder)?;
        encoder.finish()?;
        Ok(())
    }

    fn compress_xz(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        let output_file = fs::File::create(output)?;

        let mut encoder = XzEncoder::new(output_file, self.level as u32);
        let mut reader = BufReader::new(input_file);
        std::io::copy(&mut reader, &mut encoder)?;
        encoder.finish()?;
        Ok(())
    }

    fn decompress_gzip(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        let output_file = fs::File::create(output)?;

        let mut decoder = GzDecoder::new(BufReader::new(input_file));
        let mut writer = BufWriter::new(output_file);
        std::io::copy(&mut decoder, &mut writer)?;
        Ok(())
    }

    fn decompress_bzip2(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        let output_file = fs::File::create(output)?;

        let mut decoder = BzDecoder::new(BufReader::new(input_file));
        let mut writer = BufWriter::new(output_file);
        std::io::copy(&mut decoder, &mut writer)?;
        Ok(())
    }

    fn decompress_xz(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        let output_file = fs::File::create(output)?;

        let mut decoder = XzDecoder::new(BufReader::new(input_file));
        let mut writer = BufWriter::new(output_file);
        std::io::copy(&mut decoder, &mut writer)?;
        Ok(())
    }

    fn create_tar(&self, input: &Path, output: &Path) -> Result<()> {
        let output_file = fs::File::create(output)?;
        let mut archive = Builder::new(output_file);
        archive.append_dir_all(".", input)?;
        archive.finish()?;
        Ok(())
    }

    fn create_tar_gz(&self, input: &Path, output: &Path) -> Result<()> {
        let output_file = fs::File::create(output)?;
        let mut encoder = GzEncoder::new(output_file, FlateCompression::new(self.level));
        {
            let mut archive = Builder::new(&mut encoder);
            archive.append_dir_all(".", input)?;
            archive.finish()?;
        }
        encoder.finish()?;
        Ok(())
    }

    fn create_tar_bz2(&self, input: &Path, output: &Path) -> Result<()> {
        let output_file = fs::File::create(output)?;
        let mut encoder = BzEncoder::new(output_file, BzipCompression::new(self.level));
        {
            let mut archive = Builder::new(&mut encoder);
            archive.append_dir_all(".", input)?;
            archive.finish()?;
        }
        encoder.finish()?;
        Ok(())
    }

    fn create_tar_xz(&self, input: &Path, output: &Path) -> Result<()> {
        let output_file = fs::File::create(output)?;
        let mut encoder = XzEncoder::new(output_file, self.level as u32);
        {
            let mut archive = Builder::new(&mut encoder);
            archive.append_dir_all(".", input)?;
            archive.finish()?;
        }
        encoder.finish()?;
        Ok(())
    }

    // Extraction goes through `untar::extract`, not `Archive::unpack`: this
    // device denies `link(2)`, so archives containing hard links would fail.
    fn extract_tar(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        crate::untar::extract(input_file, output)
    }

    fn extract_tar_gz(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        crate::untar::extract(GzDecoder::new(BufReader::new(input_file)), output)
    }

    fn extract_tar_bz2(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        crate::untar::extract(BzDecoder::new(BufReader::new(input_file)), output)
    }

    fn extract_tar_xz(&self, input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)?;
        crate::untar::extract(XzDecoder::new(BufReader::new(input_file)), output)
    }
}

/// High-level functions for common operations
pub fn compress_directory(
    input: &Path,
    output: &Path,
    format: CompressionFormat,
    level: u32,
) -> Result<()> {
    let compressor = Compressor::new(format, level);
    compressor.compress_file(input, output)
}

pub fn extract_archive(input: &Path, output: &Path, format: CompressionFormat) -> Result<()> {
    let compressor = Compressor::new(format, 6);
    compressor.decompress_file(input, output)
}
