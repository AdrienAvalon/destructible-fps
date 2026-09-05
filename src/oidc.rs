//! Bounded offline OIDC access-token verification for secure game sessions.

use crate::{AuthenticatedPrincipal, MAX_SESSION_CREDENTIAL_BYTES, SessionCredentialVerifier};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use core::fmt;
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
};
use ring::digest::{Context, SHA256};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_OIDC_JWKS_BYTES: usize = 64 * 1_024;
pub const MAX_OIDC_JWKS_KEYS: usize = 32;
pub const MAX_OIDC_REPLAY_ENTRIES: usize = 4_096;
pub const MAX_OIDC_TOKEN_LIFETIME_SECONDS: u64 = 15 * 60;
pub const OIDC_CLOCK_SKEW_SECONDS: u64 = 30;
pub const OIDC_MINIMUM_REMAINING_SECONDS: u64 = 10;

const MAX_ISSUER_BYTES: usize = 512;
const MAX_AUDIENCE_BYTES: usize = 256;
const MAX_SUBJECT_BYTES: usize = 255;
const MIN_JTI_BYTES: usize = 8;
const MAX_JTI_BYTES: usize = 128;
const MAX_KEY_ID_BYTES: usize = 128;
const MIN_RSA_MODULUS_BYTES: usize = 256;
const MAX_RSA_MODULUS_BYTES: usize = 512;
const MAX_RSA_MODULUS_ENCODED_BYTES: usize = 700;
const MAX_RSA_EXPONENT_ENCODED_BYTES: usize = 8;
const RSA_F4_EXPONENT: [u8; 3] = [1, 0, 1];
const PRINCIPAL_DOMAIN: &[u8] = b"destructible-fps/oidc-principal/v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcVerificationError {
    InvalidConfiguration,
    JwksOversized(usize),
    InvalidJwks,
    InvalidKeyCount(usize),
    InvalidKey,
    DuplicateKeyId,
    UnsupportedHeader,
    UnknownKey,
    InvalidCredential,
    InvalidClaims,
    ReplayedCredential,
    ReplayCapacity,
    StateUnavailable,
    ClockUnavailable,
}

impl fmt::Display for OidcVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => write!(formatter, "invalid OIDC verifier configuration"),
            Self::JwksOversized(bytes) => write!(formatter, "OIDC JWKS has {bytes} bytes"),
            Self::InvalidJwks => write!(formatter, "invalid OIDC JWKS"),
            Self::InvalidKeyCount(keys) => write!(formatter, "invalid OIDC JWKS key count {keys}"),
            Self::InvalidKey => write!(formatter, "invalid OIDC verification key"),
            Self::DuplicateKeyId => write!(formatter, "duplicate OIDC verification key ID"),
            Self::UnsupportedHeader => write!(formatter, "unsupported OIDC token header"),
            Self::UnknownKey => write!(formatter, "unknown OIDC verification key"),
            Self::InvalidCredential => write!(formatter, "invalid OIDC credential"),
            Self::InvalidClaims => write!(formatter, "invalid OIDC claims"),
            Self::ReplayedCredential => write!(formatter, "OIDC credential was already admitted"),
            Self::ReplayCapacity => write!(formatter, "OIDC replay cache is full"),
            Self::StateUnavailable => write!(formatter, "OIDC verifier state is unavailable"),
            Self::ClockUnavailable => write!(formatter, "system clock is unavailable"),
        }
    }
}

impl std::error::Error for OidcVerificationError {}

#[derive(Deserialize)]
struct OidcClaims {
    exp: u64,
    iat: u64,
    jti: String,
    sub: String,
}

type VerificationKeys = BTreeMap<String, Arc<DecodingKey>>;

/// Offline RS256 verifier backed by an atomically replaceable bounded JWKS.
///
/// `verify_oidc` performs no network or file-system work. A trusted supervisor is responsible for
/// fetching OIDC discovery/JWKS data over authenticated TLS, applying cache lifetime policy, and
/// calling `replace_jwks` after validating the expected issuer endpoint.
pub struct OidcSessionVerifier {
    issuer: String,
    validation: Validation,
    keys: RwLock<VerificationKeys>,
    replay: Mutex<BTreeMap<String, u64>>,
}

