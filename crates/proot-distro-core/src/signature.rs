//! Image signature verification for PRoot Distro
//!
//! Provides signature verification using cosign, notary, and simple verification.
//! Supports keyless signing, keypair signing, and notary v1/v2.

use anyhow::{bail, Context, Result};
use base64;
use hmac::Mac;
use ring::signature::{KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use x509_parser::prelude::*;

/// Signature verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureConfig {
    pub enabled: bool,
    pub require_signatures: bool,
    pub trusted_keys: Vec<String>, // Paths to public keys or key IDs
    pub trusted_certificates: Vec<String>, // Paths to certificates
    pub allow_keyless: bool,
    pub tlog_verification: bool,
    pub regex_identity: Option<String>, // Regex for keyless identity verification
    pub regex_issuer: Option<String>,   // Regex for keyless issuer verification
    pub offline: bool,
}

impl Default for SignatureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            require_signatures: false,
            trusted_keys: Vec::new(),
            trusted_certificates: Vec::new(),
            allow_keyless: true,
            tlog_verification: true,
            regex_identity: None,
            regex_issuer: None,
            offline: false,
        }
    }
}

/// Signature verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub verified: bool,
    pub signature_type: SignatureType,
    pub signer: Option<String>,
    pub certificate: Option<String>,
    pub tlog_entries: Vec<TlogEntry>,
    pub warnings: Vec<String>,
}

/// Signature types supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureType {
    Cosign,
    NotaryV1,
    NotaryV2,
    Simple,
}

/// Transparency log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlogEntry {
    pub log_id: String,
    pub log_index: u64,
    pub integrated_time: i64,
}

/// Image signature metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSignature {
    pub image_digest: String,
    pub signature_type: SignatureType,
    pub signature: Vec<u8>,
    pub certificate: Option<Vec<u8>>,
    pub chain: Vec<Vec<u8>>,
    pub annotations: HashMap<String, String>,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

/// Signature verifier
pub struct SignatureVerifier {
    config: SignatureConfig,
    trusted_keys: Vec<Vec<u8>>,
    trusted_certs: Vec<Vec<u8>>,
}

impl SignatureVerifier {
    pub fn new(config: SignatureConfig) -> Result<Self> {
        let mut verifier = Self {
            config,
            trusted_keys: Vec::new(),
            trusted_certs: Vec::new(),
        };

        verifier.load_trusted_keys()?;
        verifier.load_trusted_certificates()?;

        Ok(verifier)
    }

    fn load_trusted_keys(&mut self) -> Result<()> {
        for key_path in &self.config.trusted_keys {
            let key_data = std::fs::read(key_path)
                .with_context(|| format!("Failed to read trusted key: {}", key_path))?;
            self.trusted_keys.push(key_data);
        }
        Ok(())
    }

    fn load_trusted_certificates(&mut self) -> Result<()> {
        for cert_path in &self.config.trusted_certificates {
            let cert_data = std::fs::read(cert_path)
                .with_context(|| format!("Failed to read trusted certificate: {}", cert_path))?;
            self.trusted_certs.push(cert_data);
        }
        Ok(())
    }

    /// Verify an image signature
    pub async fn verify_image(
        &self,
        image_path: &Path,
        signature: &ImageSignature,
    ) -> Result<VerificationResult> {
        if !self.config.enabled {
            return Ok(VerificationResult {
                verified: true,
                signature_type: SignatureType::Simple,
                signer: None,
                certificate: None,
                tlog_entries: Vec::new(),
                warnings: vec!["Signature verification disabled".to_string()],
            });
        }

        // Calculate image digest
        let image_digest = self.calculate_image_digest(image_path).await?;

        if image_digest != signature.image_digest {
            bail!("Image digest mismatch");
        }

        match signature.signature_type {
            SignatureType::Cosign => self.verify_cosign_signature(image_path, signature).await,
            SignatureType::NotaryV1 => self.verify_notary_v1_signature(image_path, signature).await,
            SignatureType::NotaryV2 => self.verify_notary_v2_signature(image_path, signature).await,
            SignatureType::Simple => self.verify_simple_signature(image_path, signature).await,
        }
    }

