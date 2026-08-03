// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! ES256 JWT Authentication Integration Tests
//!
//! Tests the full ES256 authentication lifecycle:
//! - Key generation and loading (PKCS#8 and SEC1 formats)
//! - Token signing with ES256 key pair
//! - Token verification with public key
//! - Token rejection scenarios (expired, wrong key, tampered)
//! - from_config resolution priority
//! - load_or_generate file-based workflow
//! - JWKS endpoint data generation

use plexspaces_grpc_middleware::jwt::{
    sign_jwt_with_keypair, validate_bearer_token_with_keypair, validate_jwt_token_with_keypair,
    JwtClaims,
};
use plexspaces_grpc_middleware::jwt_keys::JwtKeyPair;
use std::path::Path;
use tempfile::TempDir;

fn test_claims(tenant_id: &str, expires_in_secs: i64) -> JwtClaims {
    let now = chrono::Utc::now().timestamp();
    JwtClaims {
        sub: "test-user-123".to_string(),
        exp: now + expires_in_secs,
        iat: now,
        iss: "plexspaces-test".to_string(),
        aud: vec!["plexspaces-api".to_string()],
        tenant_id: tenant_id.to_string(),
        roles: vec!["user".to_string(), "admin".to_string()],
        groups: vec!["engineering".to_string()],
        is_admin: true,
        jti: None,
    }
}

#[test]
fn test_es256_generate_sign_verify_roundtrip() {
    let kp = JwtKeyPair::generate_es256().expect("key generation should succeed");

    assert!(kp.is_asymmetric());
    assert_eq!(kp.algorithm(), jsonwebtoken::Algorithm::ES256);
    assert!(kp.public_key_pem().is_some());
    assert!(!kp.kid().is_empty());

    let claims = test_claims("tenant-abc", 3600);
    let token = sign_jwt_with_keypair(&kp, &claims).expect("signing should succeed");

    let verified =
        validate_jwt_token_with_keypair(&kp, &token).expect("verification should succeed");
    assert_eq!(verified.sub, "test-user-123");
    assert_eq!(verified.tenant_id, "tenant-abc");
    assert_eq!(verified.roles, vec!["user", "admin"]);
    assert!(verified.is_admin);
}

#[test]
fn test_es256_validate_bearer_token_roundtrip() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let claims = test_claims("my-tenant", 3600);
    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();
    let auth_header = format!("Bearer {}", token);

    let verified =
        validate_bearer_token_with_keypair(&kp, Some(&auth_header)).expect("should verify");
    assert_eq!(verified.tenant_id, "my-tenant");
    assert_eq!(verified.sub, "test-user-123");
}

#[test]
fn test_es256_rejects_expired_token() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let claims = test_claims("t1", -3600); // expired 1 hour ago
    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();

    let result = validate_jwt_token_with_keypair(&kp, &token);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("JWT validation failed"));
}

#[test]
fn test_es256_rejects_wrong_key() {
    let kp1 = JwtKeyPair::generate_es256().unwrap();
    let kp2 = JwtKeyPair::generate_es256().unwrap();

    let claims = test_claims("t1", 3600);
    let token = sign_jwt_with_keypair(&kp1, &claims).unwrap();

    // Verify with a different key should fail
    let result = validate_jwt_token_with_keypair(&kp2, &token);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("JWT validation failed"));
}

#[test]
fn test_es256_rejects_tampered_token() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let claims = test_claims("t1", 3600);
    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();

    // Tamper with the payload (flip a character)
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let mut payload_bytes = parts[1].as_bytes().to_vec();
    if let Some(b) = payload_bytes.get_mut(5) {
        *b = if *b == b'A' { b'B' } else { b'A' };
    }
    let tampered = format!(
        "{}.{}.{}",
        parts[0],
        String::from_utf8_lossy(&payload_bytes),
        parts[2]
    );

    let result = validate_jwt_token_with_keypair(&kp, &tampered);
    assert!(result.is_err());
}

#[test]
fn test_es256_rejects_missing_tenant_id() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let claims = test_claims("", 3600); // empty tenant_id

    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();
    let result = validate_jwt_token_with_keypair(&kp, &token);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("tenant_id"));
}

#[test]
fn test_es256_load_or_generate_creates_new_file() {
    let tmp = TempDir::new().unwrap();
    let key_path = tmp.path().join("jwt-es256.pem");

    assert!(!key_path.exists());

    let kp = JwtKeyPair::load_or_generate(&key_path).expect("should generate and save");
    assert!(key_path.exists());
    assert!(kp.is_asymmetric());

    // Token signed with generated key should verify
    let claims = test_claims("t1", 3600);
    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();
    let verified = validate_jwt_token_with_keypair(&kp, &token).unwrap();
    assert_eq!(verified.tenant_id, "t1");
}

