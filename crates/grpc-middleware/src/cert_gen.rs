// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Certificate generation for mTLS node-to-node communication.
//!
//! ## Purpose
//! Generates CA and node certificates using `rcgen` for self-signed mTLS.
//! Intended for dev/test; production should use cert-manager or Vault PKI.
//!
//! ## Security Note
//! Auto-generated certificates are **not** for production use.
//! Rotate them or replace with a proper CA before deploying outside dev.

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Certificate generation errors.
#[derive(Debug, Error)]
pub enum CertGenError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Certificate generation failed: {0}")]
    GenerationFailed(String),
    #[error("Invalid certificate directory: {0}")]
    InvalidDirectory(String),
}

/// Paths for a generated cert set (CA + server/node).
#[derive(Debug, Clone)]
pub struct CertificatePaths {
    pub ca_cert: PathBuf,
    pub ca_key: PathBuf,
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
}

impl CertificatePaths {
    pub fn new(cert_dir: &Path) -> Self {
        Self {
            ca_cert: cert_dir.join("ca.crt"),
            ca_key: cert_dir.join("ca.key"),
            server_cert: cert_dir.join("server.crt"),
            server_key: cert_dir.join("server.key"),
        }
    }

    pub fn all_exist(&self) -> bool {
        self.ca_cert.exists()
            && self.ca_key.exists()
            && self.server_cert.exists()
            && self.server_key.exists()
    }
}

/// Generates mTLS certificates using `rcgen`.
#[derive(Debug)]
pub struct CertificateGenerator {
    cert_dir: PathBuf,
}

impl CertificateGenerator {
    pub fn new(cert_dir: impl AsRef<Path>) -> Result<Self, CertGenError> {
        let cert_dir = cert_dir.as_ref().to_path_buf();
        if !cert_dir.exists() {
            fs::create_dir_all(&cert_dir)?;
        }
        if !cert_dir.is_dir() {
            return Err(CertGenError::InvalidDirectory(format!(
                "Not a directory: {}",
                cert_dir.display()
            )));
        }
        Ok(Self { cert_dir })
    }

    /// Generate a self-signed CA certificate + key, writing PEM files to `cert_dir`.
    ///
    /// Idempotent: returns existing paths without regenerating if all CA files are present.
    pub fn generate_ca(
        &self,
        common_name: Option<&str>,
        validity_days: Option<u32>,
    ) -> Result<CertificatePaths, CertGenError> {
        let paths = CertificatePaths::new(&self.cert_dir);

        // Idempotent: skip if CA files already exist.
        if paths.ca_cert.exists() && paths.ca_key.exists() {
            return Ok(paths);
        }

        let cn = common_name.unwrap_or("PlexSpaces CA");
        let days = validity_days.unwrap_or(365) as i64;
        let (cert_pem, key_pem) = Self::make_ca_pem(cn, days)?;

        fs::write(&paths.ca_cert, cert_pem)?;
        fs::write(&paths.ca_key, key_pem)?;
        Ok(paths)
    }

    /// Generate a server/node certificate.
    ///
    /// In dev mode this is a freshly self-signed cert (the CA and server cert are generated
    /// together so the CA can sign the server cert without reloading PEM).
    /// If CA PEM already exists on disk it is overwritten so the CA–cert chain remains valid.
    pub fn generate_server_cert(
        &self,
        common_name: &str,
        san_dns_names: Vec<String>,
        validity_days: Option<u32>,
    ) -> Result<CertificatePaths, CertGenError> {
        // Always regenerate both CA and server cert together so the CA can sign the server.
        let paths = CertificatePaths::new(&self.cert_dir);
        let days = validity_days.unwrap_or(90) as i64;
        let (ca_cert_pem, ca_key_pem, server_cert_pem, server_key_pem) =
            Self::make_ca_and_server_pem(common_name, san_dns_names, days)?;

        fs::write(&paths.ca_cert, ca_cert_pem)?;
        fs::write(&paths.ca_key, ca_key_pem)?;
        fs::write(&paths.server_cert, server_cert_pem)?;
        fs::write(&paths.server_key, server_key_pem)?;
        Ok(paths)
    }

    /// Generate CA + server cert in one shot (most common entry point).
    ///
    /// Idempotent: if all four files already exist, returns their paths unchanged.
    pub fn generate_all(
        &self,
        service_id: &str,
        san_dns_names: Vec<String>,
    ) -> Result<CertificatePaths, CertGenError> {
        let paths = CertificatePaths::new(&self.cert_dir);
        if paths.all_exist() {
            return Ok(paths);
        }
        self.generate_server_cert(service_id, san_dns_names, None)
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    fn make_ca_pem(cn: &str, days: i64) -> Result<(String, String), CertGenError> {
        let key = KeyPair::generate()
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;

        let mut params = CertificateParams::new(vec![])
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn);
        params.distinguished_name = dn;
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::days(days);

        let cert = params
            .self_signed(&key)
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;
        Ok((cert.pem(), key.serialize_pem()))
    }

