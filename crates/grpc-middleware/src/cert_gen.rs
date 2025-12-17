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

//! Certificate Generation for mTLS
//!
//! ## Purpose
//! Auto-generates CA and server certificates for mTLS authentication.
//! Used for local development and testing.
//!
//! ## Security Note
//! Auto-generated certificates are for development only.
//! Production should use proper certificate management (cert-manager, Vault, etc.)

use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Certificate generation errors
#[derive(Debug, Error)]
pub enum CertGenError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Certificate generation failed
    #[error("Certificate generation failed: {0}")]
    GenerationFailed(String),

    /// Invalid certificate directory
    #[error("Invalid certificate directory: {0}")]
    InvalidDirectory(String),
}

/// Certificate paths
#[derive(Debug, Clone)]
pub struct CertificatePaths {
    /// CA certificate path
    pub ca_cert: PathBuf,
    /// CA private key path
    pub ca_key: PathBuf,
    /// Server certificate path
    pub server_cert: PathBuf,
    /// Server private key path
    pub server_key: PathBuf,
}

impl CertificatePaths {
    /// Create certificate paths from directory
    pub fn new(cert_dir: &Path) -> Self {
        Self {
            ca_cert: cert_dir.join("ca.crt"),
            ca_key: cert_dir.join("ca.key"),
            server_cert: cert_dir.join("server.crt"),
            server_key: cert_dir.join("server.key"),
        }
    }

    /// Check if all certificates exist
    pub fn all_exist(&self) -> bool {
        self.ca_cert.exists()
            && self.ca_key.exists()
            && self.server_cert.exists()
            && self.server_key.exists()
    }
}

/// Certificate generator
#[derive(Debug)]
pub struct CertificateGenerator {
    cert_dir: PathBuf,
}

impl CertificateGenerator {
    /// Create a new certificate generator
    ///
    /// ## Arguments
    /// * `cert_dir` - Directory to store certificates
    ///
    /// ## Returns
    /// CertificateGenerator or error if directory cannot be created
    pub fn new(cert_dir: impl AsRef<Path>) -> Result<Self, CertGenError> {
        let cert_dir = cert_dir.as_ref().to_path_buf();
        
        // Create directory if it doesn't exist
        if !cert_dir.exists() {
            fs::create_dir_all(&cert_dir)?;
        }

        // Verify directory is writable
        if !cert_dir.is_dir() {
            return Err(CertGenError::InvalidDirectory(
                format!("Not a directory: {}", cert_dir.display())
            ));
        }

        Ok(Self { cert_dir })
    }

    /// Generate CA certificate and key
    ///
    /// ## Arguments
    /// * `common_name` - Common name for CA (default: "PlexSpaces CA")
    /// * `validity_days` - Validity in days (default: 365)
    ///
    /// ## Returns
    /// Paths to generated certificates
    ///
    /// ## Note
    /// This is a placeholder implementation. Full implementation will use rcgen
    /// to generate proper X.509 certificates. For now, it creates placeholder files.
    pub fn generate_ca(
        &self,
        common_name: Option<&str>,
        validity_days: Option<u32>,
    ) -> Result<CertificatePaths, CertGenError> {
        let paths = CertificatePaths::new(&self.cert_dir);
        
        // If certificates already exist, return paths
        if paths.all_exist() {
            return Ok(paths);
        }

        let cn = common_name.unwrap_or("PlexSpaces CA");
        let _validity = validity_days.unwrap_or(365);

        // TODO: Implement full certificate generation using rcgen
        // For now, create placeholder files for testing
        let ca_cert_pem = format!(
            "-----BEGIN CERTIFICATE-----\n\
            Placeholder CA Certificate for: {}\n\
            Validity: {} days\n\
            -----END CERTIFICATE-----",
            cn, _validity
        );
        
        let ca_key_pem = format!(
            "-----BEGIN PRIVATE KEY-----\n\
            Placeholder CA Private Key for: {}\n\
            -----END PRIVATE KEY-----",
            cn
        );
        
        // Write CA certificate
        fs::write(&paths.ca_cert, ca_cert_pem.as_bytes())?;
        
        // Write CA key
        fs::write(&paths.ca_key, ca_key_pem.as_bytes())?;

        Ok(paths)
    }

    /// Generate server certificate signed by CA
    ///
    /// ## Arguments
    /// * `common_name` - Common name for server (e.g., service ID)
    /// * `san_dns_names` - Subject Alternative Names (DNS names)
    /// * `validity_days` - Validity in days (default: 90)
    ///
    /// ## Returns
    /// Paths to generated certificates
    ///
    /// ## Note
    /// This is a placeholder implementation. Full implementation will use rcgen
    /// to generate proper X.509 certificates signed by the CA.
    pub fn generate_server_cert(
        &self,
        common_name: &str,
        san_dns_names: Vec<String>,
        validity_days: Option<u32>,
    ) -> Result<CertificatePaths, CertGenError> {
        let paths = CertificatePaths::new(&self.cert_dir);
        
        // Ensure CA exists first
        if !paths.ca_cert.exists() || !paths.ca_key.exists() {
            self.generate_ca(None, None)?;
        }

        let _validity = validity_days.unwrap_or(90);
        let san_list = san_dns_names.join(", ");

        // TODO: Implement full certificate generation using rcgen
        // For now, create placeholder files for testing
        let server_cert_pem = format!(
            "-----BEGIN CERTIFICATE-----\n\
            Placeholder Server Certificate for: {}\n\
            SAN: {}\n\
            Validity: {} days\n\
            -----END CERTIFICATE-----",
            common_name, san_list, _validity
        );
        
        let server_key_pem = format!(
            "-----BEGIN PRIVATE KEY-----\n\
            Placeholder Server Private Key for: {}\n\
            -----END PRIVATE KEY-----",
            common_name
        );

        // Write server certificate
        fs::write(&paths.server_cert, server_cert_pem.as_bytes())?;
        
        // Write server key
        fs::write(&paths.server_key, server_key_pem.as_bytes())?;

        Ok(paths)
    }