#[test]
fn test_es256_load_or_generate_reloads_existing_key() {
    let tmp = TempDir::new().unwrap();
    let key_path = tmp.path().join("jwt-es256.pem");

    // Generate the first time
    let kp1 = JwtKeyPair::load_or_generate(&key_path).unwrap();
    let kid1 = kp1.kid().to_string();

    // Load the second time — should get the same key (same kid)
    let kp2 = JwtKeyPair::load_or_generate(&key_path).unwrap();
    assert_eq!(kp2.kid(), kid1);

    // Token signed with kp1 should verify with kp2 (same key)
    let claims = test_claims("t1", 3600);
    let token = sign_jwt_with_keypair(&kp1, &claims).unwrap();
    let verified = validate_jwt_token_with_keypair(&kp2, &token).unwrap();
    assert_eq!(verified.tenant_id, "t1");
}

#[test]
fn test_es256_from_config_file_path() {
    let tmp = TempDir::new().unwrap();
    let key_path = tmp.path().join("jwt-es256.pem");

    // Generate key first so it exists
    JwtKeyPair::load_or_generate(&key_path).unwrap();

    let kp = JwtKeyPair::from_config(
        "",                         // no inline PEM
        key_path.to_str().unwrap(), // file path
        "",                         // no secret
        false,                      // no auto-generate
    )
    .expect("from_config with file path should work");

    assert!(kp.is_asymmetric());

    let claims = test_claims("config-tenant", 3600);
    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();
    let verified = validate_jwt_token_with_keypair(&kp, &token).unwrap();
    assert_eq!(verified.tenant_id, "config-tenant");
}

#[test]
fn test_es256_from_config_inline_pem_takes_priority() {
    let tmp = TempDir::new().unwrap();
    let key_path = tmp.path().join("jwt-es256-file.pem");
    let inline_path = tmp.path().join("jwt-es256-inline.pem");

    // Generate two different key files
    let file_kp = JwtKeyPair::load_or_generate(&key_path).unwrap();
    let inline_kp = JwtKeyPair::load_or_generate(&inline_path).unwrap();
    let inline_pem = std::fs::read_to_string(&inline_path).unwrap();

    // from_config with both: inline PEM should win over file path
    let resolved = JwtKeyPair::from_config(
        &inline_pem,
        key_path.to_str().unwrap(),
        "some-hs256-secret",
        true,
    )
    .unwrap();

    // Should use the inline key (same kid as inline_kp, different from file)
    assert_eq!(resolved.kid(), inline_kp.kid());
    assert_ne!(resolved.kid(), file_kp.kid());
}

#[test]
fn test_es256_from_config_rejects_hs256_secret() {
    let result = JwtKeyPair::from_config("", "", "my-hs256-secret", false);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("HS256 is not supported"));
}

#[test]
fn test_es256_from_config_auto_generate_when_nothing_configured() {
    let kp = JwtKeyPair::from_config("", "", "", true).unwrap();

    assert!(kp.is_asymmetric());
    assert_eq!(kp.algorithm(), jsonwebtoken::Algorithm::ES256);
}

#[test]
fn test_es256_from_config_fails_when_nothing_configured_and_no_auto_generate() {
    let result = JwtKeyPair::from_config("", "", "", false);
    assert!(result.is_err());
}

#[test]
fn test_es256_sec1_format_key_loads_correctly() {
    // Test that SEC1 format (BEGIN EC PRIVATE KEY) works via from_ec_pem
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ec-sec1-test.pem");

    if fixture_path.exists() {
        let pem = std::fs::read_to_string(&fixture_path).unwrap();
        let kp = JwtKeyPair::from_ec_pem(&pem).expect("SEC1 key should load");
        assert!(kp.is_asymmetric());

        let claims = test_claims("sec1-tenant", 3600);
        let token = sign_jwt_with_keypair(&kp, &claims).unwrap();
        let verified = validate_jwt_token_with_keypair(&kp, &token).unwrap();
        assert_eq!(verified.tenant_id, "sec1-tenant");
    }
}

#[test]
fn test_es256_jwks_json_has_valid_structure() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let jwks = kp.jwks_json();

    let keys = jwks.get("keys").unwrap().as_array().unwrap();
    assert_eq!(keys.len(), 1);

    let key = &keys[0];
    assert_eq!(key.get("kty").unwrap().as_str().unwrap(), "EC");
    assert_eq!(key.get("crv").unwrap().as_str().unwrap(), "P-256");
    assert_eq!(key.get("use").unwrap().as_str().unwrap(), "sig");
    assert_eq!(key.get("alg").unwrap().as_str().unwrap(), "ES256");
    assert_eq!(key.get("kid").unwrap().as_str().unwrap(), kp.kid());
    assert!(key.get("x").is_some());
    assert!(key.get("y").is_some());
}

