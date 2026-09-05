use destructible_fps::{
    LAN_POLICY_SCHEMA_VERSION, LanDeploymentPolicy, attest_lan_policy_on_current_host,
};
use std::{
    env,
    error::Error,
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
    let remaining = policy.expires_at_unix_seconds().saturating_sub(now);
    println!(
        "POLICY_OK schema={LAN_POLICY_SCHEMA_VERSION} sources={} expires_in_seconds={remaining} host_verified={host_verified}",
        policy.firewall_source_cidrs().len()
    );
    Ok(())
}

struct Options {
    path: PathBuf,
    verify_host: bool,
}

fn parse_options() -> Result<Options, io::Error> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let first = arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: lan-policy-check [--verify-host] /absolute/path/to/policy.json",
        )
    })?;
    let (path, verify_host) = if first == "--verify-host" {
        let path = arguments.next().map(PathBuf::from).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--verify-host requires one absolute policy path",
            )
        })?;
        (path, true)
    } else {
        (PathBuf::from(first), false)
    };
    if arguments.next().is_some() || !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "exactly one absolute policy path is required",
        ));
    }
    Ok(Options { path, verify_host })
}
