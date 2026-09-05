use destructible_fps::{
    OidcRefreshController, SERVER_PHYSICS_HZ, SecureAuthorityLaunchConfig,
    SecureAuthorityLaunchError, SecureNetworkTickReport, demo_world,
};
use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

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

#[derive(Default)]
struct RefreshCounters {
    attempts: AtomicU64,
    successes: AtomicU64,
    failures: AtomicU64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let launch = SecureAuthorityLaunchConfig::load(options.config)?;
    let refresh_controller = launch.oidc_refresh_controller();
    let refresh_counters = Arc::new(RefreshCounters::default());
    refresh_before_ready(refresh_controller.as_ref(), &refresh_counters).await?;
    let max_ticks = launch.max_ticks().map(std::num::NonZeroU64::get);
    let stop_after_commands = launch
        .stop_after_commands()
        .map(std::num::NonZeroUsize::get);
    let initial_jwks_expiration_deadline = launch
        .jwks_expiration_deadline()
        .ok_or("OIDC expiration state unavailable")?;
    let certificate_expiration_deadline =
        tokio::time::Instant::from_std(launch.certificate_expiration_deadline());
    let exposure = launch.exposure();
    let mut server = launch.start(demo_world())?;
    let refresh_task = refresh_controller
        .as_ref()
        .map(|controller| spawn_oidc_refresh(controller.clone(), Arc::clone(&refresh_counters)));
    println!(
        "READY {} exposure={exposure:?} oidc_refresh={}",
        server.local_addr()?,
        if refresh_controller.is_some() {
            "active"
        } else {
            "static"
        }
    );
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
                let jwks_expiration_deadline = refresh_controller.as_ref().map_or(
                    Some(initial_jwks_expiration_deadline),
                    OidcRefreshController::expiration_deadline,
                );
                let Some(jwks_expiration_deadline) = jwks_expiration_deadline else {
                    terminal_error = Some("OIDC expiration state unavailable".into());
                    break;
                };
                if std::time::Instant::now() >= jwks_expiration_deadline {
                    terminal_error = Some("OIDC JWKS validity expired".into());
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
    if let Some(task) = refresh_task {
        task.abort();
        let _ = task.await;
    }
    println!(
        "STOP ticks={} commands={} admitted={} disconnected={} admission_failures={} rate_limited={} active={} inbound={} outbound={} oidc_refresh_attempts={} oidc_refresh_successes={} oidc_refresh_failures={}",
        totals.ticks,
        totals.commands,
        totals.admitted,
        totals.disconnected,
        totals.admission_failures,
        totals.rate_limited,
        active,
        totals.inbound,
        totals.outbound,
        refresh_counters.attempts.load(Ordering::Relaxed),
        refresh_counters.successes.load(Ordering::Relaxed),
        refresh_counters.failures.load(Ordering::Relaxed),
    );
    terminal_error.map_or(Ok(()), Err)
}

async fn refresh_before_ready(
    controller: Option<&OidcRefreshController>,
    counters: &RefreshCounters,
) -> Result<(), SecureAuthorityLaunchError> {
    if let Some(controller) = controller {
        counters.attempts.store(1, Ordering::Relaxed);
        controller.refresh_once().await?;
        counters.successes.store(1, Ordering::Relaxed);
    }
    Ok(())
}

fn spawn_oidc_refresh(
    controller: OidcRefreshController,
    counters: Arc<RefreshCounters>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(controller.refresh_interval());
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            counters.attempts.fetch_add(1, Ordering::Relaxed);
            if controller.refresh_once().await.is_ok() {
                counters.successes.fetch_add(1, Ordering::Relaxed);
            } else {
                let failures = counters.failures.fetch_add(1, Ordering::Relaxed) + 1;
                eprintln!("OIDC_REFRESH_FAILED failures={failures}");
            }
        }
    })
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