impl OidcSessionVerifier {
    /// Creates a fail-closed verifier from an exact issuer, audience, and bounded JWKS document.
    ///
    /// # Errors
    ///
    /// Rejects non-HTTPS issuers, empty/oversized policy strings, malformed key sets, unsupported
    /// algorithms, duplicate key IDs, non-signing keys, weak RSA moduli, and non-F4 exponents.
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        jwks_json: &[u8],
    ) -> Result<Self, OidcVerificationError> {
        let issuer = issuer.into();
        let audience = audience.into();
        validate_configuration(&issuer, &audience)?;
        let keys = parse_verification_keys(jwks_json)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.set_issuer(&[&issuer]);
        validation.set_audience(&[&audience]);
        validation.validate_nbf = true;
        validation.leeway = OIDC_CLOCK_SKEW_SECONDS;
        validation.reject_tokens_expiring_in_less_than = OIDC_MINIMUM_REMAINING_SECONDS;
        Ok(Self {
            issuer,
            validation,
            keys: RwLock::new(keys),
            replay: Mutex::new(BTreeMap::new()),
        })
    }

    /// Replaces the complete verification-key set only after the new document validates.
    ///
    /// # Errors
    ///
    /// Leaves the prior key set intact when parsing, key validation, or lock acquisition fails.
    pub fn replace_jwks(&self, jwks_json: &[u8]) -> Result<usize, OidcVerificationError> {
        let replacement = parse_verification_keys(jwks_json)?;
        let count = replacement.len();
        let mut keys = self
            .keys
            .write()
            .map_err(|_| OidcVerificationError::StateUnavailable)?;
        *keys = replacement;
        drop(keys);
        Ok(count)
    }

    /// Verifies one access token and consumes its bounded `jti` for one session admission.
    ///
    /// # Errors
    ///
    /// Rejects malformed or oversized credentials, algorithm/key substitution, signature or claim
    /// failures, excessive token lifetime, invalid subjects/JTIs, replay, and exhausted state.
    pub fn verify_oidc(
        &self,
        credential: &[u8],
    ) -> Result<AuthenticatedPrincipal, OidcVerificationError> {
        if !(16..=MAX_SESSION_CREDENTIAL_BYTES).contains(&credential.len()) {
            return Err(OidcVerificationError::InvalidCredential);
        }
        let header =
            decode_header(credential).map_err(|_| OidcVerificationError::InvalidCredential)?;
        if header.alg != Algorithm::RS256
            || header.jku.is_some()
            || header.jwk.is_some()
            || header.x5u.is_some()
            || header.crit.is_some()
            || header.enc.is_some()
            || header.zip.is_some()
        {
            return Err(OidcVerificationError::UnsupportedHeader);
        }
        let key_id = header
            .kid
            .filter(|key_id| valid_identifier(key_id, 1, MAX_KEY_ID_BYTES))
            .ok_or(OidcVerificationError::UnsupportedHeader)?;
        let key = self
            .keys
            .read()
            .map_err(|_| OidcVerificationError::StateUnavailable)?
            .get(&key_id)
            .cloned()
            .ok_or(OidcVerificationError::UnknownKey)?;
        let token = decode::<OidcClaims>(credential, &key, &self.validation)
            .map_err(|_| OidcVerificationError::InvalidCredential)?;
        let now = unix_seconds()?;
        validate_claims(&token.claims, now)?;
        self.consume_jti(&token.claims.jti, token.claims.exp, now)?;
        Ok(principal_from_subject(&self.issuer, &token.claims.sub))
    }

    #[must_use]
    pub fn replay_entries(&self) -> Option<usize> {
        self.replay.lock().ok().map(|entries| entries.len())
    }

    fn consume_jti(&self, jti: &str, expires: u64, now: u64) -> Result<(), OidcVerificationError> {
        let mut replay = self
            .replay
            .lock()
            .map_err(|_| OidcVerificationError::StateUnavailable)?;
        replay.retain(|_, retained_until| *retained_until > now);
        if replay.contains_key(jti) {
            return Err(OidcVerificationError::ReplayedCredential);
        }
        if replay.len() >= MAX_OIDC_REPLAY_ENTRIES {
            return Err(OidcVerificationError::ReplayCapacity);
        }
        replay.insert(
            jti.to_owned(),
            expires.saturating_add(OIDC_CLOCK_SKEW_SECONDS),
        );
        drop(replay);
        Ok(())
    }
}

