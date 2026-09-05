use destructible_fps::{
    LAN_POLICY_SCHEMA_VERSION, LanDeploymentPolicy, attest_lan_policy_on_current_host,
    attest_lan_server_certificate,
};
use std::{
    env,
    error::Error,
    ffi::OsString,
    io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let policy = LanDeploymentPolicy::load(options.path, now)?;
    let host_verified = if options.verify_host {
        let _attestation = attest_lan_policy_on_current_host(&policy)?;
        true
    } else {
        false
    };
    let certificate_verified = if let (Some(certificate_chain), Some(trust_anchor)) =
        (options.certificate_chain, options.trust_anchor)
    {
        let _attestation =
            attest_lan_server_certificate(&policy, certificate_chain, trust_anchor, now)?;
        true
    } else {
        false
    };
    let remaining = policy.expires_at_unix_seconds().saturating_sub(now);
    println!(
        "POLICY_OK schema={LAN_POLICY_SCHEMA_VERSION} sources={} expires_in_seconds={remaining} host_verified={host_verified} certificate_verified={certificate_verified}",
        policy.firewall_source_cidrs().len()
    );
    Ok(())
}

struct Options {
    path: PathBuf,
    verify_host: bool,
    certificate_chain: Option<PathBuf>,
    trust_anchor: Option<PathBuf>,
}

fn parse_options() -> Result<Options, io::Error> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let mut path = None;
    let mut verify_host = false;
    let mut certificate_chain = None;
    let mut trust_anchor = None;
    while let Some(argument) = arguments.next() {
        if argument == "--verify-host" {
            if verify_host {
                return Err(invalid_usage());
            }
            verify_host = true;
        } else if argument == "--certificate-chain" {
            set_path_option(&mut certificate_chain, arguments.next())?;
        } else if argument == "--trust-anchor" {
            set_path_option(&mut trust_anchor, arguments.next())?;
        } else if argument.to_string_lossy().starts_with('-') || path.is_some() {
            return Err(invalid_usage());
        } else {
            path = Some(PathBuf::from(argument));
        }
    }
    let path = path.ok_or_else(invalid_usage)?;
    if !path.is_absolute()
        || certificate_chain
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        || trust_anchor
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        || certificate_chain.is_some() != trust_anchor.is_some()
    {
        return Err(invalid_usage());
    }
    Ok(Options {
        path,
        verify_host,
        certificate_chain,
        trust_anchor,
    })
}

fn set_path_option(
    destination: &mut Option<PathBuf>,
    value: Option<OsString>,
) -> Result<(), io::Error> {
    if destination.is_some() {
        return Err(invalid_usage());
    }
    *destination = Some(PathBuf::from(value.ok_or_else(invalid_usage)?));
    Ok(())
}

fn invalid_usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: lan-policy-check [--verify-host] [--certificate-chain /absolute/chain.pem --trust-anchor /absolute/root.pem] /absolute/policy.json",
    )
}