#[test]
fn test_es256_two_nodes_same_key_file_cross_verify() {
    // Simulates two server nodes sharing the same key file (the 8091/8094 scenario)
    let tmp = TempDir::new().unwrap();
    let shared_key_path = tmp.path().join("shared-jwt-es256.pem");

    // Node 1 starts first, generates the key
    let node1_kp = JwtKeyPair::load_or_generate(&shared_key_path).unwrap();

    // Node 2 starts second, loads the same key file
    let node2_kp = JwtKeyPair::load_or_generate(&shared_key_path).unwrap();

    // Both should have the same kid (same key)
    assert_eq!(node1_kp.kid(), node2_kp.kid());

    // Token signed by node 1 should verify on node 2
    let claims = test_claims("multi-node-tenant", 3600);
    let token = sign_jwt_with_keypair(&node1_kp, &claims).unwrap();
    let verified = validate_jwt_token_with_keypair(&node2_kp, &token).unwrap();
    assert_eq!(verified.tenant_id, "multi-node-tenant");

    // Token signed by node 2 should verify on node 1
    let claims2 = test_claims("another-tenant", 3600);
    let token2 = sign_jwt_with_keypair(&node2_kp, &claims2).unwrap();
    let verified2 = validate_jwt_token_with_keypair(&node1_kp, &token2).unwrap();
    assert_eq!(verified2.tenant_id, "another-tenant");
}

#[test]
fn test_es256_rejects_token_signed_with_different_algorithm() {
    let es256_kp = JwtKeyPair::generate_es256().unwrap();

    // Manually create an HS256-signed token (simulating a forged token)
    let now = chrono::Utc::now().timestamp();
    #[derive(serde::Serialize)]
    struct FakeClaims {
        sub: String,
        exp: i64,
        iat: i64,
        tenant_id: String,
    }
    let claims = FakeClaims {
        sub: "attacker".to_string(),
        exp: now + 3600,
        iat: now,
        tenant_id: "t1".to_string(),
    };
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    let key = jsonwebtoken::EncodingKey::from_secret(b"secret");
    let hs256_token = jsonwebtoken::encode(&header, &claims, &key).unwrap();

    // ES256 key should reject HS256 token
    let result = validate_jwt_token_with_keypair(&es256_kp, &hs256_token);
    assert!(result.is_err());
}

#[test]
fn test_es256_bearer_missing_header_returns_error() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let result = validate_bearer_token_with_keypair(&kp, None);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("Missing or invalid Authorization"));
}

#[test]
fn test_es256_bearer_no_bearer_prefix_returns_error() {
    let kp = JwtKeyPair::generate_es256().unwrap();
    let claims = test_claims("t1", 3600);
    let token = sign_jwt_with_keypair(&kp, &claims).unwrap();

    // Pass token without "Bearer " prefix
    let result = validate_bearer_token_with_keypair(&kp, Some(&token));
    assert!(result.is_err());
}

#[test]
fn test_es256_aud_as_string_accepted() {
    // RFC 7519: "aud" can be either a string or array. gen-test-jwt.sh emits a string.
    let kp = JwtKeyPair::generate_es256().unwrap();

    // Manually build a token with "aud" as a single string (not array)
    #[derive(serde::Serialize)]
    struct ClaimsWithStringAud {
        sub: String,
        exp: i64,
        iat: i64,
        iss: String,
        aud: String, // string, not Vec
        tenant_id: String,
        roles: Vec<String>,
        groups: Vec<String>,
        is_admin: bool,
    }
    let now = chrono::Utc::now().timestamp();
    let claims = ClaimsWithStringAud {
        sub: "test-user".to_string(),
        exp: now + 3600,
        iat: now,
        iss: "plexspaces".to_string(),
        aud: "plexspaces-api".to_string(),
        tenant_id: "my-tenant".to_string(),
        roles: vec!["user".to_string()],
        groups: vec![],
        is_admin: false,
    };
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some(kp.kid().to_string());
    let token = jsonwebtoken::encode(&header, &claims, kp.encoding_key()).unwrap();

    // Verify it decodes correctly
    let verified =
        validate_jwt_token_with_keypair(&kp, &token).expect("string aud should be accepted");
    assert_eq!(verified.tenant_id, "my-tenant");
    assert_eq!(verified.aud, vec!["plexspaces-api"]);
}

#[test]
fn test_es256_aud_as_empty_string_accepted() {
    let kp = JwtKeyPair::generate_es256().unwrap();

    #[derive(serde::Serialize)]
    struct ClaimsWithEmptyAud {
        sub: String,
        exp: i64,
        iat: i64,
        tenant_id: String,
        aud: String,
    }
    let now = chrono::Utc::now().timestamp();
    let claims = ClaimsWithEmptyAud {
        sub: "u".to_string(),
        exp: now + 3600,
        iat: now,
        tenant_id: "t1".to_string(),
        aud: "".to_string(),
    };
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some(kp.kid().to_string());
    let token = jsonwebtoken::encode(&header, &claims, kp.encoding_key()).unwrap();

    let verified =
        validate_jwt_token_with_keypair(&kp, &token).expect("empty aud string should work");
    assert!(verified.aud.is_empty());
}

#[cfg(unix)]
#[test]
fn test_es256_key_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let key_path = tmp.path().join("jwt-es256.pem");

    JwtKeyPair::load_or_generate(&key_path).unwrap();

    let meta = std::fs::metadata(&key_path).unwrap();
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "key file should have 0600 permissions, got {:o}",
        mode
    );
}
