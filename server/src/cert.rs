use std::time::Duration;
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use sha2::{Sha256, Digest};
use wtransport::{Endpoint, Identity, ServerConfig, tls::Certificate, tls::CertificateChain, tls::PrivateKey};
use base64::Engine;
use time::{OffsetDateTime, Duration as TimeDuration};

/// Loads a cached self-signed cert if it has >1 day remaining, otherwise
/// generates a new 13-day cert (Chrome's max for hash-pinned certs).
/// Returns (cert_der, key_der, sha256_b64_fingerprint).
pub fn make_or_load_cert() -> (Vec<u8>, Vec<u8>, String) {
    const CERT_FILE: &str = "cert.der";
    const KEY_FILE: &str = "key.der";
    const EXPIRY_FILE: &str = "cert_expiry.txt";

    // Reuse saved cert if it has more than 1 day remaining
    if let (Ok(cert_der), Ok(key_der), Ok(expiry_str)) = (
        std::fs::read(CERT_FILE),
        std::fs::read(KEY_FILE),
        std::fs::read_to_string(EXPIRY_FILE),
    ) {
        if let Ok(expiry_unix) = expiry_str.trim().parse::<i64>() {
            let now_unix = OffsetDateTime::now_utc().unix_timestamp();
            let days_left = (expiry_unix - now_unix) / 86400;
            if days_left > 1 {
                let mut hasher = Sha256::new();
                hasher.update(&cert_der);
                let b64 = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());
                println!("[CERT] Reusing saved cert ({} days remaining)", days_left);
                println!("[CERT] SHA-256 fingerprint (base64): {}", b64);
                return (cert_der, key_der, b64);
            }
            println!("[CERT] Saved cert expires too soon, regenerating...");
        }
    }

    // Generate a new cert (Chrome enforces <= 14 days for hash-pinned certs)
    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .expect("ECDSA keygen failed");
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(vec!["localhost".into()])
        .expect("failed to build cert params");
    params.not_before = now;
    params.not_after = now + TimeDuration::days(13);

    let cert = params.self_signed(&key_pair)
        .expect("self-signed cert generation failed");
    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    let _ = std::fs::write(CERT_FILE, &cert_der);
    let _ = std::fs::write(KEY_FILE, &key_der);
    let _ = std::fs::write(
        EXPIRY_FILE,
        (now + TimeDuration::days(13)).unix_timestamp().to_string(),
    );

    // Write PEM files so serve.py can use them for HTTPS static file serving
    let cert_b64 = base64::engine::general_purpose::STANDARD.encode(&cert_der);
    let cert_pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        cert_b64
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).expect("base64 output is always valid UTF-8"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let key_b64 = base64::engine::general_purpose::STANDARD.encode(&key_der);
    let key_pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        key_b64
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).expect("base64 output is always valid UTF-8"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let _ = std::fs::write("cert.pem", &cert_pem);
    let _ = std::fs::write("key.pem", &key_pem);

    let mut hasher = Sha256::new();
    hasher.update(&cert_der);
    let b64 = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());

    println!("[CERT] Generated new cert (valid 13 days)");
    println!("[CERT] SHA-256 fingerprint (base64): {}", b64);
    println!("[CERT] Paste this into NetworkManager.gd as CERT_HASH_B64");

    (cert_der, key_der, b64)
}

/// Builds the wtransport server endpoint.
/// Uses a Let's Encrypt cert when the DOMAIN env var is set,
/// otherwise falls back to a local self-signed cert.
pub async fn build_endpoint() -> Endpoint<wtransport::endpoint::endpoint_side::Server> {
    let identity = match std::env::var("DOMAIN") {
        Ok(domain) => {
            let cert_path = format!("/etc/letsencrypt/live/{domain}/fullchain.pem");
            let key_path = format!("/etc/letsencrypt/live/{domain}/privkey.pem");
            println!("[CERT] Loading Let's Encrypt cert for {domain}");

            let cert_bytes = std::fs::read(&cert_path)
                .unwrap_or_else(|e| panic!("[CERT] Cannot read {cert_path}: {e}"));
            let key_bytes = std::fs::read(&key_path)
                .unwrap_or_else(|e| panic!("[CERT] Cannot read {key_path}: {e}"));

            // Parse ALL certs from fullchain.pem (leaf + Let's Encrypt intermediates)
            let certs: Vec<Certificate> = rustls_pemfile::certs(&mut cert_bytes.as_slice())
                .filter_map(|r| r.ok())
                .map(|der| Certificate::from_der(der.to_vec()).expect("invalid cert DER"))
                .collect();
            println!("[CERT] Loaded {} certificate(s) in chain for {domain}", certs.len());
            assert!(!certs.is_empty(), "[CERT] No certificates found in {cert_path}");

            let key_der = rustls_pemfile::private_key(&mut key_bytes.as_slice())
                .unwrap_or_else(|e| panic!("[CERT] Cannot parse key for {domain}: {e}"))
                .unwrap_or_else(|| panic!("[CERT] No private key found in {key_path}"));

            Identity::new(
                CertificateChain::new(certs),
                PrivateKey::from_der_pkcs8(key_der.secret_der().to_vec()),
            )
        }
        Err(_) => {
            let (cert_der, key_der, _hash_b64) = make_or_load_cert();
            Identity::new(
                CertificateChain::single(
                    Certificate::from_der(cert_der).expect("invalid cert DER bytes"),
                ),
                PrivateKey::from_der_pkcs8(key_der),
            )
        }
    };

    let config = ServerConfig::builder()
        .with_bind_default(7777)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();

    Endpoint::server(config).expect("failed to start WebTransport endpoint")
}