impl SessionCredentialVerifier for OidcSessionVerifier {
    fn verify(&self, credential: &[u8]) -> Option<AuthenticatedPrincipal> {
        self.verify_oidc(credential).ok()
    }
}

fn validate_configuration(issuer: &str, audience: &str) -> Result<(), OidcVerificationError> {
    if !issuer.starts_with("https://")
        || !valid_identifier(issuer, 9, MAX_ISSUER_BYTES)
        || !valid_identifier(audience, 1, MAX_AUDIENCE_BYTES)
    {
        return Err(OidcVerificationError::InvalidConfiguration);
    }
    Ok(())
}

fn parse_verification_keys(jwks_json: &[u8]) -> Result<VerificationKeys, OidcVerificationError> {
    if jwks_json.len() > MAX_OIDC_JWKS_BYTES {
        return Err(OidcVerificationError::JwksOversized(jwks_json.len()));
    }
    if jwks_json.is_empty() {
        return Err(OidcVerificationError::InvalidJwks);
    }
    let jwks: JwkSet =
        serde_json::from_slice(jwks_json).map_err(|_| OidcVerificationError::InvalidJwks)?;
    if jwks.keys.is_empty() || jwks.keys.len() > MAX_OIDC_JWKS_KEYS {
        return Err(OidcVerificationError::InvalidKeyCount(jwks.keys.len()));
    }
    let mut keys = BTreeMap::new();
    for jwk in &jwks.keys {
        let key_id = validate_jwk(jwk)?;
        let key = DecodingKey::try_from(jwk).map_err(|_| OidcVerificationError::InvalidKey)?;
        if keys.insert(key_id, Arc::new(key)).is_some() {
            return Err(OidcVerificationError::DuplicateKeyId);
        }
    }
    Ok(keys)
}

fn validate_jwk(jwk: &Jwk) -> Result<String, OidcVerificationError> {
    let key_id = jwk
        .common
        .key_id
        .as_ref()
        .filter(|key_id| valid_identifier(key_id, 1, MAX_KEY_ID_BYTES))
        .cloned()
        .ok_or(OidcVerificationError::InvalidKey)?;
    if jwk.common.key_algorithm != Some(KeyAlgorithm::RS256)
        || jwk
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|usage| usage != &PublicKeyUse::Signature)
        || jwk
            .common
            .key_operations
            .as_ref()
            .is_some_and(|operations| {
                operations.is_empty()
                    || operations
                        .iter()
                        .any(|operation| operation != &KeyOperations::Verify)
            })
    {
        return Err(OidcVerificationError::InvalidKey);
    }
    let AlgorithmParameters::RSA(parameters) = &jwk.algorithm else {
        return Err(OidcVerificationError::InvalidKey);
    };
    if parameters.n.len() > MAX_RSA_MODULUS_ENCODED_BYTES
        || parameters.e.len() > MAX_RSA_EXPONENT_ENCODED_BYTES
    {
        return Err(OidcVerificationError::InvalidKey);
    }
    let modulus = URL_SAFE_NO_PAD
        .decode(parameters.n.as_bytes())
        .map_err(|_| OidcVerificationError::InvalidKey)?;
    let exponent = URL_SAFE_NO_PAD
        .decode(parameters.e.as_bytes())
        .map_err(|_| OidcVerificationError::InvalidKey)?;
    if !(MIN_RSA_MODULUS_BYTES..=MAX_RSA_MODULUS_BYTES).contains(&modulus.len())
        || modulus.first().is_none_or(|first| first & 0x80 == 0)
        || exponent != RSA_F4_EXPONENT
    {
        return Err(OidcVerificationError::InvalidKey);
    }
    Ok(key_id)
}

