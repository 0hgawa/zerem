//! What a player needs to be told a file is.
//!
//! A media player asked to open a URL decides what to do with it from the
//! `Content-Type` before a byte of the body arrives. Handed
//! `application/octet-stream` a few will sniff the container anyway, most will
//! offer to save it, and the one somebody actually has installed is always in
//! the second group.
//!
//! Only the containers that get streamed. A torrent full of `.nfo` and `.sfv`
//! is not something anybody is going to press play on, and a list that tries to
//! be complete is a list nobody maintains.

/// The type for a file name, or `None` for one this does not know.
///
/// `None` is a real answer and the caller should say `application/octet-stream`
/// rather than guess: a wrong type sends a player looking for a demuxer that
/// will not open the file, which fails later and more confusingly than not
/// knowing does.
#[must_use]
pub fn of(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let known = |ext: &str| TABLE.iter().find(|(key, _)| *key == ext).map(|(_, value)| *value);
    known(&extension)
}

/// Sorted for reading, not for searching — twenty entries do not need a binary
/// search, and one somebody can scan is one somebody will keep correct.
const TABLE: [(&str, &str); 20] = [
    ("aac", "audio/aac"),
    ("avi", "video/x-msvideo"),
    ("flac", "audio/flac"),
    ("m4a", "audio/mp4"),
    ("m4v", "video/x-m4v"),
    ("mka", "audio/x-matroska"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("mpeg", "video/mpeg"),
    ("mpg", "video/mpeg"),
    ("ogg", "audio/ogg"),
    ("ogv", "video/ogg"),
    ("opus", "audio/opus"),
    ("srt", "application/x-subrip"),
    ("ts", "video/mp2t"),
    ("wav", "audio/wav"),
    ("webm", "video/webm"),
    ("wmv", "video/x-ms-wmv"),
];

/// Whether this is something worth offering to play.
///
/// The menu asks before it draws a Play row: offering to stream a `.nfo` is
/// offering something that opens Notepad on a file that is not there yet.
#[must_use]
pub fn is_playable(name: &str) -> bool {
    of(name).is_some_and(|kind| kind.starts_with("video/") || kind.starts_with("audio/"))
}

#[cfg(test)]
mod tests {
    use super::{is_playable, of, TABLE};

    #[test]
    fn the_containers_a_torrent_actually_holds_are_known() {
        assert_eq!(of("A Film (1999).mkv"), Some("video/x-matroska"));
        assert_eq!(of("track 01.flac"), Some("audio/flac"));
        assert_eq!(of("clip.MP4"), Some("video/mp4"), "the case of the extension is not a different file");
    }

    #[test]
    fn what_is_not_known_says_so_rather_than_guessing() {
        // A wrong type sends a player looking for a demuxer that will not open
        // the file, which fails later and more confusingly than not knowing.
        assert_eq!(of("readme.nfo"), None);
        assert_eq!(of("no extension at all"), None);
        assert_eq!(of(""), None);
        assert_eq!(of(".mkv"), Some("video/x-matroska"), "a dotfile is still its extension");
    }

    #[test]
    fn only_what_can_be_played_offers_to_be() {
        assert!(is_playable("show.s01e01.mkv"));
        assert!(is_playable("song.mp3"));
        assert!(!is_playable("subs.srt"), "carried, not played");
        assert!(!is_playable("readme.nfo"));
    }

    #[test]
    fn the_table_is_sorted_because_somebody_has_to_read_it() {
        let mut sorted = TABLE;
        sorted.sort_unstable_by_key(|(key, _)| *key);
        assert_eq!(sorted, TABLE, "TABLE is out of order");
    }

    #[test]
    fn no_extension_is_listed_twice() {
        let mut keys: Vec<&str> = TABLE.iter().map(|(k, _)| *k).collect();
        keys.dedup();
        assert_eq!(keys.len(), TABLE.len());
    }
}

/// Where the loopback server is, and the secret that gets past it.
///
/// The two travel together and there is no way to hold one without the other,
/// which is the whole design. The port alone was public: it is a loopback port
/// and any program on the machine can find it by trying, torrent ids count from
/// one, and file indexes count from zero — so `GET /t/1/0` from anything at all
/// read whatever the first torrent was downloading. The port is still easy to
/// find. It is now useless on its own.
///
/// Here rather than in the server that answers it, because two sides need the
/// shape and they must not drift: the engine builds this, and the window hands
/// it to whatever plays films. A path the server does not recognise is a play
/// button that opens a media player on a 404.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    /// On the loopback, and ephemeral: a new one every launch.
    pub port: u16,
    /// Made by the server at startup, never written down, gone when the app
    /// closes. `Arc` because the snapshot carrying it is cloned every tick.
    pub key: std::sync::Arc<str>,
}

impl Stream {
    /// Where a player is pointed to watch one file of one torrent.
    #[must_use]
    pub fn url(&self, torrent: u32, file: usize) -> String {
        let Self { port, key } = self;
        format!("http://127.0.0.1:{port}/{key}/t/{torrent}/{file}")
    }
}

#[cfg(test)]
mod address {
    use super::Stream;

    fn at(port: u16) -> Stream {
        Stream { port, key: "0123456789abcdef0123456789abcdef".into() }
    }

    #[test]
    fn the_url_names_the_torrent_and_the_file() {
        assert_eq!(at(51413).url(7, 2), "http://127.0.0.1:51413/0123456789abcdef0123456789abcdef/t/7/2");
    }

    #[test]
    fn it_is_the_loopback_and_says_so() {
        // Never a hostname and never `0.0.0.0`: the server binds the loopback,
        // and a URL naming anything else is a URL that would not answer.
        assert!(at(1).url(0, 0).starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn the_secret_comes_before_anything_that_names_a_file() {
        // So a request that does not carry it is refused by the parser, before
        // a torrent id has been read out of it, let alone looked up.
        let url = at(1).url(7, 2);
        let key = url.find("0123456789abcdef").expect("the key is in there");
        assert!(key < url.find("/t/").expect("the route is in there"));
    }
}
