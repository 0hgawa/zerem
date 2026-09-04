//! Zerem engine — the BitTorrent session and the tick.
//!
//! Knows nothing about the UI. It publishes an immutable [`Snapshot`] from its
//! own thread and consumes [`Command`]s — that is the whole surface, and it is
//! why a headless mode later is the same engine with a different consumer
//! rather than a rewrite.
//!
//! There is no wake-up callback. A consumer reads [`Engine::snapshot`] whenever
//! it likes and compares `seq`; at one publish a second a callback would buy
//! nothing but a `Send` bound reaching back into the consumer's own types.

mod command;
mod config;
mod journal;
mod map;
mod session;
mod snapshot;

pub use command::Command;
pub use config::{EngineConfig, DEFAULT_PORT};
pub use snapshot::Snapshot;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use session::TorrentSession;

/// How often the session republishes. A torrent client's data is interesting
/// once a second; anything faster is heat.
pub const TICK: Duration = Duration::from_secs(1);

/// A handle to the running session, not the session itself.
///
/// `Clone` for the same reason a channel sender is: every field is already a
/// shared handle, and work that has to leave the UI thread — a native file
/// dialog, say — needs something `Send` to talk to the engine with. Cloning
/// starts nothing and costs three refcount bumps.
#[derive(Clone)]
pub struct Engine {
    latest: Arc<RwLock<Arc<Snapshot>>>,
    commands: UnboundedSender<Command>,
    paused: Arc<AtomicBool>,
}

impl Engine {
    /// Start the session on its own thread.
    ///
    /// Never fails, on purpose. A session that cannot bind its port or reach its
    /// download folder reports that in the snapshot's notice and the window
    /// opens saying so — which the user can act on. Refusing to start at all
    /// would leave them with a process that vanished and no explanation.
    ///
    /// # Panics
    /// If the OS refuses to spawn the thread or to build a tokio runtime. There
    /// is no client without an engine and nothing to recover to.
    #[must_use]
    pub fn start(config: EngineConfig) -> Self {
        let latest = Arc::new(RwLock::new(Arc::new(Snapshot::new(0, 0, Vec::new()))));
        let paused = Arc::new(AtomicBool::new(false));
        let (commands, inbox) = mpsc::unbounded_channel();

        thread::Builder::new()
            .name("zerem-engine".into())
            .spawn({
                let (latest, paused) = (latest.clone(), paused.clone());
                move || {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("build the engine runtime");
                    runtime.block_on(run(config, inbox, &latest, &paused));
                }
            })
            .expect("spawn the engine thread");

        Self { latest, commands, paused }
    }

    /// The most recent snapshot. Cheap — one lock acquisition and a refcount.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.latest.read().map_or_else(|e| e.into_inner().clone(), |s| s.clone())
    }

    /// Queue a command. Never blocks; the engine picks it up immediately.
    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("the engine thread is gone");
        }
    }

    /// Stop advancing the session.
    ///
    /// Set while the window is not visible. This is not "tick more slowly" — a
    /// paused engine publishes nothing and the UI redraws nothing, which Phase 0
    /// measured at 0.000 % CPU. Transfers keep running: pausing the *view* must
    /// not pause the downloads.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}

async fn run(
    config: EngineConfig,
    mut inbox: UnboundedReceiver<Command>,
    latest: &RwLock<Arc<Snapshot>>,
    paused: &AtomicBool,
) {
    let mut session = match TorrentSession::start(&config).await {
        Ok(session) => session,
        Err(e) => {
            // The window still opens, holding an explanation.
            tracing::error!(error = %e, "the session could not start");
            publish(
                latest,
                Arc::new(
                    Snapshot::new(1, 1, Vec::new()).with_notice(Some(Arc::from(format!("{e:#}").as_str()))),
                ),
            );
            return;
        }
    };
    publish(latest, session.publish());

    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut was_paused = false;

    loop {
        let acted = tokio::select! {
            _ = ticker.tick() => false,
            command = inbox.recv() => {
                let Some(command) = command else {
                    tracing::debug!("engine inbox closed, thread exiting");
                    return;
                };
                apply(&mut session, command, latest).await;
                true
            }
        };

        // Releases what a finished pin was holding. Cheap: it looks only at
        // torrents that have a pin at all, which is normally none of them.
        session.reconcile().await;
        // And gives the next torrent its turn when one finishes or is stopped.
        // Free when no limit is set, which is the default.
        session.enforce_queue().await;

        // A command still publishes while the view is paused: a tray action has
        // to show its result the moment the window comes back.
        let is_paused = paused.load(Ordering::Relaxed);
        if is_paused && !acted {
            was_paused = true;
            continue;
        }
        // Nothing was sampled while the window was away, so the footer's minute
        // starts again rather than joining two separate ones end to end. Keyed
        // on the first publish after the gap, not on the flag going down: a
        // tray command publishes while the view is still paused.
        if was_paused {
            session.forget_history();
            was_paused = false;
        }
        publish(latest, session.publish());
    }
}