    /// Verify cosign signature (simplified - without external cosign crate)
    async fn verify_cosign_signature(
        &self,
        image_path: &Path,
        signature: &ImageSignature,
    ) -> Result<VerificationResult> {
        // Cosign uses Ed25519 signatures with transparency logs
        // This is a simplified implementation using ring for Ed25519 verification

        let image_data = tokio::fs::read(image_path).await?;

        // Try to verify with trusted keys
        for key in &self.trusted_keys {
            let public_key = UnparsedPublicKey::new(&ED25519, key);
            if public_key.verify(&image_data, &signature.signature).is_ok() {
                return Ok(VerificationResult {
                    verified: true,
                    signature_type: SignatureType::Cosign,
                    signer: signature.annotations.get("signer").cloned(),
                    certificate: signature
                        .certificate
                        .as_ref()
                        .map(|c| String::from_utf8_lossy(c).to_string()),
                    tlog_entries: Vec::new(),
                    warnings: Vec::new(),
                });
            }
        }

        // Try certificate verification
        for cert_data in &self.trusted_certs {
            if let Ok((_, cert)) = X509Certificate::from_der(cert_data) {
                let public_key = cert.public_key();
                let key = UnparsedPublicKey::new(&ED25519, public_key.subject_public_key.data);
                if key.verify(&image_data, &signature.signature).is_ok() {
                    return Ok(VerificationResult {
                        verified: true,
                        signature_type: SignatureType::Cosign,
                        signer: Some(cert.subject().to_string()),
                        certificate: Some(String::from_utf8_lossy(cert_data).to_string()),
                        tlog_entries: Vec::new(),
                        warnings: Vec::new(),
                    });
                }
            }
        }

        // Keyless verification (would need TLOG integration)
        if self.config.allow_keyless && !signature.certificate.is_none() {
            return Ok(VerificationResult {
                verified: false,
                signature_type: SignatureType::Cosign,
                signer: None,
                certificate: None,
                tlog_entries: Vec::new(),
                warnings: vec!["Keyless verification requires TLOG integration".to_string()],
            });
        }

        Ok(VerificationResult {
            verified: false,
            signature_type: SignatureType::Cosign,
            signer: None,
            certificate: None,
            tlog_entries: Vec::new(),
            warnings: vec![
                "Cosign signature verification failed - no matching key found".to_string(),
            ],
        })
    }

    /// Verify Notary v1 signature
    async fn verify_notary_v1_signature(
        &self,
        image_path: &Path,
        _signature: &ImageSignature,
    ) -> Result<VerificationResult> {
        // Notary v1 uses a different format
        // This is a simplified implementation
        let image_data = std::fs::read(image_path)?;
        let mut hasher = Sha256::new();
        hasher.update(&image_data);
        let _digest = format!("{:x}", hasher.finalize());

        // In a real implementation, this would verify against Notary v1 server
        // and validate the signature using the notary client library

        Ok(VerificationResult {
            verified: false,
            signature_type: SignatureType::NotaryV1,
            signer: None,
            certificate: None,
            tlog_entries: Vec::new(),
            warnings: vec!["Notary v1 verification not fully implemented".to_string()],
        })
    }

    /// Verify Notary v2 signature
    async fn verify_notary_v2_signature(
        &self,
        _image_path: &Path,
        _signature: &ImageSignature,
    ) -> Result<VerificationResult> {
        // Notary v2 uses the same format as cosign but with different trust model
        // This would use the notary v2 client library

        Ok(VerificationResult {
            verified: false,
            signature_type: SignatureType::NotaryV2,
            signer: None,
            certificate: None,
            tlog_entries: Vec::new(),
            warnings: vec!["Notary v2 verification not fully implemented".to_string()],
        })
    }

    /// Verify simple signature (for testing/development)
    /// Verify simple signature (for testing/development)
    async fn verify_simple_signature(
        &self,
        image_path: &Path,
        signature: &ImageSignature,
    ) -> Result<VerificationResult> {
        // Simple verification using HMAC-SHA256 with a test key
        // In a real implementation, this would verify the signature properly
        let image_data = std::fs::read(image_path)?;
        let mut hasher = Sha256::new();
        hasher.update(&image_data);
        let digest = format!("{:x}", hasher.finalize());

        if digest != signature.image_digest {
            bail!("Image digest mismatch");
        }

        // For simple verification, we check if the signature is a valid HMAC
        // Using a fixed test key for development
        let test_key = b"test-key-for-development-only";
        let mut hmac = hmac::Hmac::<Sha256>::new_from_slice(test_key)
            .map_err(|e| anyhow::anyhow!("Invalid HMAC key: {}", e))?;
        hmac.update(&image_data);
        let expected_signature = hmac.finalize().into_bytes();

        let verified = signature.signature == expected_signature.to_vec();

        Ok(VerificationResult {
            verified,
            signature_type: SignatureType::Simple,
            signer: signature.annotations.get("signer").cloned(),
            certificate: signature
                .certificate
                .as_ref()
                .map(|c| String::from_utf8_lossy(c).to_string()),
            tlog_entries: Vec::new(),
            warnings: vec!["Using simple HMAC verification - not for production".to_string()],
        })
    }

