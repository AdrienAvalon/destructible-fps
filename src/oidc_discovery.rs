//! Bounded HTTPS discovery of one explicitly configured `OpenID` Connect issuer.

use reqwest::{Certificate, Client, Url, redirect::Policy};
use serde::Deserialize;
use std::{fmt, time::Duration};

pub const MAX_OIDC_DISCOVERY_BYTES: usize = 16 * 1_024;
pub const MAX_OIDC_DISCOVERY_ROOT_BYTES: usize = 256 * 1_024;
pub const MAX_OIDC_DISCOVERY_ROOTS: usize = 16;
pub const MAX_OIDC_DISCOVERY_ISSUER_BYTES: usize = 512;
pub const MIN_OIDC_REFRESH_INTERVAL_SECONDS: u64 = 60;
pub const MAX_OIDC_REFRESH_INTERVAL_SECONDS: u64 = 60 * 60;
const OIDC_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const OIDC_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Eq, PartialEq)]
pub enum OidcDiscoveryError {
    InvalidIssuer,
    InvalidRefreshInterval,
    InvalidRootBundle,
    HttpClient,
    Request,
    HttpStatus,
    InvalidContentType,
    OversizedResponse { maximum: usize },
    InvalidDocument,
    IssuerMismatch,
    InvalidJwksUri,
}

impl fmt::Display for OidcDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIssuer => write!(formatter, "invalid OIDC discovery issuer"),
            Self::InvalidRefreshInterval => write!(formatter, "invalid OIDC refresh interval"),
            Self::InvalidRootBundle => write!(formatter, "invalid OIDC discovery root bundle"),
            Self::HttpClient => write!(formatter, "cannot construct bounded OIDC HTTP client"),
            Self::Request => write!(formatter, "OIDC HTTPS request failed"),
            Self::HttpStatus => write!(formatter, "OIDC HTTPS endpoint returned an error status"),
            Self::InvalidContentType => write!(formatter, "OIDC endpoint did not return JSON"),
            Self::OversizedResponse { maximum } => {
                write!(formatter, "OIDC response exceeds the {maximum}-byte limit")
            }
            Self::InvalidDocument => write!(formatter, "invalid OIDC discovery document"),
            Self::IssuerMismatch => write!(formatter, "discovered OIDC issuer does not match"),
            Self::InvalidJwksUri => write!(formatter, "invalid discovered OIDC JWKS URI"),
        }
    }
}

impl std::error::Error for OidcDiscoveryError {}

#[derive(Clone)]
pub struct OidcDiscoveryClient {
    client: Client,
    issuer: String,
    issuer_url: Url,
    discovery_url: Url,
    refresh_interval: Duration,
}

impl OidcDiscoveryClient {
    /// Builds a TLS-verifying client for one exact issuer and an optional private trust bundle.
    ///
    /// Redirects, ambient proxies, cleartext HTTP, and cross-origin JWKS endpoints are deliberately
    /// disabled. The optional bundle augments platform roots and is independently bounded.
    ///
    /// # Errors
    ///
    /// Rejects malformed issuers, out-of-policy intervals, malformed root bundles, and unavailable
    /// HTTPS client configuration.
    pub fn new(
        issuer: impl Into<String>,
        refresh_interval: Duration,
        root_bundle: Option<&[u8]>,
    ) -> Result<Self, OidcDiscoveryError> {
        let issuer = issuer.into();
        let issuer_url = validate_issuer_url(&issuer)?;
        let refresh_seconds = refresh_interval.as_secs();
        if !(MIN_OIDC_REFRESH_INTERVAL_SECONDS..=MAX_OIDC_REFRESH_INTERVAL_SECONDS)
            .contains(&refresh_seconds)
            || refresh_interval.subsec_nanos() != 0
        {
            return Err(OidcDiscoveryError::InvalidRefreshInterval);
        }
        let discovery_url = discovery_url(&issuer)?;
        let mut builder = Client::builder()
            .https_only(true)
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .http1_only()
            .redirect(Policy::none())
            .no_proxy()
            .connect_timeout(OIDC_CONNECT_TIMEOUT)
            .timeout(OIDC_REQUEST_TIMEOUT)
            .user_agent("destructible-fps-oidc/0.1");
        if let Some(bundle) = root_bundle {
            if bundle.is_empty() || bundle.len() > MAX_OIDC_DISCOVERY_ROOT_BYTES {
                return Err(OidcDiscoveryError::InvalidRootBundle);
            }
            let roots = Certificate::from_pem_bundle(bundle)
                .map_err(|_| OidcDiscoveryError::InvalidRootBundle)?;
            if roots.is_empty() || roots.len() > MAX_OIDC_DISCOVERY_ROOTS {
                return Err(OidcDiscoveryError::InvalidRootBundle);
            }
            for root in roots {
                builder = builder.add_root_certificate(root);
            }
        }
        let client = builder
            .build()
            .map_err(|_| OidcDiscoveryError::HttpClient)?;
        Ok(Self {
            client,
            issuer,
            issuer_url,
            discovery_url,
            refresh_interval,
        })
    }