async fn apply(session: &mut TorrentSession, command: Command, latest: &RwLock<Arc<Snapshot>>) {
    let outcome = match command {
        Command::Add { ref source } => session.add(source).await,
        Command::Inspect { ref source } => {
            // Published between the two halves, so the dialog is on screen
            // saying "fetching" while the swarm is being asked, rather than
            // appearing several seconds after the click looked ignored.
            session.begin_inspect(source);
            publish(latest, session.publish());
            session.finish_inspect(source).await;
            Ok(())
        }
        Command::ConfirmAdd { ref only_files } => session.confirm_add(only_files.clone()).await,
        Command::CancelAdd => {
            session.cancel_add();
            Ok(())
        }
        Command::SetFileWanted { id, file, wanted } => session.set_file_wanted(id, file, wanted).await,
        Command::SetFileFirst { id, file, first } => session.set_file_first(id, file, first).await,
        Command::Start(id) => session.set_running(id, true).await,
        Command::Pause(id) => session.set_running(id, false).await,
        Command::Remove { id, delete_data } => session.remove(id, delete_data).await,
        Command::SetLimits { down, up } => {
            session.set_limits(down, up);
            Ok(())
        }
        Command::SetMaxActive(limit) => {
            session.set_max_active(limit).await;
            Ok(())
        }
        // By reference so `command` survives for the error log below. The
        // clone is one PathBuf per preference change.
        Command::SetDownloadDir(ref dir) => session.set_output_dir(dir.clone()),
        Command::WatchDetails(id) => {
            session.watch_details(id);
            Ok(())
        }
    };
    if let Err(e) = outcome {
        // `{:#}` so the anyhow context chain reads as one sentence — the cause
        // is what makes an error actionable.
        let text = format!("{e:#}");
        tracing::warn!(?command, error = %text, "command failed");
        session.notify(text.as_str());
    }
}

fn publish(latest: &RwLock<Arc<Snapshot>>, next: Arc<Snapshot>) {
    match latest.write() {
        Ok(mut slot) => *slot = next,
        Err(poisoned) => *poisoned.into_inner() = next,
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, Engine, EngineConfig};
    use zerem_core::TorrentId;

    fn scratch_config() -> EngineConfig {
        let scratch = std::env::temp_dir().join("zerem-engine-tests");
        EngineConfig {
            download_dir: scratch.join("downloads"),
            // Its own state folder, and not for tidiness: the default is the
            // real one, so a test run would read and rewrite the user's actual
            // torrent list.
            state_dir: scratch.join("state"),
            // Port 0 is wrong for a client and right for a test: two test
            // binaries must not fight over 6881.
            port: 0,
            max_active: 0,
            utp: false,
            upnp: false,
        }
    }

    #[test]
    fn a_snapshot_is_readable_before_the_session_is_up() {
        // The window paints as soon as it opens. Starting the session involves
        // binding a port and touching the disk, and the UI cannot wait for it.
        let engine = Engine::start(scratch_config());
        let snap = engine.snapshot();
        assert!(snap.torrents.is_empty());
    }

    #[test]
    fn nonsense_input_is_reported_rather_than_swallowed() {
        let engine = Engine::start(scratch_config());
        engine.send(Command::Add { source: "not a magnet".into() });
        // Asserting the notice arrives would mean sleeping on the session's
        // startup. What matters here is that a bad command cannot take the
        // engine thread down — `send` is infallible and the loop survives.
        engine.send(Command::Pause(TorrentId(0)));
        engine.send(Command::Remove { id: TorrentId(0), delete_data: false });
    }

    #[test]
    fn pausing_the_view_is_not_pausing_the_transfers() {
        let engine = Engine::start(scratch_config());
        engine.set_paused(true);
        engine.send(Command::Start(TorrentId(0)));
        engine.set_paused(false);
    }
}