    /// Generate all certificates (CA + server)
    ///
    /// ## Arguments
    /// * `service_id` - Service ID for server certificate CN
    /// * `san_dns_names` - Subject Alternative Names
    ///
    /// ## Returns
    /// Paths to generated certificates
    pub fn generate_all(
        &self,
        service_id: &str,
        san_dns_names: Vec<String>,
    ) -> Result<CertificatePaths, CertGenError> {
        // Generate CA first
        self.generate_ca(None, None)?;
        
        // Generate server certificate
        self.generate_server_cert(service_id, san_dns_names, None)?;

        Ok(CertificatePaths::new(&self.cert_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_certificate_paths_new() {
        let dir = Path::new("/tmp/certs");
        let paths = CertificatePaths::new(dir);
        
        assert_eq!(paths.ca_cert, dir.join("ca.crt"));
        assert_eq!(paths.ca_key, dir.join("ca.key"));
        assert_eq!(paths.server_cert, dir.join("server.crt"));
        assert_eq!(paths.server_key, dir.join("server.key"));
    }

    #[test]
    fn test_certificate_paths_all_exist() {
        let temp_dir = TempDir::new().unwrap();
        let paths = CertificatePaths::new(temp_dir.path());
        
        // Initially, files don't exist
        assert!(!paths.all_exist());
        
        // Create all files
        fs::write(&paths.ca_cert, "ca cert").unwrap();
        fs::write(&paths.ca_key, "ca key").unwrap();
        fs::write(&paths.server_cert, "server cert").unwrap();
        fs::write(&paths.server_key, "server key").unwrap();
        
        // Now all should exist
        assert!(paths.all_exist());
    }

    #[test]
    fn test_certificate_generator_new() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path());
        
        assert!(gen.is_ok());
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn test_certificate_generator_new_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("certs");
        
        let gen = CertificateGenerator::new(&subdir);
        
        assert!(gen.is_ok());
        assert!(subdir.exists());
        assert!(subdir.is_dir());
    }

    #[test]
    fn test_certificate_generator_new_invalid_path() {
        // Try to use a file as directory (should fail)
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "test").unwrap();
        
        let gen = CertificateGenerator::new(&file_path);
        assert!(gen.is_err());
        assert!(gen.unwrap_err().to_string().contains("Not a directory"));
    }

    #[test]
    fn test_generate_ca() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        
        let paths = gen.generate_ca(None, None).unwrap();
        
        // Check files were created
        assert!(paths.ca_cert.exists());
        assert!(paths.ca_key.exists());
        let cert_content = fs::read_to_string(&paths.ca_cert).unwrap();
        assert!(cert_content.contains("BEGIN CERTIFICATE"));
        assert!(cert_content.contains("PlexSpaces CA"));
    }

    #[test]
    fn test_generate_ca_with_custom_name() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        
        let paths = gen.generate_ca(Some("My Custom CA"), Some(180)).unwrap();
        
        assert!(paths.ca_cert.exists());
        assert!(paths.ca_key.exists());
        let cert_content = fs::read_to_string(&paths.ca_cert).unwrap();
        assert!(cert_content.contains("My Custom CA"));
    }

    #[test]
    fn test_generate_ca_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        
        // Generate twice
        let paths1 = gen.generate_ca(None, None).unwrap();
        let paths2 = gen.generate_ca(None, None).unwrap();
        
        // Should return same paths
        assert_eq!(paths1.ca_cert, paths2.ca_cert);
        assert_eq!(paths1.ca_key, paths2.ca_key);
    }

    #[test]
    fn test_generate_server_cert() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        
        // Generate CA first
        gen.generate_ca(None, None).unwrap();
        
        let paths = gen.generate_server_cert(
            "test-service",
            vec!["test-service.local".to_string(), "localhost".to_string()],
            None,
        ).unwrap();
        
        // Check all files were created
        assert!(paths.all_exist());
        let cert_content = fs::read_to_string(&paths.server_cert).unwrap();
        assert!(cert_content.contains("BEGIN CERTIFICATE"));
        assert!(cert_content.contains("test-service"));
    }

    #[test]
    fn test_generate_all() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        
        let paths = gen.generate_all(
            "test-service",
            vec!["test-service.local".to_string(), "localhost".to_string()],
        ).unwrap();
        
        // Check all files were created
        assert!(paths.all_exist());
        assert!(fs::read_to_string(&paths.server_cert).unwrap().contains("BEGIN CERTIFICATE"));
        assert!(fs::read_to_string(&paths.server_key).unwrap().contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_generate_server_cert_creates_ca_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        
        // Generate server cert without CA (should create CA first)
        let paths = gen.generate_server_cert(
            "test-service",
            vec![],
            None,
        ).unwrap();
        
        // Both CA and server certs should exist
        assert!(paths.all_exist());
    }
}
