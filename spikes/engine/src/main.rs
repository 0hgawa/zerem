// Zerem — spike S0.2 (engine: librqbit throughput, uTP, and the mapping).
//
// What was measured, and what it came back as (docs/spike-report.md):
//
//   * saturates the link          ✅ 3.72 GB in 96 s, peaking at 66.9 MB/s
//   * uTP actually connects       ✅ up to 15 peers over uTP, read from
//                                    `live_utp` rather than inferred from a setting
//   * seeding stays cheap         ✅ 0.77 % of one core, 25 MB resident
//   * every librqbit state maps   ✅ see map.rs
//   * seeding does not wreck the connection's latency — still open, and the one
//     criterion that needs a human browsing while the uplink is full
//
// The download costs ~1.45 % of one core per MB/s, and neither the SHA-1
// backend nor a peer cap moves it. The roadmap's original "≤ 15 % of one core
// at 1 Gbps" was set without data and was the thing that was wrong.
//
// THIS ONE TOUCHES THE NETWORK. It joins a real BitTorrent swarm from this
// machine's address, so it refuses to start without an explicit torrent
// argument — there is no default and no accidental run.
//
//   cargo run --release -- <magnet-or-url-or-.torrent> [--out DIR] [--tcp-only]
//                          [--port N] [--seconds N] [--peer-limit N]
//
// A large Linux distribution image is the conventional subject: plenty of
// seeds, legal to fetch, and big enough to reach a steady rate.

mod map;

use std::net::{Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use librqbit::{
    AddTorrent, AddTorrentOptions, ListenerMode, ListenerOptions, Session, SessionOptions, TorrentStats,
};
use zerem_core::{fmt, TorrentId};

/// Fixed rather than ephemeral. librqbit defaults `listen_addr` to port 0, which
/// works for outgoing connections and quietly costs every incoming one — the
/// difference between leeching and seeding.
const DEFAULT_PORT: u16 = 6881;

struct Args {
    torrent: String,
    out: PathBuf,
    tcp_only: bool,
    port: u16,
    seconds: Option<u64>,
    /// `None` leaves librqbit's default, which let ~128 connections open at
    /// once in the first measured runs.
    peer_limit: Option<usize>,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut torrent = None;
    let mut out = std::env::temp_dir().join("zerem-spike");
    let mut tcp_only = false;
    let mut port = DEFAULT_PORT;
    let mut seconds = None;
    let mut peer_limit = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--tcp-only" => tcp_only = true,
            "--out" => out = it.next().context("--out needs a directory")?.into(),
            "--port" => port = it.next().context("--port needs a number")?.parse()?,
            "--seconds" => seconds = Some(it.next().context("--seconds needs a number")?.parse()?),
            "--peer-limit" => {
                peer_limit = Some(it.next().context("--peer-limit needs a number")?.parse()?);
            }
            other if other.starts_with('-') => bail!("unknown flag {other}"),
            other => torrent = Some(other.to_string()),
        }
    }

    let Some(torrent) = torrent else {
        bail!(
            "no torrent given.\n\n\
             This spike joins a real BitTorrent swarm from this machine's address, \
             so it will not run without being told what to fetch:\n\n  \
             cargo run --release -- <magnet-or-url-or-.torrent> [--out DIR] [--tcp-only] \
             [--port N] [--seconds N] [--peer-limit N]\n"
        );
    };
    Ok(Args { torrent, out, tcp_only, port, seconds, peer_limit })
}

/// One line per second, in the shape the report wants.
fn report(elapsed: Duration, stats: &TorrentStats, peak_down: &mut u64) {
    let row = map::to_row(TorrentId(0), "spike", stats);
    *peak_down = (*peak_down).max(row.down_bps);

    let (live_tcp, live_utp, connecting, seen) = stats
        .live
        .as_ref()
        .map(|l| {
            let p = &l.snapshot.peer_stats;
            (p.live_tcp, p.live_utp, p.connecting, p.seen)
        })
        .unwrap_or_default();

    println!(
        "t={:<5}s {:<12} {:>7} {:>11} up {:>11} peers tcp={:<4} utp={:<4} conn={:<4} seen={:<5} eta {}",
        elapsed.as_secs(),
        row.status_text(),
        fmt::percent(row.done, row.size),
        fmt::speed(row.down_bps),
        fmt::speed(row.up_bps),
        live_tcp,
        live_utp,
        connecting,
        seen,
        fmt::eta(row.eta),
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(match std::env::var("ZEREM_LOG").as_deref() {
            Ok("debug") => tracing::Level::DEBUG,
            Ok("trace") => tracing::Level::TRACE,
            _ => tracing::Level::WARN,
        })
        .with_target(false)
        .compact()
        .init();

    let args = parse_args()?;
    std::fs::create_dir_all(&args.out)?;

    let mode = if args.tcp_only { ListenerMode::TcpOnly } else { ListenerMode::TcpAndUtp };
    println!("output   {}", args.out.display());
    println!("listen   {mode:?} on port {}", args.port);
    println!("torrent  {}\n", args.torrent);

    let session = Session::new_with_opts(
        args.out.clone(),
        SessionOptions {
            listen: Some(ListenerOptions {
                // librqbit defaults to TcpOnly, with an upstream note that uTP
                // becomes the default "once uTP is stable". Asking for it is the
                // whole point of this spike.
                mode,
                // IPv6 unspecified is dual-stack; `ipv4_only: false` keeps it.
                listen_addr: SocketAddr::from((Ipv6Addr::UNSPECIFIED, args.port)),
                enable_upnp_port_forwarding: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .context("creating the session")?;

    if let Some(addr) = session.listen_addr() {
        println!("bound to {addr}\n");
    }

    let handle = session
        .add_torrent(
            AddTorrent::from_cli_argument(&args.torrent)?,
            Some(AddTorrentOptions {
                // Not optional for a client, whatever the default says. From
                // librqbit's own doc: "Even when all the torrent pieces have
                // been written, `overwrite` needs to be enabled in order to
                // resume/seed the torrent." Without it, restoring a session on
                // startup fails with "file exists" on every finished torrent.
                overwrite: true,
                peer_limit: args.peer_limit,
                ..Default::default()
            }),
        )
        .await
        .context("adding the torrent")?
        .into_handle()
        .context("the torrent was listed rather than added")?;

    let started = Instant::now();
    let mut peak_down = 0_u64;
    let mut ever_utp = false;
    let deadline = args.seconds.map(Duration::from_secs);

    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = tokio::signal::ctrl_c() => {
                println!("\ninterrupted");
                break;
            }
        }

        let stats = handle.stats();
        if let Some(live) = &stats.live {
            ever_utp |= live.snapshot.peer_stats.live_utp > 0;
        }
        report(started.elapsed(), &stats, &mut peak_down);

        // `--seconds` governs when it is given: a seeding run is measured on an
        // already-complete torrent, where "finished" is the starting condition
        // rather than an event to wait for. Without a deadline, finishing ends
        // the run.
        if stats.finished && deadline.is_none() {
            println!("\nfinished");
            break;
        }
        if deadline.is_some_and(|d| started.elapsed() >= d) {
            println!("\nreached the time limit");
            break;
        }
    }

    println!("\n── result ──");
    println!("elapsed      {:?}", started.elapsed());
    println!("peak down    {}", fmt::speed(peak_down));
    println!(
        "uTP peers    {}",
        if args.tcp_only {
            "n/a — ran with --tcp-only"
        } else if ever_utp {
            "yes — at least one peer connected over uTP"
        } else {
            "NONE seen: uTP was requested but nothing connected over it"
        }
    );

    session.stop().await;
    Ok(())
}
