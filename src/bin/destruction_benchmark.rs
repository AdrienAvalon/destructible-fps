use destructible_fps::{
    AuthoritativeServer, ClientReplica, ClientStatus, ExplosionCommand, FrameAssembler, IVec3,
    decode_frame, demo_world, encode_frames,
};
use std::hint::black_box;
use std::time::{Duration, Instant};

const CLIENT_ID: u64 = 1;
const NETWORK_MTU: usize = 1_200;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let events = parse_events()?;
    let result = run_benchmark(events)?;
    print_result(&result);
    Ok(())
}

struct Harness {
    server: AuthoritativeServer,
    client: ClientReplica,
    assembler: FrameAssembler,
    random: XorShift64,
    changes: usize,
    fractured: usize,
    frames_sent: usize,
    wire_bytes: usize,
}

impl Harness {
    fn new(initial_world: destructible_fps::World) -> Self {
        Self {
            server: AuthoritativeServer::new(initial_world.clone()),
            client: ClientReplica::new(initial_world),
            assembler: FrameAssembler::default(),
            random: XorShift64::new(0x5eed_f00d_dead_beef),
            changes: 0,
            fractured: 0,
            frames_sent: 0,
            wire_bytes: 0,
        }
    }

    fn run_event(&mut self, event_index: usize) -> Result<Duration, Box<dyn std::error::Error>> {
        let command = ExplosionCommand {
            command_id: u64::try_from(event_index)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
            center: IVec3::new(
                self.random.range_i32(-24, 24),
                self.random.range_i32(-2, 16),
                self.random.range_i32(-20, 20),
            ),
            radius_voxels: self.random.range_u16(3, 8),
            peak_energy: self.random.range_u32(4_000, 24_000),
        };
        let event_start = Instant::now();
        let (packet, report) = self.server.execute_explosion(CLIENT_ID, command)?;
        let mut encoded = encode_frames(black_box(&packet), NETWORK_MTU)?;
        if event_index % 2 == 1 {
            encoded.reverse();
        }
        let mut reassembled = None;
        for bytes in encoded {
            self.wire_bytes += bytes.len();
            self.frames_sent += 1;
            let frame = decode_frame(black_box(&bytes))?;
            if let Some(packet) = self.assembler.push(frame)? {
                reassembled = Some(packet);
            }
        }
        let packet = reassembled.ok_or("all fragments arrived but no packet was assembled")?;
        if self.client.receive(&packet)? != ClientStatus::Applied {
            return Err("fresh packet was unexpectedly treated as a duplicate".into());
        }
        self.changes += report.changes.len();
        self.fractured += report.fractured_voxels;
        Ok(event_start.elapsed())
    }

    fn verify(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.server.world().fingerprint() != self.client.world().fingerprint() {
            return Err("authoritative server and client replica diverged".into());
        }
        if self.server.world().fingerprint() != self.server.world().recompute_fingerprint() {
            return Err("incremental server fingerprint differs from the reference scan".into());
        }
        Ok(())
    }
}

struct BenchmarkResult {
    events: usize,
    elapsed: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    changes: usize,
    fractured: usize,
    frames_sent: usize,
    wire_bytes: usize,
    initial_solids: usize,
    final_stats: destructible_fps::WorldStats,
}

fn run_benchmark(events: usize) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
    let initial_world = demo_world();
    let initial_stats = initial_world.stats();
    let mut harness = Harness::new(initial_world);
    let mut durations = Vec::with_capacity(events);
    let benchmark_start = Instant::now();
    for event_index in 0..events {
        durations.push(harness.run_event(event_index)?);
    }
    let elapsed = benchmark_start.elapsed();
    harness.verify()?;
    durations.sort_unstable();
    Ok(BenchmarkResult {
        events,
        elapsed,
        p50: percentile(&durations, 50),
        p95: percentile(&durations, 95),
        p99: percentile(&durations, 99),
        changes: harness.changes,
        fractured: harness.fractured,
        frames_sent: harness.frames_sent,
        wire_bytes: harness.wire_bytes,
        initial_solids: initial_stats.solid_voxels,
        final_stats: harness.server.world().stats(),
    })
}

fn print_result(result: &BenchmarkResult) {
    let throughput = u128::try_from(result.events).unwrap_or(u128::MAX) * 1_000_000_000
        / result.elapsed.as_nanos().max(1);
    let budget_tenths = result.p99.as_nanos() * 1_000 / 16_666_667;
    println!("Destructible FPS technical spike — authoritative loopback");
    println!("  events              {}", result.events);
    println!("  elapsed             {}", duration_ms(result.elapsed));
    println!("  throughput          {throughput} events/s");
    println!("  event latency p50   {}", duration_ms(result.p50));
    println!("  event latency p95   {}", duration_ms(result.p95));
    println!("  event latency p99   {}", duration_ms(result.p99));
    println!(
        "  p99 / 60 Hz budget  {}.{}%",
        budget_tenths / 10,
        budget_tenths % 10
    );
    println!("  voxel changes       {}", result.changes);
    println!("  fractured voxels    {}", result.fractured);
    println!(
        "  network frames      {} (MTU {NETWORK_MTU})",
        result.frames_sent
    );
    println!("  wire payload        {}", binary_mib(result.wire_bytes));
    println!(
        "  world solids        {} -> {}",
        result.initial_solids, result.final_stats.solid_voxels
    );
    println!("  allocated chunks    {}", result.final_stats.chunks);
    println!(
        "  replicas            synchronized ({:032x})",
        result.final_stats.fingerprint
    );
}

fn duration_ms(duration: Duration) -> String {
    let micros = duration.as_micros();
    format!("{}.{:03} ms", micros / 1_000, micros % 1_000)
}

fn binary_mib(bytes: usize) -> String {
    let bytes = u128::try_from(bytes).unwrap_or(u128::MAX);
    let thousandths = bytes * 1_000 / 1_048_576;
    format!("{}.{:03} MiB", thousandths / 1_000, thousandths % 1_000)
}

fn parse_events() -> Result<usize, Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut events = 500_usize;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--events" => {
                let value = arguments.next().ok_or("--events requires a value")?;
                events = value.parse()?;
                if !(1..=1_000_000).contains(&events) {
                    return Err("--events must be between 1 and 1,000,000".into());
                }
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(events)
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    let index = (sorted.len() - 1) * percentile / 100;
    sorted[index]
}

struct XorShift64(u64);

impl XorShift64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    const fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn range_u32(&mut self, minimum: u32, maximum: u32) -> u32 {
        assert!(minimum < maximum);
        let offset = self.next() % u64::from(maximum - minimum);
        minimum + u32::try_from(offset).unwrap_or_default()
    }

    fn range_u16(&mut self, minimum: u16, maximum: u16) -> u16 {
        assert!(minimum < maximum);
        let offset = self.next() % u64::from(maximum - minimum);
        minimum + u16::try_from(offset).unwrap_or_default()
    }

    fn range_i32(&mut self, minimum: i32, maximum: i32) -> i32 {
        assert!(minimum < maximum);
        let width = u64::from(maximum.abs_diff(minimum));
        let offset = self.next() % width;
        minimum + i32::try_from(offset).unwrap_or_default()
    }
}