    #[must_use]
    pub const fn refresh_interval(&self) -> Duration {
        self.refresh_interval
    }

    /// Fetches and validates discovery metadata, then returns one bounded JWKS document.
    ///
    /// # Errors
    ///
    /// Fails closed on TLS, timeout, status, media type, size, JSON, issuer, or endpoint-policy
    /// errors. The response bodies and URLs are never included in the public error text.
    pub async fn fetch_jwks(&self) -> Result<Vec<u8>, OidcDiscoveryError> {
        let metadata = self
            .get_bounded_json(
                &self.discovery_url,
                MAX_OIDC_DISCOVERY_BYTES,
                &["application/json"],
            )
            .await?;
        let jwks_url = validate_discovery_document(&metadata, &self.issuer, &self.issuer_url)?;
        self.get_bounded_json(
            &jwks_url,
            crate::MAX_OIDC_JWKS_BYTES,
            &["application/json", "application/jwk-set+json"],
        )
        .await
    }

    async fn get_bounded_json(
        &self,
        url: &Url,
        maximum: usize,
        allowed_content_types: &[&str],
    ) -> Result<Vec<u8>, OidcDiscoveryError> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| OidcDiscoveryError::Request)?;
        if !response.status().is_success() {
            return Err(OidcDiscoveryError::HttpStatus);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let media_type = content_type
            .split(';')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if !allowed_content_types
            .iter()
            .any(|allowed| media_type.eq_ignore_ascii_case(allowed))
        {
            return Err(OidcDiscoveryError::InvalidContentType);
        }
        if response
            .content_length()
            .is_some_and(|length| length > u64::try_from(maximum).unwrap_or(u64::MAX))
        {
            return Err(OidcDiscoveryError::OversizedResponse { maximum });
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| OidcDiscoveryError::Request)?
        {
            if body.len().saturating_add(chunk.len()) > maximum {
                return Err(OidcDiscoveryError::OversizedResponse { maximum });
            }
            body.extend_from_slice(&chunk);
        }
        if body.is_empty() {
            return Err(OidcDiscoveryError::InvalidDocument);
        }
        Ok(body)
    }
}

#[derive(Deserialize)]
struct DiscoveryDocument {
    issuer: String,
    jwks_uri: String,
}

fn validate_issuer_url(issuer: &str) -> Result<Url, OidcDiscoveryError> {
    if !(9..=MAX_OIDC_DISCOVERY_ISSUER_BYTES).contains(&issuer.len()) {
        return Err(OidcDiscoveryError::InvalidIssuer);
    }
    let url = Url::parse(issuer).map_err(|_| OidcDiscoveryError::InvalidIssuer)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OidcDiscoveryError::InvalidIssuer);
    }
    Ok(url)
}

fn discovery_url(issuer: &str) -> Result<Url, OidcDiscoveryError> {
    Url::parse(&format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    ))
    .map_err(|_| OidcDiscoveryError::InvalidIssuer)
}

fn validate_discovery_document(
    bytes: &[u8],
    issuer: &str,
    issuer_url: &Url,
) -> Result<Url, OidcDiscoveryError> {
    let document = serde_json::from_slice::<DiscoveryDocument>(bytes)
        .map_err(|_| OidcDiscoveryError::InvalidDocument)?;
    if document.issuer != issuer {
        return Err(OidcDiscoveryError::IssuerMismatch);
    }
    let jwks_url =
        Url::parse(&document.jwks_uri).map_err(|_| OidcDiscoveryError::InvalidJwksUri)?;
    if jwks_url.scheme() != "https"
        || jwks_url.host_str().is_none()
        || !jwks_url.username().is_empty()
        || jwks_url.password().is_some()
        || jwks_url.fragment().is_some()
        || !same_origin(issuer_url, &jwks_url)
    {
        return Err(OidcDiscoveryError::InvalidJwksUri);
    }
    Ok(jwks_url)
}

