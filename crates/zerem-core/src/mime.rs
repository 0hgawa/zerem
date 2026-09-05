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

/// Where a player is pointed to watch one file of one torrent.
///
/// Here rather than in the server that answers it, because two places need the
/// shape and they must not drift: the engine builds this, and the window hands
/// it to whatever plays films. A path the server does not recognise is a play
/// button that opens a media player on a 404.
#[must_use]
pub fn stream_url(port: u16, torrent: u32, file: usize) -> String {
    format!("http://127.0.0.1:{port}/t/{torrent}/{file}")
}

#[cfg(test)]
mod address {
    use super::stream_url;

    #[test]
    fn the_url_names_the_torrent_and_the_file() {
        assert_eq!(stream_url(51413, 7, 2), "http://127.0.0.1:51413/t/7/2");
    }

    #[test]
    fn it_is_the_loopback_and_says_so() {
        // Never a hostname and never `0.0.0.0`: the server binds the loopback,
        // and a URL naming anything else is a URL that would not answer.
        assert!(stream_url(1, 0, 0).starts_with("http://127.0.0.1:"));
    }
}
