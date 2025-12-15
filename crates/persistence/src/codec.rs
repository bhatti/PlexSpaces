// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Codec module for compression and encryption of snapshot data
//!
//! ## Purpose
//! Provides transparent compression and encryption for snapshot state data to:
//! - Reduce storage size (compression: 2-5x smaller)
//! - Secure sensitive data (encryption: AES-256-GCM)
//! - Maintain performance (minimal overhead)
//!
//! ## Compression Algorithms
//! - **Snappy**: Fast compression (~2x), lower CPU overhead
//! - **Zstd**: Balanced compression (~3-5x), good CPU/space tradeoff
//!
//! ## Encryption Algorithm
//! - **AES-256-GCM**: Authenticated encryption, prevents tampering

use crate::{CompressionType, EncryptionType, JournalError};
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use snap::raw::{Decoder as SnapDecoder, Encoder as SnapEncoder};
use std::io::{Read, Write};
use zstd::stream::{read::Decoder as ZstdDecoder, write::Encoder as ZstdEncoder};

/// Encryption key for AES-256-GCM
///
/// ## Security Note
/// In production, derive this from a secure key management system (KMS),
/// environment variable, or key file. Never hardcode keys in source code.
pub type EncryptionKey = [u8; 32]; // 256 bits for AES-256

/// Compress data using the specified algorithm
///
/// ## Arguments
/// * `data` - Raw data to compress
/// * `compression` - Compression algorithm to use
///
/// ## Returns
/// Compressed data or original if compression is None
///
/// ## Errors
/// Returns JournalError::SerializationError if compression fails
pub fn compress(data: &[u8], compression: CompressionType) -> Result<Vec<u8>, JournalError> {
    match compression {
        CompressionType::None => Ok(data.to_vec()),

        CompressionType::Snappy => {
            let mut encoder = SnapEncoder::new();
            encoder.compress_vec(data).map_err(|e| {
                JournalError::SerializationError(format!("Snappy compression failed: {}", e))
            })
        }

        CompressionType::Zstd => {
            let mut encoder = ZstdEncoder::new(Vec::new(), 3)?; // Level 3 for balanced performance
            encoder.write_all(data).map_err(|e| {
                JournalError::SerializationError(format!("Zstd write failed: {}", e))
            })?;
            encoder.finish().map_err(|e| {
                JournalError::SerializationError(format!("Zstd compression failed: {}", e))
            })
        }
    }
}

/// Decompress data using the specified algorithm
///
/// ## Arguments
/// * `data` - Compressed data
/// * `compression` - Compression algorithm that was used
///
/// ## Returns
/// Decompressed data or original if compression is None
///
/// ## Errors
/// Returns JournalError::SerializationError if decompression fails or data is corrupted
pub fn decompress(data: &[u8], compression: CompressionType) -> Result<Vec<u8>, JournalError> {
    match compression {
        CompressionType::None => Ok(data.to_vec()),

        CompressionType::Snappy => {
            let mut decoder = SnapDecoder::new();
            decoder
                .decompress_vec(data)
                .map_err(|e| JournalError::Corrupted(format!("Snappy decompression failed: {}", e)))
        }

        CompressionType::Zstd => {
            let mut decoder = ZstdDecoder::new(data)?;
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map_err(|e| {
                JournalError::Corrupted(format!("Zstd decompression failed: {}", e))
            })?;
            Ok(decompressed)
        }
    }
}

/// Encrypt data using the specified algorithm
///
/// ## Arguments
/// * `data` - Raw data to encrypt
/// * `encryption` - Encryption algorithm to use
/// * `key` - Encryption key (32 bytes for AES-256)
///
/// ## Returns
/// Encrypted data with nonce prepended (12 bytes nonce + ciphertext)
///
/// ## Format
/// ```text
/// [12-byte nonce][encrypted data][16-byte auth tag]
/// ```
///
/// ## Errors
/// Returns JournalError::SerializationError if encryption fails
pub fn encrypt(
    data: &[u8],
    encryption: EncryptionType,
    key: &EncryptionKey,
) -> Result<Vec<u8>, JournalError> {
    match encryption {
        EncryptionType::None => Ok(data.to_vec()),

        EncryptionType::Aes256Gcm => {
            let cipher = Aes256Gcm::new(key.into());
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

            let ciphertext = cipher.encrypt(&nonce, data).map_err(|e| {
                JournalError::SerializationError(format!("AES encryption failed: {}", e))
            })?;

            // Prepend nonce to ciphertext for decryption
            let mut result = nonce.to_vec();
            result.extend_from_slice(&ciphertext);
            Ok(result)
        }
    }
}