fn validate_claims(claims: &OidcClaims, now: u64) -> Result<(), OidcVerificationError> {
    if !valid_identifier(&claims.sub, 1, MAX_SUBJECT_BYTES)
        || !valid_identifier(&claims.jti, MIN_JTI_BYTES, MAX_JTI_BYTES)
        || claims.iat > now.saturating_add(OIDC_CLOCK_SKEW_SECONDS)
        || claims.exp < claims.iat
        || claims.exp.saturating_sub(claims.iat) > MAX_OIDC_TOKEN_LIFETIME_SECONDS
    {
        return Err(OidcVerificationError::InvalidClaims);
    }
    Ok(())
}

fn valid_identifier(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.len())
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn principal_from_subject(issuer: &str, subject: &str) -> AuthenticatedPrincipal {
    let mut context = Context::new(&SHA256);
    context.update(PRINCIPAL_DOMAIN);
    context.update(
        &u64::try_from(issuer.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    context.update(issuer.as_bytes());
    context.update(subject.as_bytes());
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(context.finish().as_ref());
    AuthenticatedPrincipal::from_digest(digest)
}

fn unix_seconds() -> Result<u64, OidcVerificationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| OidcVerificationError::ClockUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SampleWindow;
    use base64::engine::general_purpose::STANDARD;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;
    use serde_json::{Value, json};

    const ISSUER: &str = "https://identity.example.test/realms/game";
    const AUDIENCE: &str = "destructible-fps";
    const KEY_ID: &str = "test-key-2026";
    // Published jsonwebtoken RSA fixture, used only to produce deterministic local test tokens.
    const TEST_RSA_PRIVATE_DER_BASE64: &str = concat!(
        "MIIEpAIBAAKCAQEAyRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTLUTv4l4sg",
        "gh5/CYYi/cvI+SXVT9kPWSKXxJXBXd/4LkvcPuUakBoAkfh+eiFVMh2VrUyWyj3MFl0HTVF9K",
        "wRXLAcwkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8HoGfG/AtH89BIE9jDBHZ9dLelK9",
        "a184zAf8LwoPLxvJb3Il5nncqPcSfKDDodMFBIMc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5x",
        "qi+yUod+j8MtvIj812dkS4QMiRVN/by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQIDAQAB",
        "AoIBAHREk0I0O9DvECKdWUpAmF3mY7oY9PNQiu44Yaf+AoSuyRpRUGTMIgc3u3eivOE8ALX0Bm",
        "YUO5JtuRNZDpvt4SAwqCnVUinIf6C+eH/wSurCpapSM0BAHp4aOA7igptyOMgMPYBHNA1e9A7j",
        "E0dCxKWMl3DSWNyjQTk4zeRGEAEfbNjHrq6YCtjHSZSLmWiG80hnfnYos9hOr5JnLnyS7ZmFE/",
        "5P3XVrxLc/tQ5zum0R4cbrgzHiQP5RgfxGJaEi7XcgherCCOgurJSSbYH29Gz8u5fFbS+Yg8s+",
        "OiCss3cs1rSgJ9/eHZuzGEdUZVARH6hVMjSuwvqVTFaE8AgtleECgYEA+uLMn4kNqHlJS2A5u",
        "AnCkj90ZxEtNm3E8hAxUrhssktY5XSOAPBlxyf5RuRGIImGtUVIr4HuJSa5TX48n3Vdt9MYCpr",
        "O/iYl6moNRSPt5qowIIOJmIjY2mqPDfDt/zw+fcDD3lmCJrFlzcnh0uea1CohxEbQnL3cypeLt",
        "+WbU6kCgYEAzSp19m1ajieFkqgoB0YTpt/OroDx38vvI5unInJlEeOjQ+oIAQdN2wpxBvTrRor",
        "MU6P07mFUbt1j+Co6CbNiw+X8HcCaqYLR5clbJOOWNR36PuzOpQLkfK8woupBxzW9B8gZmY8rB",
        "1mbJ+/WTPrEJy6YGmIEBkWylQ2VpW8O4O0CgYEApdbvvfFBlwD9YxbrcGz7MeNCFbMz+MucqQn",
        "tIKoKJ91ImPxvtc0y6e/Rhnv0oyNlaUOwJVu0yNgNG117w0g4t/+Q38mvVC5xV7/cn7x9UMFk6",
        "MkqVir3dYGEqIl/OP1grY2Tq9HtB5iyG9L8NIamQOLMyUqqMUILxdthHyFmiGkCgYEAn9+PjpjG",
        "MPHxL0gj8Q8VbzsFtou6b1deIRRA2CHmSltltR1gYVTMwXxQeUhPMmgkMqUXzs4/WijgpthY44h",
        "K1TaZEKIuoxrS70nJ4WQLf5a9k1065fDsFZD6yGjdGxvwEmlGMZgTwqV7t1I4X0Ilqhav5hcs5",
        "apYL7gnPYPeRz0CgYALHCj/Ji8XSsDoF/MhVhnGdIs2P99NNdmo3R2Pv0CuZbDKMU559LJHUvrK",
        "S8WkuWRDuKrz1W/EQKApFjDGpdqToZqriUFQzwy7mR3ayIiogzNtHcvbDHx8oFnGY0OFksX/ye",
        "0/XGpy2SFxYRwGU98HPYeBvAQQrVjdkzfy7BmXQQ=="
    );
    const TEST_RSA_PUBLIC_DER_BASE64: &str = concat!(
        "MIIBCgKCAQEAyRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTLUTv4l4sggh5/C",
        "YYi/cvI+SXVT9kPWSKXxJXBXd/4LkvcPuUakBoAkfh+eiFVMh2VrUyWyj3MFl0HTVF9KwRXLAc",
        "wkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8HoGfG/AtH89BIE9jDBHZ9dLelK9a184zAf",
        "8LwoPLxvJb3Il5nncqPcSfKDDodMFBIMc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5xqi+yUod+",
        "j8MtvIj812dkS4QMiRVN/by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQIDAQAB"
    );

    struct TestIdentity {
        encoding_key: EncodingKey,
        jwks: Vec<u8>,
    }

    #[derive(Clone, Copy, Serialize)]
    struct TestClaims<'a> {
        iss: &'a str,
        aud: &'a str,
        sub: &'a str,
        exp: u64,
        iat: u64,
        jti: &'a str,
    }

    #[test]
    fn valid_token_maps_subject_and_replay_is_rejected() {
        let identity = test_identity(KEY_ID);
        let verifier =
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &identity.jwks).expect("valid verifier");
        let now = unix_seconds().expect("test clock");
        let token = signed_token(
            &identity.encoding_key,
            KEY_ID,
            TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "4cfd1c5a-d338-4bc6-a6c7-04f58f56738d",
                exp: now + 120,
                iat: now,
                jti: "valid-jti-0001",
            },
        );

        assert_eq!(
            verifier.verify_oidc(token.as_bytes()),
            Ok(principal_from_subject(
                ISSUER,
                "4cfd1c5a-d338-4bc6-a6c7-04f58f56738d"
            ))
        );
        assert_eq!(verifier.replay_entries(), Some(1));
        assert_eq!(
            verifier.verify_oidc(token.as_bytes()),
            Err(OidcVerificationError::ReplayedCredential)
        );
    }

    #[test]
    fn algorithm_and_key_substitution_fail_closed() {
        let identity = test_identity(KEY_ID);
        let verifier =
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &identity.jwks).expect("valid verifier");
        let now = unix_seconds().expect("test clock");
        let wrong_audience = signed_token(
            &identity.encoding_key,
            KEY_ID,
            TestClaims {
                iss: ISSUER,
                aud: "another-game",
                sub: "player-1",
                exp: now + 120,
                iat: now,
                jti: "wrong-audience-1",
            },
        );
        let unknown_key = signed_token(
            &identity.encoding_key,
            "unknown-key",
            TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "player-1",
                exp: now + 120,
                iat: now,
                jti: "unknown-key-jti",
            },
        );
        let mut hmac_header = Header::new(Algorithm::HS256);
        hmac_header.kid = Some(KEY_ID.to_owned());
        let hmac_token = encode(
            &hmac_header,
            &TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "player-1",
                exp: now + 120,
                iat: now,
                jti: "algorithm-jti-1",
            },
            &EncodingKey::from_secret(b"not-an-rsa-key"),
        )
        .expect("test HMAC token");
        assert_eq!(
            verifier.verify_oidc(wrong_audience.as_bytes()),
            Err(OidcVerificationError::InvalidCredential)
        );
        assert_eq!(
            verifier.verify_oidc(unknown_key.as_bytes()),
            Err(OidcVerificationError::UnknownKey)
        );
        assert_eq!(
            verifier.verify_oidc(hmac_token.as_bytes()),
            Err(OidcVerificationError::UnsupportedHeader)
        );
        assert_eq!(verifier.replay_entries(), Some(0));
    }

    #[test]
    fn invalid_temporal_and_identity_claims_fail_closed() {
        let identity = test_identity(KEY_ID);
        let verifier =
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &identity.jwks).expect("valid verifier");
        let now = unix_seconds().expect("test clock");
        let cases = [
            (
                TestClaims {
                    iss: "https://identity.example.test/realms/other",
                    aud: AUDIENCE,
                    sub: "player-1",
                    exp: now + 120,
                    iat: now,
                    jti: "wrong-issuer-jti",
                },
                OidcVerificationError::InvalidCredential,
            ),
            (
                TestClaims {
                    iss: ISSUER,
                    aud: AUDIENCE,
                    sub: "player-1",
                    exp: now.saturating_sub(OIDC_CLOCK_SKEW_SECONDS + 1),
                    iat: now.saturating_sub(120),
                    jti: "expired-token-jti",
                },
                OidcVerificationError::InvalidCredential,
            ),
            (
                TestClaims {
                    iss: ISSUER,
                    aud: AUDIENCE,
                    sub: "player-1",
                    exp: now + MAX_OIDC_TOKEN_LIFETIME_SECONDS + 1,
                    iat: now,
                    jti: "long-lifetime-jti",
                },
                OidcVerificationError::InvalidClaims,
            ),
        ];
        for (index, (claims, expected)) in cases.into_iter().enumerate() {
            let token = signed_token(&identity.encoding_key, KEY_ID, claims);
            assert_eq!(
                verifier.verify_oidc(token.as_bytes()),
                Err(expected),
                "invalid claim case {index}"
            );
        }
        let valid = signed_token(
            &identity.encoding_key,
            KEY_ID,
            TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "player-1",
                exp: now + 120,
                iat: now,
                jti: "tampered-token-jti",
            },
        );
        let mut tampered = valid.into_bytes();
        let last = tampered.last_mut().expect("non-empty signed token");
        *last = if *last == b'A' { b'B' } else { b'A' };
        assert_eq!(
            verifier.verify_oidc(&tampered),
            Err(OidcVerificationError::InvalidCredential)
        );
        assert_eq!(verifier.replay_entries(), Some(0));
    }

    #[test]
    fn repeated_oidc_verification_latency_is_measured_separately() {
        const ITERATIONS: usize = 100;
        let identity = test_identity(KEY_ID);
        let verifier =
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &identity.jwks).expect("valid verifier");
        let now = unix_seconds().expect("test clock");
        let tokens = (0..ITERATIONS)
            .map(|iteration| {
                let jti = format!("measured-jti-{iteration:04}");
                signed_token(
                    &identity.encoding_key,
                    KEY_ID,
                    TestClaims {
                        iss: ISSUER,
                        aud: AUDIENCE,
                        sub: "measured-player",
                        exp: now + 120,
                        iat: now,
                        jti: &jti,
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut samples = SampleWindow::new(ITERATIONS);
        for token in &tokens {
            let started = std::time::Instant::now();
            verifier
                .verify_oidc(token.as_bytes())
                .expect("measured token verification");
            samples.record_ms(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let summary = samples.summary().expect("OIDC verification samples");
        assert_eq!(summary.samples, ITERATIONS);
        assert_eq!(verifier.replay_entries(), Some(ITERATIONS));
        eprintln!(
            "OIDC RS256 verification: p50={:.3}ms p95={:.3}ms p99={:.3}ms max={:.3}ms",
            summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
        );
    }

    #[test]
    fn bounded_key_rotation_is_atomic() {
        let identity = test_identity(KEY_ID);
        let verifier =
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &identity.jwks).expect("valid verifier");
        assert!(verifier.replace_jwks(b"{}").is_err());
        let now = unix_seconds().expect("test clock");
        let before_rotation = signed_token(
            &identity.encoding_key,
            KEY_ID,
            TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "player-2",
                exp: now + 120,
                iat: now,
                jti: "before-rotation-1",
            },
        );
        assert!(verifier.verify_oidc(before_rotation.as_bytes()).is_ok());
        let mut replacement: Value =
            serde_json::from_slice(&identity.jwks).expect("test JWKS value");
        replacement["keys"][0]["kid"] = Value::String("rotated-key".to_owned());
        let replacement = serde_json::to_vec(&replacement).expect("replacement JWKS");
        assert_eq!(verifier.replace_jwks(&replacement), Ok(1));

        let old_key = signed_token(
            &identity.encoding_key,
            KEY_ID,
            TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "player-2",
                exp: now + 120,
                iat: now,
                jti: "old-key-jti-01",
            },
        );
        let new_key = signed_token(
            &identity.encoding_key,
            "rotated-key",
            TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "player-2",
                exp: now + 120,
                iat: now,
                jti: "new-key-jti-01",
            },
        );
        assert_eq!(
            verifier.verify_oidc(old_key.as_bytes()),
            Err(OidcVerificationError::UnknownKey)
        );
        assert!(verifier.verify_oidc(new_key.as_bytes()).is_ok());
    }

    #[test]
    fn malformed_or_weak_key_sets_are_rejected_before_crypto_work() {
        let identity = test_identity(KEY_ID);
        let mut duplicate: Value = serde_json::from_slice(&identity.jwks).expect("test JWKS value");
        let key = duplicate["keys"][0].clone();
        duplicate["keys"]
            .as_array_mut()
            .expect("test key array")
            .push(key);
        let duplicate = serde_json::to_vec(&duplicate).expect("duplicate JWKS");
        let source_key = serde_json::from_slice::<Value>(&identity.jwks).expect("test JWKS value")
            ["keys"][0]
            .clone();
        let oversized_keys = (0..=MAX_OIDC_JWKS_KEYS)
            .map(|index| {
                let mut key = source_key.clone();
                key["kid"] = Value::String(format!("bounded-key-{index}"));
                key
            })
            .collect::<Vec<_>>();
        let oversized_keys =
            serde_json::to_vec(&json!({ "keys": oversized_keys })).expect("large key set");
        let weak = serde_json::to_vec(&json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": "weak-key",
                "n": "AQAB",
                "e": "AQAB"
            }]
        }))
        .expect("weak JWKS");

        assert!(matches!(
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &duplicate),
            Err(OidcVerificationError::DuplicateKeyId)
        ));
        assert!(matches!(
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &weak),
            Err(OidcVerificationError::InvalidKey)
        ));
        assert!(matches!(
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &oversized_keys),
            Err(OidcVerificationError::InvalidKeyCount(_))
        ));
        assert!(matches!(
            OidcSessionVerifier::new(ISSUER, AUDIENCE, &vec![b' '; MAX_OIDC_JWKS_BYTES + 1]),
            Err(OidcVerificationError::JwksOversized(_))
        ));
        assert!(matches!(
            OidcSessionVerifier::new("http://identity.test", AUDIENCE, &identity.jwks),
            Err(OidcVerificationError::InvalidConfiguration)
        ));
    }

    fn signed_token(claims_key: &EncodingKey, key_id: &str, claims: TestClaims<'_>) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(key_id.to_owned());
        encode(&header, &claims, claims_key).expect("signed test token")
    }

    fn test_identity(key_id: &str) -> TestIdentity {
        let private_der = STANDARD
            .decode(TEST_RSA_PRIVATE_DER_BASE64)
            .expect("published test private key");
        let public_der = STANDARD
            .decode(TEST_RSA_PUBLIC_DER_BASE64)
            .expect("published test public key");
        let encoding_key = EncodingKey::from_rsa_der(&private_der);
        let decoding_key = DecodingKey::from_rsa_der(&public_der);
        let mut jwk =
            Jwk::from_decoding_key(&decoding_key, Some(Algorithm::RS256)).expect("test public JWK");
        jwk.common.key_id = Some(key_id.to_owned());
        jwk.common.key_algorithm = Some(KeyAlgorithm::RS256);
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);
        jwk.common.key_operations = Some(vec![KeyOperations::Verify]);
        let jwks = serde_json::to_vec(&JwkSet { keys: vec![jwk] }).expect("test JWKS");
        TestIdentity { encoding_key, jwks }
    }
}
