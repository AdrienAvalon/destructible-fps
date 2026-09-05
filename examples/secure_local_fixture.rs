//! Generates short-lived loopback-only TLS/OIDC material for the graphical secure demo.
//!
//! This utility is deliberately an example target: its generated authority is for local validation,
//! never for LAN or Internet exposure.

use aws_lc_rs::{
    encoding::AsDer,
    rand::SystemRandom,
    rsa::{KeyPair as RsaKeyPair, KeySize},
    signature::{KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair as SigningRsaKeyPair},
};
use base64::Engine as _;
use jsonwebtoken::{
    Algorithm, DecodingKey, Header,
    jwk::{Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
};
use serde::Serialize;
use serde_json::json;
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const ISSUER: &str = "https://identity.example.test/realms/local-game";
const AUDIENCE: &str = "destructible-fps";
const KEY_ID: &str = "local-demo-key";
const SERVER_NAME: &str = "localhost";
const SERVER_ADDRESS: &str = "127.0.0.1:40001";
const FIXTURE_VALIDITY_SECONDS: u64 = 900;
const TOKEN_VALIDITY_SECONDS: u64 = 600;

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    aud: &'a str,
    sub: &'a str,
    exp: u64,
    iat: u64,
    jti: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let output = parse_output_directory()?;
    create_private_directory(&output)?;

    let certificate_path = output.join("server.pem");
    let private_key_path = output.join("server-key.pem");
    let jwks_path = output.join("jwks.json");
    let first_credential_path = output.join("access-token-player-1");
    let second_credential_path = output.join("access-token-player-2");
    let config_path = output.join("server.json");

    let identity = rcgen::generate_simple_self_signed(vec![SERVER_NAME.into()])?;
    write_owner_only(&certificate_path, identity.cert.pem().as_bytes())?;
    write_owner_only(
        &private_key_path,
        identity.signing_key.serialize_pem().as_bytes(),
    )?;

    let (signing_key, jwks) = oidc_material()?;
    write_owner_only(&jwks_path, &jwks)?;
    let now = unix_seconds()?;
    let first_token = signed_token(&signing_key, now, 1)?;
    let second_token = signed_token(&signing_key, now, 2)?;
    write_owner_only(&first_credential_path, first_token.as_bytes())?;
    write_owner_only(&second_credential_path, second_token.as_bytes())?;

    let config = json!({
        "bind": SERVER_ADDRESS,
        "exposure": "loopback",
        "certificate_chain_file": path_string(&certificate_path)?,
        "private_key_file": path_string(&private_key_path)?,
        "oidc_jwks_file": path_string(&jwks_path)?,
        "oidc_issuer": ISSUER,
        "oidc_audience": AUDIENCE,
        "jwks_valid_until_unix_seconds": now + FIXTURE_VALIDITY_SECONDS,
        "max_ticks": 36_000,
    });
    write_owner_only(&config_path, &serde_json::to_vec_pretty(&config)?)?;

    println!("Local secure fixture created for 10 minutes:");
    println!("  server config: {}", config_path.display());
    println!("  trusted root:  {}", certificate_path.display());
    println!("  player 1:      {}", first_credential_path.display());
    println!("  player 2:      {}", second_credential_path.display());
    println!("  address:       {SERVER_ADDRESS}");
    println!("  TLS name:      {SERVER_NAME}");
    println!("Delete the complete directory after the local validation run.");
    Ok(())
}

fn parse_output_directory() -> Result<PathBuf, Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: secure_local_fixture <new-absolute-output-directory>")?;
    if arguments.next().is_some() || !output.is_absolute() {
        return Err("usage: secure_local_fixture <new-absolute-output-directory>".into());
    }
    if output.exists() {
        return Err("output directory already exists".into());
    }
    Ok(output)
}

fn oidc_material() -> Result<(SigningRsaKeyPair, Vec<u8>), Box<dyn Error>> {
    let key_pair = RsaKeyPair::generate(KeySize::Rsa2048)?;
    let private_der = key_pair.as_der()?;
    let signing_key = SigningRsaKeyPair::from_pkcs8(private_der.as_ref())?;
    let decoding_key = DecodingKey::from_rsa_der(key_pair.public_key().as_ref());
    let mut jwk = Jwk::from_decoding_key(&decoding_key, Some(Algorithm::RS256))?;
    jwk.common.key_id = Some(KEY_ID.into());
    jwk.common.key_algorithm = Some(KeyAlgorithm::RS256);
    jwk.common.public_key_use = Some(PublicKeyUse::Signature);
    jwk.common.key_operations = Some(vec![KeyOperations::Verify]);
    Ok((
        signing_key,
        serde_json::to_vec(&JwkSet { keys: vec![jwk] })?,
    ))
}

fn signed_token(
    signing_key: &SigningRsaKeyPair,
    now: u64,
    player: u8,
) -> Result<String, Box<dyn Error>> {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KEY_ID.into());
    let claims = Claims {
        iss: ISSUER,
        aud: AUDIENCE,
        sub: if player == 1 {
            "local-demo-player-1"
        } else {
            "local-demo-player-2"
        },
        exp: now + TOKEN_VALIDITY_SECONDS,
        iat: now,
        jti: format!("local-{}-{now}-{player}", std::process::id()),
    };
    let encoded_header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let encoded_claims =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
    let message = format!("{encoded_header}.{encoded_claims}");
    let mut signature = vec![0_u8; signing_key.public_modulus_len()];
    signing_key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )?;
    Ok(format!(
        "{message}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn create_private_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir(path)?;
    set_owner_only_directory(path)?;
    Ok(())
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    fs::write(path, bytes)?;
    set_owner_only_file(path)?;
    Ok(())
}

fn unix_seconds() -> Result<u64, Box<dyn Error>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn path_string(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| "fixture path is not UTF-8".into())
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_file(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