fn same_origin(first: &Url, second: &Url) -> bool {
    first.scheme() == second.scheme()
        && first.host_str() == second.host_str()
        && first.port_or_known_default() == second.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
    };

    #[test]
    fn issuer_and_metadata_policy_rejects_unsafe_endpoints() {
        for issuer in [
            "http://identity.example.test/realms/game",
            "https://user@identity.example.test/realms/game",
            "https://identity.example.test/realms/game?tenant=other",
        ] {
            assert!(matches!(
                OidcDiscoveryClient::new(issuer, Duration::from_mins(5), None),
                Err(OidcDiscoveryError::InvalidIssuer)
            ));
        }
        let issuer = "https://identity.example.test/realms/game";
        let issuer_url = validate_issuer_url(issuer).expect("issuer URL");
        for (document, expected) in [
            (
                br#"{"issuer":"https://other.example.test/realms/game","jwks_uri":"https://identity.example.test/keys"}"#.as_slice(),
                OidcDiscoveryError::IssuerMismatch,
            ),
            (
                br#"{"issuer":"https://identity.example.test/realms/game","jwks_uri":"http://identity.example.test/keys"}"#.as_slice(),
                OidcDiscoveryError::InvalidJwksUri,
            ),
            (
                br#"{"issuer":"https://identity.example.test/realms/game","jwks_uri":"https://other.example.test/keys"}"#.as_slice(),
                OidcDiscoveryError::InvalidJwksUri,
            ),
        ] {
            assert_eq!(
                validate_discovery_document(document, issuer, &issuer_url),
                Err(expected)
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn trusted_https_discovery_fetches_bounded_same_origin_jwks() {
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .expect("discovery test identity");
        let certificate = identity.cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der());
        let tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], private_key.into())
            .expect("discovery test TLS config");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("discovery test listener");
        let address = listener.local_addr().expect("discovery test address");
        let issuer = format!("https://localhost:{}/realms/game", address.port());
        let discovery = format!(
            "{{\"issuer\":{issuer:?},\"jwks_uri\":\"https://localhost:{}/realms/game/keys\"}}",
            address.port()
        );
        let jwks = br#"{"keys":[]}"#.to_vec();
        let server_jwks = jwks.clone();
        let server = tokio::spawn(serve_https_documents(
            listener,
            Arc::new(tls),
            vec![discovery.into_bytes(), server_jwks],
        ));
        let client = OidcDiscoveryClient::new(
            issuer,
            Duration::from_mins(5),
            Some(identity.cert.pem().as_bytes()),
        )
        .expect("trusted discovery client");

        assert_eq!(client.fetch_jwks().await, Ok(jwks));
        server.await.expect("discovery server task");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chunked_discovery_body_cannot_cross_the_memory_ceiling() {
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .expect("bounded discovery test identity");
        let private_key = PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der());
        let tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![identity.cert.der().clone()], private_key.into())
            .expect("bounded discovery test TLS config");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bounded discovery test listener");
        let address = listener
            .local_addr()
            .expect("bounded discovery test address");
        let issuer = format!("https://localhost:{}/realms/game", address.port());
        let oversized = vec![b' '; MAX_OIDC_DISCOVERY_BYTES + 1];
        let server = tokio::spawn(serve_chunked_https_document(
            listener,
            Arc::new(tls),
            oversized,
        ));
        let client = OidcDiscoveryClient::new(
            issuer,
            Duration::from_mins(5),
            Some(identity.cert.pem().as_bytes()),
        )
        .expect("bounded discovery client");

        assert_eq!(
            client
                .get_bounded_json(
                    &client.discovery_url,
                    MAX_OIDC_DISCOVERY_BYTES,
                    &["application/json"],
                )
                .await,
            Err(OidcDiscoveryError::OversizedResponse {
                maximum: MAX_OIDC_DISCOVERY_BYTES
            })
        );
        server.await.expect("bounded discovery server task");
    }

    async fn serve_https_documents(
        listener: TcpListener,
        config: Arc<ServerConfig>,
        documents: Vec<Vec<u8>>,
    ) {
        let acceptor = TlsAcceptor::from(config);
        for document in documents {
            let (stream, _) = listener.accept().await.expect("discovery TLS client");
            let mut stream = acceptor
                .accept(stream)
                .await
                .expect("discovery TLS handshake");
            let mut request = vec![0_u8; 8 * 1_024];
            let mut received = 0_usize;
            loop {
                let count = stream
                    .read(&mut request[received..])
                    .await
                    .expect("discovery request");
                assert!(count > 0, "discovery request ended before headers");
                received += count;
                if request[..received]
                    .windows(4)
                    .any(|window| window == b"\r\n\r\n")
                {
                    break;
                }
                assert!(received < request.len(), "discovery request exceeded bound");
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                document.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("discovery response headers");
            stream
                .write_all(&document)
                .await
                .expect("discovery response body");
            stream.shutdown().await.expect("discovery TLS shutdown");
        }
    }

    async fn serve_chunked_https_document(
        listener: TcpListener,
        config: Arc<ServerConfig>,
        document: Vec<u8>,
    ) {
        let acceptor = TlsAcceptor::from(config);
        let (stream, _) = listener.accept().await.expect("bounded TLS client");
        let mut stream = acceptor
            .accept(stream)
            .await
            .expect("bounded TLS handshake");
        let mut request = vec![0_u8; 8 * 1_024];
        let _ = stream
            .read(&mut request)
            .await
            .expect("bounded discovery request");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("bounded response headers");
        stream
            .write_all(format!("{:x}\r\n", document.len()).as_bytes())
            .await
            .expect("bounded response chunk size");
        stream
            .write_all(&document)
            .await
            .expect("bounded response chunk");
        stream
            .write_all(b"\r\n0\r\n\r\n")
            .await
            .expect("bounded response terminator");
        stream.shutdown().await.expect("bounded TLS shutdown");
    }
}
