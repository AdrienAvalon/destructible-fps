use destructible_fps::{
    OidcRefreshController, SERVER_PHYSICS_HZ, SecureAuthorityLaunchConfig,
    SecureAuthorityLaunchError, SecureDedicatedServer, SecureNetworkExposure,
    SecureNetworkTickReport, SecureTlsConfigUpdater, TlsIdentityRefreshController,
    TlsIdentityRefreshOutcome, demo_world,
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
    installed: AtomicU64,
    unchanged: AtomicU64,
}

struct RefreshTasks {
    oidc: Option<JoinHandle<()>>,
    tls: Option<JoinHandle<()>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let launch = SecureAuthorityLaunchConfig::load(options.config)?;
    let oidc_refresh = launch.oidc_refresh_controller();
    let tls_refresh = launch.tls_refresh_controller();
    let oidc_counters = Arc::new(RefreshCounters::default());
    let tls_counters = Arc::new(RefreshCounters::default());
    refresh_before_ready(oidc_refresh.as_ref(), &oidc_counters).await?;
    let max_ticks = launch.max_ticks().map(std::num::NonZeroU64::get);
    let stop_after_commands = launch
        .stop_after_commands()
        .map(std::num::NonZeroUsize::get);
    let initial_jwks_expiration_deadline = launch
        .jwks_expiration_deadline()
        .ok_or("OIDC expiration state unavailable")?;
    let initial_certificate_safety_deadline = launch
        .certificate_safety_deadline()
        .ok_or("TLS safety state unavailable")?;
    let exposure = launch.exposure();
    let mut server = launch.start(demo_world())?;
    let refresh_tasks = spawn_refresh_tasks(
        &server,
        oidc_refresh.as_ref(),
        tls_refresh.as_ref(),
        &oidc_counters,
        &tls_counters,
    );
    print_ready(
        &server,
        exposure,
        oidc_refresh.is_some(),
        tls_refresh.is_some(),
    )?;

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
                if let Some(error) = trust_deadline_error(
                    oidc_refresh.as_ref(),
                    tls_refresh.as_ref(),
                    initial_jwks_expiration_deadline,
                    initial_certificate_safety_deadline,
                ) {
                    terminal_error = Some(error.into());
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
    refresh_tasks.stop().await;
    server.shutdown().await;
    println!(
        "STOP ticks={} commands={} admitted={} disconnected={} admission_failures={} rate_limited={} active={} inbound={} outbound={} oidc_refresh_attempts={} oidc_refresh_successes={} oidc_refresh_failures={} tls_reload_attempts={} tls_reload_successes={} tls_reload_failures={} tls_reload_installed={} tls_reload_unchanged={}",
        totals.ticks,
        totals.commands,
        totals.admitted,
        totals.disconnected,
        totals.admission_failures,
        totals.rate_limited,
        active,
        totals.inbound,
        totals.outbound,
        oidc_counters.attempts.load(Ordering::Relaxed),
        oidc_counters.successes.load(Ordering::Relaxed),
        oidc_counters.failures.load(Ordering::Relaxed),
        tls_counters.attempts.load(Ordering::Relaxed),
        tls_counters.successes.load(Ordering::Relaxed),
        tls_counters.failures.load(Ordering::Relaxed),
        tls_counters.installed.load(Ordering::Relaxed),
        tls_counters.unchanged.load(Ordering::Relaxed),
    );
    terminal_error.map_or(Ok(()), Err)
}

fn print_ready(
    server: &SecureDedicatedServer,
    exposure: SecureNetworkExposure,
    oidc_refresh: bool,
    tls_reload: bool,
) -> io::Result<()> {
    println!(
        "READY {} exposure={exposure:?} oidc_refresh={} tls_reload={}",
        server.local_addr()?,
        if oidc_refresh { "active" } else { "static" },
        if tls_reload { "active" } else { "static" },
    );
    io::stdout().flush()
}

fn spawn_refresh_tasks(
    server: &SecureDedicatedServer,
    oidc: Option<&OidcRefreshController>,
    tls: Option<&TlsIdentityRefreshController>,
    oidc_counters: &Arc<RefreshCounters>,
    tls_counters: &Arc<RefreshCounters>,
) -> RefreshTasks {
    RefreshTasks {
        oidc: oidc
            .map(|controller| spawn_oidc_refresh(controller.clone(), Arc::clone(oidc_counters))),
        tls: tls.map(|controller| {
            spawn_tls_refresh(
                controller.clone(),
                server.tls_config_updater(),
                Arc::clone(tls_counters),
            )
        }),
    }
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

fn spawn_tls_refresh(
    controller: TlsIdentityRefreshController,
    updater: SecureTlsConfigUpdater,
    counters: Arc<RefreshCounters>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(controller.refresh_interval());
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            counters.attempts.fetch_add(1, Ordering::Relaxed);
            match controller.refresh_once(&updater) {
                Ok(outcome) => {
                    counters.successes.fetch_add(1, Ordering::Relaxed);
                    match outcome {
                        TlsIdentityRefreshOutcome::Installed => {
                            counters.installed.fetch_add(1, Ordering::Relaxed);
                        }
                        TlsIdentityRefreshOutcome::Unchanged => {
                            counters.unchanged.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(_error) => {
                    let failures = counters.failures.fetch_add(1, Ordering::Relaxed) + 1;
                    eprintln!("TLS_RELOAD_FAILED failures={failures}");
                }
            }
        }
    })
}

fn trust_deadline_error(
    oidc_refresh: Option<&OidcRefreshController>,
    tls_refresh: Option<&TlsIdentityRefreshController>,
    initial_jwks_deadline: std::time::Instant,
    initial_certificate_safety_deadline: std::time::Instant,
) -> Option<&'static str> {
    let now = std::time::Instant::now();
    let certificate_safety_deadline = tls_refresh.map_or(
        Some(initial_certificate_safety_deadline),
        TlsIdentityRefreshController::safety_deadline,
    );
    let Some(certificate_safety_deadline) = certificate_safety_deadline else {
        return Some("TLS safety state unavailable");
    };
    if now >= certificate_safety_deadline {
        return Some("TLS certificate renewal safety deadline expired");
    }
    let jwks_deadline = oidc_refresh.map_or(
        Some(initial_jwks_deadline),
        OidcRefreshController::expiration_deadline,
    );
    let Some(jwks_deadline) = jwks_deadline else {
        return Some("OIDC expiration state unavailable");
    };
    (now >= jwks_deadline).then_some("OIDC JWKS validity expired")
}

impl RefreshTasks {
    async fn stop(self) {
        for task in [self.oidc, self.tls].into_iter().flatten() {
            task.abort();
            let _ = task.await;
        }
    }
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