    fn make_ca_and_server_pem(
        server_cn: &str,
        san_dns_names: Vec<String>,
        server_days: i64,
    ) -> Result<(String, String, String, String), CertGenError> {
        // Generate CA key + cert.
        let ca_key = KeyPair::generate()
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;
        let mut ca_params = CertificateParams::new(vec![])
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;
        {
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, "PlexSpaces CA");
            ca_params.distinguished_name = dn;
        }
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.not_before = time::OffsetDateTime::now_utc();
        ca_params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::days(365);
        let ca_cert = ca_params
            .self_signed(&ca_key)
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;

        // Build SANs — always include localhost.
        let mut sans: Vec<SanType> = san_dns_names
            .iter()
            .map(|n| SanType::DnsName(n.clone().try_into().expect("SAN")))
            .collect();
        if !san_dns_names.iter().any(|n| n == "localhost") {
            sans.push(SanType::DnsName("localhost".try_into().expect("SAN")));
        }

        // Generate server key + cert signed by CA.
        let server_key = KeyPair::generate()
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;
        let mut server_params = CertificateParams::new(vec![])
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;
        {
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, server_cn);
            server_params.distinguished_name = dn;
        }
        server_params.subject_alt_names = sans;
        server_params.not_before = time::OffsetDateTime::now_utc();
        server_params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::days(server_days);
        let server_cert = server_params
            .signed_by(&server_key, &ca_cert, &ca_key)
            .map_err(|e| CertGenError::GenerationFailed(e.to_string()))?;

        Ok((
            ca_cert.pem(),
            ca_key.serialize_pem(),
            server_cert.pem(),
            server_key.serialize_pem(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_certificate_paths_new() {
        let dir = Path::new("/tmp/certs");
        let paths = CertificatePaths::new(dir);
        assert_eq!(paths.ca_cert, dir.join("ca.crt"));
        assert_eq!(paths.server_key, dir.join("server.key"));
    }

    #[test]
    fn test_certificate_paths_all_exist() {
        let temp_dir = TempDir::new().unwrap();
        let paths = CertificatePaths::new(temp_dir.path());
        assert!(!paths.all_exist());
        fs::write(&paths.ca_cert, "x").unwrap();
        fs::write(&paths.ca_key, "x").unwrap();
        fs::write(&paths.server_cert, "x").unwrap();
        fs::write(&paths.server_key, "x").unwrap();
        assert!(paths.all_exist());
    }

    #[test]
    fn test_certificate_generator_new() {
        let temp_dir = TempDir::new().unwrap();
        assert!(CertificateGenerator::new(temp_dir.path()).is_ok());
    }

    #[test]
    fn test_certificate_generator_new_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("certs");
        assert!(CertificateGenerator::new(&subdir).is_ok());
        assert!(subdir.is_dir());
    }

    #[test]
    fn test_certificate_generator_new_invalid_path() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "test").unwrap();
        let result = CertificateGenerator::new(&file_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not a directory"));
    }

    #[test]
    fn test_generate_ca_produces_valid_pem() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        let paths = gen.generate_ca(Some("Test CA"), Some(30)).unwrap();
        assert!(paths.ca_cert.exists());
        assert!(paths.ca_key.exists());
        let pem = fs::read_to_string(&paths.ca_cert).unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"), "CA cert should be PEM");
    }

    #[test]
    fn test_generate_ca_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        gen.generate_ca(None, None).unwrap();
        let content1 = fs::read_to_string(temp_dir.path().join("ca.crt")).unwrap();
        gen.generate_ca(None, None).unwrap();
        let content2 = fs::read_to_string(temp_dir.path().join("ca.crt")).unwrap();
        assert_eq!(content1, content2, "idempotent: should not overwrite existing certs");
    }

    #[test]
    fn test_generate_server_cert_produces_valid_pem() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        let paths = gen
            .generate_server_cert("node-1", vec!["node-1.local".to_string()], Some(30))
            .unwrap();
        assert!(paths.all_exist());
        let pem = fs::read_to_string(&paths.server_cert).unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_generate_all() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        let paths = gen
            .generate_all("node-1", vec!["node-1.local".to_string()])
            .unwrap();
        assert!(paths.all_exist());
    }

    #[test]
    fn test_generate_all_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        let paths1 = gen.generate_all("node-1", vec![]).unwrap();
        let cert1 = fs::read_to_string(&paths1.server_cert).unwrap();
        let paths2 = gen.generate_all("node-1", vec![]).unwrap();
        let cert2 = fs::read_to_string(&paths2.server_cert).unwrap();
        assert_eq!(cert1, cert2);
    }

    #[test]
    fn test_generate_server_cert_creates_ca() {
        let temp_dir = TempDir::new().unwrap();
        let gen = CertificateGenerator::new(temp_dir.path()).unwrap();
        let paths = gen.generate_server_cert("svc", vec![], None).unwrap();
        assert!(paths.all_exist());
        let ca_pem = fs::read_to_string(&paths.ca_cert).unwrap();
        assert!(ca_pem.contains("BEGIN CERTIFICATE"));
    }
}