/// Decrypt data using the specified algorithm
///
/// ## Arguments
/// * `data` - Encrypted data with nonce prepended
/// * `encryption` - Encryption algorithm that was used
/// * `key` - Encryption key (must match encryption key)
///
/// ## Returns
/// Decrypted data
///
/// ## Errors
/// Returns JournalError::Corrupted if:
/// - Data is too short (missing nonce)
/// - Key is incorrect
/// - Data has been tampered with (auth tag verification fails)
pub fn decrypt(
    data: &[u8],
    encryption: EncryptionType,
    key: &EncryptionKey,
) -> Result<Vec<u8>, JournalError> {
    match encryption {
        EncryptionType::None => Ok(data.to_vec()),

        EncryptionType::Aes256Gcm => {
            if data.len() < 12 {
                return Err(JournalError::Corrupted(
                    "Encrypted data too short (missing nonce)".to_string(),
                ));
            }

            let cipher = Aes256Gcm::new(key.into());
            let (nonce_bytes, ciphertext) = data.split_at(12);
            let nonce = Nonce::from(*<&[u8; 12]>::try_from(nonce_bytes).unwrap());

            cipher.decrypt(&nonce, ciphertext).map_err(|e| {
                JournalError::Corrupted(format!(
                    "AES decryption failed (wrong key or tampered data): {}",
                    e
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_none() {
        let data = b"Hello, World!";
        let compressed = compress(data, CompressionType::None).unwrap();
        assert_eq!(compressed, data);

        let decompressed = decompress(&compressed, CompressionType::None).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_decompress_snappy() {
        let data = b"Hello, World! This is a test of Snappy compression.";
        let compressed = compress(data, CompressionType::Snappy).unwrap();
        // Snappy may not compress short strings, but should not fail
        assert!(!compressed.is_empty());

        let decompressed = decompress(&compressed, CompressionType::Snappy).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_decompress_snappy_large() {
        // Create larger data for better compression ratio
        let data = b"A".repeat(1000);
        let compressed = compress(&data, CompressionType::Snappy).unwrap();
        assert!(
            compressed.len() < data.len(),
            "Snappy should compress repeated data"
        );

        let decompressed = decompress(&compressed, CompressionType::Snappy).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_decompress_zstd() {
        let data = b"Hello, World! This is a test of Zstd compression.";
        let compressed = compress(data, CompressionType::Zstd).unwrap();
        assert!(!compressed.is_empty());

        let decompressed = decompress(&compressed, CompressionType::Zstd).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_decompress_zstd_large() {
        // Create larger data for better compression ratio
        let data = b"B".repeat(1000);
        let compressed = compress(&data, CompressionType::Zstd).unwrap();
        assert!(
            compressed.len() < data.len(),
            "Zstd should compress repeated data"
        );

        let decompressed = decompress(&compressed, CompressionType::Zstd).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_encrypt_decrypt_none() {
        let data = b"Secret data";
        let key = [0u8; 32]; // Dummy key (not used for None)

        let encrypted = encrypt(data, EncryptionType::None, &key).unwrap();
        assert_eq!(encrypted, data);

        let decrypted = decrypt(&encrypted, EncryptionType::None, &key).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_decrypt_aes256gcm() {
        let data = b"Very secret data that must be encrypted!";
        let key = [42u8; 32]; // Test key

        let encrypted = encrypt(data, EncryptionType::Aes256Gcm, &key).unwrap();
        assert_ne!(
            encrypted, data,
            "Encrypted data should differ from plaintext"
        );
        assert!(
            encrypted.len() > data.len(),
            "Encrypted data includes nonce and auth tag"
        );

        let decrypted = decrypt(&encrypted, EncryptionType::Aes256Gcm, &key).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let data = b"Secret data";
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];

        let encrypted = encrypt(data, EncryptionType::Aes256Gcm, &key1).unwrap();

        // Decrypt with wrong key should fail
        let result = decrypt(&encrypted, EncryptionType::Aes256Gcm, &key2);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), JournalError::Corrupted(_)));
    }

    #[test]
    fn test_decrypt_corrupted_data_fails() {
        let data = b"Secret data";
        let key = [42u8; 32];

        let mut encrypted = encrypt(data, EncryptionType::Aes256Gcm, &key).unwrap();

        // Corrupt the data (flip a bit in the ciphertext)
        if encrypted.len() > 12 {
            encrypted[20] ^= 1;
        }

        // Decrypt corrupted data should fail
        let result = decrypt(&encrypted, EncryptionType::Aes256Gcm, &key);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), JournalError::Corrupted(_)));
    }

    #[test]
    fn test_compress_then_encrypt() {
        let data = b"C".repeat(1000); // Compressible data
        let key = [99u8; 32];

        // Compress first
        let compressed = compress(&data, CompressionType::Zstd).unwrap();
        assert!(compressed.len() < data.len());

        // Then encrypt
        let encrypted = encrypt(&compressed, EncryptionType::Aes256Gcm, &key).unwrap();

        // Decrypt first
        let decrypted = decrypt(&encrypted, EncryptionType::Aes256Gcm, &key).unwrap();

        // Then decompress
        let decompressed = decompress(&decrypted, CompressionType::Zstd).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compression_ratios() {
        let data = b"D".repeat(10000); // 10KB of repeated data

        // No compression
        let none = compress(&data, CompressionType::None).unwrap();
        assert_eq!(none.len(), data.len());

        // Snappy
        let snappy = compress(&data, CompressionType::Snappy).unwrap();
        let snappy_ratio = data.len() as f64 / snappy.len() as f64;
        println!("Snappy compression ratio: {:.2}x", snappy_ratio);
        assert!(
            snappy_ratio >= 1.5,
            "Snappy should achieve at least 1.5x on repeated data"
        );

        // Zstd
        let zstd = compress(&data, CompressionType::Zstd).unwrap();
        let zstd_ratio = data.len() as f64 / zstd.len() as f64;
        println!("Zstd compression ratio: {:.2}x", zstd_ratio);
        assert!(
            zstd_ratio >= 3.0,
            "Zstd should achieve at least 3x on repeated data"
        );
    }
}
