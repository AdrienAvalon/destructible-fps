use destructible_fps::{LAN_POLICY_SCHEMA_VERSION, LanDeploymentPolicy};
use std::{
    env,
    error::Error,
    io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> Result<(), Box<dyn Error>> {
    let path = parse_path()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let policy = LanDeploymentPolicy::load(path, now)?;
    let remaining = policy.expires_at_unix_seconds().saturating_sub(now);
    println!(
        "POLICY_OK schema={LAN_POLICY_SCHEMA_VERSION} sources={} expires_in_seconds={remaining}",
        policy.firewall_source_cidrs().len()
    );
    Ok(())
}

fn parse_path() -> Result<PathBuf, io::Error> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let path = arguments.next().map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: lan-policy-check /absolute/path/to/policy.json",
        )
    })?;
    if arguments.next().is_some() || !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "exactly one absolute policy path is required",
        ));
    }
    Ok(path)
}
