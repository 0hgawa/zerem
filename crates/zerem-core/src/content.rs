//! What kind of thing a torrent holds.
//!
//! A list of names is read; a list of *shapes* is scanned. Giving each row the
//! icon of what is actually inside it is the cheapest way to make a long table
//! findable — and unlike cover art it needs no network, no title guessing and no
//! third party: the answer is already in the metadata.
//!
//! Weighed by bytes rather than by file count, which is the whole trick. A film
//! ships with a poster, an nfo and four subtitle tracks — five of the seven
//! files — and it is still a film, because the film is 99 % of it.

use std::path::Path;

/// What a torrent mostly is.
///
/// The variant order is a contract with the UI, which picks an icon from
/// [`Content::kind`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Content {
    /// A magnet whose metadata has not arrived. There is nothing to look at
    /// yet, which is not the same as looking at nothing.
    #[default]
    Unknown,
    Video,
    Audio,
    Image,
    Archive,
    Disc,
    Document,
    /// Nothing holds enough of the bytes to name the whole torrent.
    Mixed,
}

/// The kinds a single file can be counted towards, in the order [`EXTENSIONS`]
/// declares them. The two arrays are read together or not at all.
const NAMED: [Content; 6] =
    [Content::Video, Content::Audio, Content::Image, Content::Archive, Content::Disc, Content::Document];

/// Extensions per kind, without the dot. Matched case-insensitively, so only
/// one spelling of each is written down.
const EXTENSIONS: [&[&str]; 6] = [
    &["mkv", "mp4", "avi", "mov", "wmv", "m4v", "mpg", "mpeg", "webm", "flv", "ts", "m2ts", "vob", "ogv"],
    &["mp3", "flac", "wav", "aac", "ogg", "opus", "m4a", "wma", "alac", "aiff", "ape", "mka"],
    &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "heic", "avif", "svg", "psd"],
    &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "cab"],
    &["iso", "img", "bin", "cue", "mdf", "nrg", "dmg", "vhd"],
    &["pdf", "epub", "mobi", "azw3", "djvu", "txt", "doc", "docx", "cbz", "cbr"],
];

impl Content {
    /// Read a torrent's file list. `(path, length)` pairs, in any order.
    ///
    /// A kind has to hold two thirds of the bytes to name the whole torrent
    /// rather than a bare majority: a film with its subtitles is a film, while
    /// a folder that is half video and half something else genuinely has no
    /// single answer, and saying "video" there would be a guess dressed as a
    /// fact.
    #[must_use]
    pub fn of<'a>(files: impl IntoIterator<Item = (&'a Path, u64)>) -> Self {
        // Indexed by NAMED. Unrecognised files are counted into `total` and
        // into nothing else, so a torrent full of them lands on Mixed.
        let mut held = [0u64; NAMED.len()];
        let mut total = 0u64;
        for (path, len) in files {
            total = total.saturating_add(len);
            if let Some(kind) = slot_of(path) {
                held[kind] = held[kind].saturating_add(len);
            }
        }

        if total == 0 {
            return Self::Unknown;
        }
        NAMED
            .iter()
            .zip(held)
            .max_by_key(|&(_, bytes)| bytes)
            // Widened rather than saturated: an exact comparison at any size,
            // and the arithmetic runs once per torrent in its whole lifetime.
            .filter(|&(_, bytes)| u128::from(bytes) * 3 >= u128::from(total) * 2)
            .map_or(Self::Mixed, |(kind, _)| *kind)
    }

    /// The discriminant the UI picks an icon by, mirroring [`crate::State::kind`].
    ///
    /// Written out rather than cast, so reordering the enum cannot silently
    /// give every torrent the wrong icon.
    #[must_use]
    pub const fn kind(self) -> i32 {
        match self {
            Self::Unknown => 0,
            Self::Video => 1,
            Self::Audio => 2,
            Self::Image => 3,
            Self::Archive => 4,
            Self::Disc => 5,
            Self::Document => 6,
            Self::Mixed => 7,
        }
    }
}

/// Which of [`NAMED`] a file counts towards, if any.
fn slot_of(path: &Path) -> Option<usize> {
    let ext = path.extension()?.to_str()?;
    EXTENSIONS.iter().position(|exts| exts.iter().any(|known| known.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::Content;
    use std::path::Path;

    fn of(files: &[(&str, u64)]) -> Content {
        Content::of(files.iter().map(|(p, len)| (Path::new(*p), *len)))
    }

    #[test]
    fn a_film_with_its_trimmings_is_still_a_film() {
        // Five of the seven files are not video. The film is 99 % of the bytes,
        // which is why the weighing is by size and not by count.
        assert_eq!(
            of(&[
                ("The Longest Winter.mkv", 4_000_000_000),
                ("The Longest Winter.eng.srt", 60_000),
                ("The Longest Winter.pt-BR.srt", 60_000),
                ("The Longest Winter.nfo", 2_000),
                ("poster.jpg", 400_000),
                ("sample/sample.mkv", 20_000_000),
                ("RARBG.txt", 30),
            ]),
            Content::Video
        );
    }

    #[test]
    fn an_album_is_audio_despite_its_artwork() {
        assert_eq!(
            of(&[
                ("01 - Opening.flac", 40_000_000),
                ("02 - Closing.flac", 38_000_000),
                ("cover.png", 900_000),
                ("album.log", 4_000),
            ]),
            Content::Audio
        );
    }

    #[test]
    fn an_even_split_refuses_to_pick_a_side() {
        // Half and half genuinely has no answer, and "video" here would be a
        // guess dressed as a fact.
        assert_eq!(of(&[("film.mkv", 1_000_000_000), ("soundtrack.flac", 1_000_000_000)]), Content::Mixed);
    }

    #[test]
    fn content_nobody_listed_is_mixed_rather_than_wrong() {
        // A game: executables and data blobs, none of them in any list.
        assert_eq!(of(&[("game.exe", 8_000_000), ("data/assets.pak", 6_000_000_000)]), Content::Mixed);
    }

    #[test]
    fn extensions_match_whatever_case_they_are_written_in() {
        assert_eq!(of(&[("DEBIAN.ISO", 1_000)]), Content::Disc);
        assert_eq!(of(&[("Track.FLAC", 1_000)]), Content::Audio);
    }

    #[test]
    fn a_file_with_no_extension_counts_towards_nothing() {
        assert_eq!(of(&[("README", 1_000)]), Content::Mixed);
    }

    #[test]
    fn metadata_that_has_not_arrived_is_unknown_not_mixed() {
        // The distinction the row depends on: "nothing to look at yet" is not
        // "looked, and it is a jumble".
        assert_eq!(of(&[]), Content::Unknown);
        assert_eq!(of(&[("pending.mkv", 0)]), Content::Unknown);
    }

    #[test]
    fn every_kind_is_reachable_and_distinct() {
        // The variant order is a contract with the UI's icon table, so a
        // reordering has to break something here rather than in the window.
        let kinds: Vec<i32> = [
            of(&[("a.mkv", 1)]),
            of(&[("a.flac", 1)]),
            of(&[("a.png", 1)]),
            of(&[("a.zip", 1)]),
            of(&[("a.iso", 1)]),
            of(&[("a.epub", 1)]),
            of(&[("a.bin.unknown", 1)]),
        ]
        .iter()
        .map(|c| c.kind())
        .collect();
        assert_eq!(kinds, [1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(Content::default().kind(), 0);
    }
}
