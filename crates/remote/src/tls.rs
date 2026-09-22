//! Per-pair private CA support. Certificate expiry and hostname validation stay enabled.
use anyhow::{Result, bail};
use rustls::pki_types::CertificateDer;
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio_tungstenite::Connector;

fn certificates(pem: &str) -> Result<Vec<CertificateDer<'static>>> {
    if pem.len() > 8192 {
        bail!("relay CA exceeds 8 KiB")
    }
    let certificates = rustls_pemfile::certs(&mut Cursor::new(pem.as_bytes()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if certificates.is_empty() || certificates.len() > 4 {
        bail!("relay CA must contain 1 to 4 PEM certificates")
    }
    Ok(certificates)
}
pub(super) fn http_client(pem: Option<&str>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(pem) = pem {
        builder = builder.tls_built_in_root_certs(false);
        for cert in certificates(pem)? {
            builder = builder.add_root_certificate(reqwest::Certificate::from_der(cert.as_ref())?);
        }
    }
    Ok(builder.build()?)
}
pub(super) fn connector(pem: Option<&str>) -> Result<Option<Connector>> {
    let Some(pem) = pem else { return Ok(None) };
    let mut roots = rustls::RootCertStore::empty();
    for certificate in certificates(pem)? {
        roots.add(certificate)?;
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Some(Connector::Rustls(Arc::new(config))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_invalid_ca_never_falls_back_to_default_or_insecure_tls() {
        for value in [String::new(), "not a certificate".into(), "x".repeat(8193)] {
            assert!(http_client(Some(&value)).is_err());
            assert!(connector(Some(&value)).is_err());
        }
        assert!(connector(None).unwrap().is_none());
    }
}