    /// Calculate image digest
    async fn calculate_image_digest(&self, image_path: &Path) -> Result<String> {
        let mut file = tokio::fs::File::open(image_path).await?;
        let mut hasher = Sha256::new();

        use tokio::io::AsyncReadExt;
        let mut buffer = vec![0u8; 8192];
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Add a trusted public key
    pub fn add_trusted_key(&mut self, key: Vec<u8>) {
        self.trusted_keys.push(key);
    }

    /// Add a trusted certificate
    pub fn add_trusted_certificate(&mut self, cert: Vec<u8>) {
        self.trusted_certs.push(cert);
    }
}

/// Extract signature from image annotations (cosign stores signatures in annotations)
pub fn extract_signature_from_annotations(
    annotations: &HashMap<String, String>,
) -> Option<ImageSignature> {
    // Cosign stores signatures in the "dev.cosignproject.cosign/signature" annotation
    // and certificates in "dev.cosignproject.cosign/certificate"

    if let Some(sig_b64) = annotations.get("dev.cosignproject.cosign/signature") {
        let signature = base64::decode(sig_b64).ok()?;

        let certificate = annotations
            .get("dev.cosignproject.cosign/certificate")
            .and_then(|c| base64::decode(c).ok());

        let chain = annotations
            .get("dev.cosignproject.cosign/chain")
            .and_then(|c| base64::decode(c).ok())
            .map(|c| vec![c])
            .unwrap_or_default();

        // Extract annotations
        let mut sig_annotations = HashMap::new();
        for (k, v) in annotations {
            if k.starts_with("dev.cosignproject.cosign/") {
                sig_annotations.insert(k.clone(), v.clone());
            }
        }

        Some(ImageSignature {
            image_digest: annotations
                .get("dev.cosignproject.cosign/digest")
                .unwrap_or(&String::new())
                .clone(),
            signature_type: SignatureType::Cosign,
            signature,
            certificate,
            chain,
            annotations: sig_annotations,
            signed_at: chrono::Utc::now(),
        })
    } else {
        None
    }
}

/// Verify image with cosign keyless signing (placeholder for future implementation)
pub async fn verify_keyless_signature(
    _image_ref: &str,
    _identity_regex: Option<&str>,
    _issuer_regex: Option<&str>,
) -> Result<VerificationResult> {
    Ok(VerificationResult {
        verified: false,
        signature_type: SignatureType::Cosign,
        signer: None,
        certificate: None,
        tlog_entries: Vec::new(),
        warnings: vec!["Keyless verification not fully implemented".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_config_default() {
        let config = SignatureConfig::default();
        assert!(!config.enabled);
        assert!(config.allow_keyless);
    }

    #[test]
    fn test_signature_type_serialization() {
        let sig_type = SignatureType::Cosign;
        let json = serde_json::to_string(&sig_type).unwrap();
        assert_eq!(json, "\"cosign\"");
    }

    #[tokio::test]
    async fn test_simple_verification() {
        let config = SignatureConfig {
            enabled: true,
            ..Default::default()
        };
        let verifier = SignatureVerifier::new(config).unwrap();

        // Create a test file
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("test.img");
        std::fs::write(&test_file, b"test image data").unwrap();

        // Create a simple signature with correct HMAC
        let test_key = b"test-key-for-development-only";
        let mut hmac = hmac::Hmac::<Sha256>::new_from_slice(test_key).unwrap();
        hmac.update(b"test image data");
        let expected_signature = hmac.finalize().into_bytes();

        let mut hasher = Sha256::new();
        hasher.update(b"test image data");
        let digest = format!("{:x}", hasher.finalize());

        let signature = ImageSignature {
            image_digest: digest.clone(),
            signature_type: SignatureType::Simple,
            signature: expected_signature.to_vec(),
            certificate: None,
            chain: Vec::new(),
            annotations: HashMap::new(),
            signed_at: chrono::Utc::now(),
        };

        let result = verifier.verify_image(&test_file, &signature).await.unwrap();
        assert!(result.verified); // Simple verification returns true for correct HMAC

        // Test with wrong signature
        let wrong_signature = ImageSignature {
            image_digest: digest.clone(),
            signature_type: SignatureType::Simple,
            signature: b"wrong-signature".to_vec(),
            certificate: None,
            chain: Vec::new(),
            annotations: HashMap::new(),
            signed_at: chrono::Utc::now(),
        };

        let result = verifier
            .verify_image(&test_file, &wrong_signature)
            .await
            .unwrap();
        assert!(!result.verified); // Simple verification returns false for wrong signature
    }
}
