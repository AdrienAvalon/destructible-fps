use destructible_fps::{DedicatedServer, SERVER_PHYSICS_HZ, demo_world};
use std::{
    error::Error,
    io::{self, Write},
    net::SocketAddr,
    thread,
    time::{Duration, Instant},
};

const DEFAULT_BIND: &str = "127.0.0.1:40000";

struct Options {
    bind: SocketAddr,
    max_ticks: u64,
    exit_after_commands: Option<usize>,
    exit_after_repairs: Option<usize>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let mut server = DedicatedServer::bind(options.bind, demo_world())?;
    println!("READY {}", server.local_addr()?);
    io::stdout().flush()?;

    let tick_duration = Duration::from_nanos(
        1_000_000_000_u64 / u64::try_from(SERVER_PHYSICS_HZ).expect("positive fixed rate"),
    );
    let mut deadline = Instant::now();
    let mut applied_commands = 0_usize;
    let mut served_repairs = 0_usize;
    let mut ticks = 0_u64;
    let mut inbound = 0_usize;
    let mut outbound = 0_usize;
    while ticks < options.max_ticks {
        let report = server.tick()?;
        ticks += 1;
        applied_commands = applied_commands.saturating_add(report.commands_applied);
        served_repairs = served_repairs.saturating_add(report.repairs_served);
        inbound = inbound.saturating_add(report.received_datagrams);
        outbound = outbound.saturating_add(report.outbound_datagrams);
        if options
            .exit_after_commands
            .is_some_and(|target| applied_commands >= target)
            || options
                .exit_after_repairs
                .is_some_and(|target| served_repairs >= target)
        {
            break;
        }
        deadline += tick_duration;
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        } else {
            deadline = now;
        }
    }
    println!(
        "STOP ticks={ticks} commands={applied_commands} repairs={served_repairs} peers={} inbound={inbound} outbound={outbound}",
        server.peer_count()
    );
    Ok(())
}

fn parse_options() -> Result<Options, Box<dyn Error>> {
    let mut bind = DEFAULT_BIND.parse::<SocketAddr>()?;
    let mut max_ticks = u64::MAX;
    let mut exit_after_commands = None;
    let mut exit_after_repairs = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bind" => {
                bind = arguments
                    .next()
                    .ok_or("--bind requires an address")?
                    .parse()?;
            }
            "--max-ticks" => {
                max_ticks = arguments
                    .next()
                    .ok_or("--max-ticks requires a value")?
                    .parse()?;
            }
            "--exit-after-commands" => {
                exit_after_commands = Some(
                    arguments
                        .next()
                        .ok_or("--exit-after-commands requires a value")?
                        .parse()?,
                );
            }
            "--exit-after-repairs" => {
                exit_after_repairs = Some(
                    arguments
                        .next()
                        .ok_or("--exit-after-repairs requires a value")?
                        .parse()?,
                );
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if !bind.ip().is_loopback() {
        return Err("this unauthenticated milestone only permits an explicit loopback bind".into());
    }
    if max_ticks == 0 {
        return Err("--max-ticks must be greater than zero".into());
    }
    if exit_after_commands == Some(0) {
        return Err("--exit-after-commands must be greater than zero".into());
    }
    if exit_after_repairs == Some(0) {
        return Err("--exit-after-repairs must be greater than zero".into());
    }
    Ok(Options {
        bind,
        max_ticks,
        exit_after_commands,
        exit_after_repairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bind_is_loopback() {
        assert!(
            DEFAULT_BIND
                .parse::<SocketAddr>()
                .expect("address")
                .ip()
                .is_loopback()
        );
    }
}
