//! How the session is set up.
//!
//! Every value here is set explicitly, because **librqbit's defaults are wrong
//! for a client** — spike S0.2 read them out of the crate source:
//!
//!  * `ListenerMode::TcpOnly` — uTP off, with an upstream note that it becomes
//!    the default "once uTP is stable"
//!  * `listen_addr` port `0` — ephemeral, which works for outgoing connections
//!    and quietly costs every incoming one
//!  * `enable_upnp_port_forwarding: false`
//!
//! Inheriting any of those would produce a client that leeches and never seeds.

use std::net::{Ipv6Addr, SocketAddr};
use std::path::PathBuf;

use librqbit::dht::DhtPersistenceConfig;
use librqbit::{DhtSessionConfig, ListenerMode, ListenerOptions, SessionOptions, SessionPersistenceConfig};

/// The conventional BitTorrent port. Fixed rather than ephemeral so peers can
/// reach us, and so a router's forwarding rule has something to point at.
pub const DEFAULT_PORT: u16 = 6881;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub download_dir: PathBuf,
    /// Where the session survives a restart. Ours, not librqbit's: left to its
    /// defaults it writes into `…/rqbit/`, which a real rqbit install would then
    /// be sharing with us.
    pub state_dir: PathBuf,
    pub port: u16,
    /// How many torrents may download at once. Zero is no limit.
    pub max_active: u32,
    /// Whether a torrent is added stopped rather than started.
    pub add_paused: bool,
    /// uTP alongside TCP. On by default here, off by default in librqbit.
    pub utp: bool,
    pub upnp: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            state_dir: default_state_dir(),
            port: DEFAULT_PORT,
            // No limit by default, so nothing changes for somebody with three
            // torrents. A queue is what you reach for when you have twenty.
            max_active: 0,
            add_paused: false,
            utp: true,
            upnp: true,
        }
    }
}

impl EngineConfig {
    pub(crate) fn to_session_options(&self) -> SessionOptions {
        SessionOptions {
            listen: Some(ListenerOptions {
                mode: if self.utp { ListenerMode::TcpAndUtp } else { ListenerMode::TcpOnly },
                // An IPv6 unspecified address is dual-stack; `ipv4_only` stays
                // false, so this listens on both families.
                listen_addr: SocketAddr::from((Ipv6Addr::UNSPECIFIED, self.port)),
                enable_upnp_port_forwarding: self.upnp,
                ..Default::default()
            }),
            // The list survives a restart, and it is librqbit's own resume data
            // that does it — not a list of magnets we wrote out ourselves.
            // `Session::new_with_opts` re-adds every stored torrent before it
            // returns, keeping their ids, so nothing above this layer has to
            // know a restart happened.
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(self.state_dir.join("session")),
            }),
            // Without it every launch re-hashes every complete torrent. On a
            // handful of large ones that is minutes of disk churn before the
            // client is usable, for data it already verified last time.
            fastresume: true,
            dht: Some(DhtSessionConfig {
                persistence: Some(DhtPersistenceConfig {
                    config_filename: Some(self.state_dir.join("dht.json")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

/// The system Downloads folder, falling back to the home directory and then to
/// the working directory. Never fails: a client that will not start because it
/// could not guess a folder is worse than one that starts in the wrong place,
/// which the user can then change.
fn default_download_dir() -> PathBuf {
    dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("Zerem")
}

/// Where the session and the DHT routing table live between runs.
/// `%LOCALAPPDATA%\Zerem` on Windows, `~/.local/share/zerem` elsewhere.
fn default_state_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("Zerem")
}

#[cfg(test)]
mod tests {
    use super::{EngineConfig, DEFAULT_PORT};
    use librqbit::ListenerMode;
    use librqbit::SessionPersistenceConfig;
    use std::path::PathBuf;

    #[test]
    fn the_defaults_are_ours_not_librqbits() {
        // Every one of these is the opposite of what librqbit would give.
        let opts = EngineConfig::default().to_session_options();
        let listen = opts.listen.expect("we always configure a listener");
        assert!(matches!(listen.mode, ListenerMode::TcpAndUtp), "uTP must be on");
        assert_eq!(listen.listen_addr.port(), DEFAULT_PORT, "never an ephemeral port");
        assert!(listen.enable_upnp_port_forwarding);
        assert!(!listen.ipv4_only, "dual-stack");
    }

    #[test]
    fn turning_utp_off_is_the_only_thing_that_changes_the_mode() {
        let config = EngineConfig { utp: false, ..Default::default() };
        let listen = config.to_session_options().listen.expect("a listener");
        assert!(matches!(listen.mode, ListenerMode::TcpOnly));
    }

    #[test]
    fn a_download_folder_is_always_chosen() {
        // A client that refuses to start because it could not guess a folder is
        // worse than one that starts somewhere the user can then change.
        assert!(EngineConfig::default().download_dir.ends_with("Zerem"));
    }

    #[test]
    fn the_session_is_persisted_into_our_own_folder() {
        // Left to librqbit's default this lands in `…/rqbit/`, which a real
        // rqbit install would then be sharing with us.
        let config = EngineConfig { state_dir: PathBuf::from("/state"), ..Default::default() };
        let opts = config.to_session_options();
        let Some(SessionPersistenceConfig::Json { folder: Some(folder) }) = opts.persistence else {
            panic!("the session must persist as JSON in a folder we chose");
        };
        assert_eq!(folder, PathBuf::from("/state/session"));
        let dht = opts.dht.expect("DHT stays on");
        let dht_file = dht.persistence.expect("its routing table is persisted too").config_filename;
        assert_eq!(dht_file, Some(PathBuf::from("/state/dht.json")));
    }

    #[test]
    fn fastresume_is_on() {
        // Off by default, and off means re-hashing every complete torrent on
        // every launch — minutes of disk churn for data already verified.
        assert!(EngineConfig::default().to_session_options().fastresume);
    }
}
