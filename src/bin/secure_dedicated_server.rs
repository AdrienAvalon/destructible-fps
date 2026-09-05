use destructible_fps::{
    SERVER_PHYSICS_HZ, SecureAuthorityLaunchConfig, SecureNetworkTickReport, demo_world,
};
use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};
use tokio::time::{MissedTickBehavior, interval};

struct Options {
    config: PathBuf,
}

#[derive(Default)]
struct Totals {
    ticks: u64,
    commands: usize,
    inbound: usize,
    outbound: usize,
    admitted: usize,
    disconnected: usize,
    admission_failures: usize,
    rate_limited: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let launch = SecureAuthorityLaunchConfig::load(options.config)?;
    let max_ticks = launch.max_ticks().map(std::num::NonZeroU64::get);
    let stop_after_commands = launch
        .stop_after_commands()
        .map(std::num::NonZeroUsize::get);
    let jwks_expiration_deadline =
        tokio::time::Instant::from_std(launch.jwks_expiration_deadline());
    let certificate_expiration_deadline =
        tokio::time::Instant::from_std(launch.certificate_expiration_deadline());
    let exposure = launch.exposure();
    let mut server = launch.start(demo_world())?;
    println!("READY {} exposure={exposure:?}", server.local_addr()?);
    io::stdout().flush()?;

    let tick_duration = Duration::from_nanos(
        1_000_000_000_u64 / u64::try_from(SERVER_PHYSICS_HZ).expect("positive fixed rate"),
    );
    let mut ticker = interval(tick_duration);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut totals = Totals::default();
    let mut terminal_error: Option<Box<dyn Error>> = None;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if tokio::time::Instant::now() >= certificate_expiration_deadline {
                    terminal_error = Some("TLS certificate validity expired".into());
                    break;
                }
                if tokio::time::Instant::now() >= jwks_expiration_deadline {
                    terminal_error = Some("static JWKS validity expired".into());
                    break;
                }
                let report = match server.tick() {
                    Ok(report) => report,
                    Err(error) => {
                        terminal_error = Some(Box::new(error));
                        break;
                    }
                };
                totals.record(&report);
                if max_ticks.is_some_and(|maximum| totals.ticks >= maximum)
                    || stop_after_commands
                        .is_some_and(|maximum| totals.commands >= maximum)
                {
                    break;
                }
            }
            signal = tokio::signal::ctrl_c() => {
                if let Err(error) = signal {
                    terminal_error = Some(Box::new(error));
                }
                break;
            }
        }
    }
    let active = server.active_sessions();
    server.shutdown().await;
    println!(
        "STOP ticks={} commands={} admitted={} disconnected={} admission_failures={} rate_limited={} active={} inbound={} outbound={}",
        totals.ticks,
        totals.commands,
        totals.admitted,
        totals.disconnected,
        totals.admission_failures,
        totals.rate_limited,
        active,
        totals.inbound,
        totals.outbound,
    );
    terminal_error.map_or(Ok(()), Err)
}

impl Totals {
    const fn record(&mut self, report: &SecureNetworkTickReport) {
        self.ticks = self.ticks.saturating_add(1);
        self.commands = self
            .commands
            .saturating_add(report.authority.commands_applied);
        self.inbound = self
            .inbound
            .saturating_add(report.authority.received_datagrams);
        self.outbound = self
            .outbound
            .saturating_add(report.authority.outbound_datagrams);
        self.admitted = self.admitted.saturating_add(report.admitted_sessions);
        self.disconnected = self
            .disconnected
            .saturating_add(report.disconnected_sessions);
        self.admission_failures = self
            .admission_failures
            .saturating_add(report.admission_failures);
        self.rate_limited = self
            .rate_limited
            .saturating_add(report.rate_limited_sessions);
    }
}

fn parse_options() -> Result<Options, Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--config")) {
        return Err("usage: secure-dedicated-server --config <json-file>".into());
    }
    let config = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("--config requires a file")?;
    if arguments.next().is_some() {
        return Err("unexpected extra arguments".into());
    }
    Ok(Options { config })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_config_path_is_accepted() {
        assert_eq!(
            std::mem::size_of::<Options>(),
            std::mem::size_of::<PathBuf>()
        );
    }
}
