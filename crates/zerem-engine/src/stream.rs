//! Serving a file that has not finished downloading.
//!
//! The point of the whole thing: press play on the first episode while the
//! season is still coming down. librqbit has the hard half — `stream(file_id)`
//! hands back a reader that waits for the pieces it needs and tells the picker
//! to fetch those first — and what was missing is a way to give that reader to
//! a media player.
//!
//! Players open URLs. So this is an HTTP server: a hundred lines over the
//! `tokio` that is already here, rather than a web framework and its tree for
//! one route.
//!
//! # It listens on the loopback and nowhere else
//!
//! `127.0.0.1` with a port the operating system picks. Not a fixed port, which
//! two copies would fight over; not `0.0.0.0`, which would serve somebody
//! else's films to the network the moment the machine joined a café's wi-fi.
//!
//! # Every URL carries a secret, and that was not always true
//!
//! It used to say here that no authentication was needed, because nothing off
//! this machine can reach a loopback socket and anything on it could read the
//! files directly anyway. The first half is true. The second is not: the app
//! runs as the person using it, and the other programs running as that person
//! are not all trusted by them — a browser tab, an extension, anything
//! installed on a whim. A loopback port is found by trying, torrent ids count
//! from one, and file indexes count from zero, so `GET /t/1/0` from any of them
//! returned whatever was downloading.
//!
//! So the path begins with sixteen random bytes made at startup, kept in
//! memory, never logged and never written down. `Stream` carries them with the
//! port because the two must not be separable: a URL that could be built from a
//! port alone is the hole this closes.
//!
//! # Range requests are the whole protocol
//!
//! A player asks for the first megabyte, reads the container's header, and then
//! seeks — which is a second request with `Range: bytes=N-`. Answer that
//! wrongly and the file appears to play for four seconds and stop. So the
//! answer carries `Accept-Ranges`, a `206` with a `Content-Range` when a range
//! was asked for, and the exact length of what follows.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use librqbit::ManagedTorrent;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use zerem_core::{Stream, TorrentId};

/// How much is moved per write.
///
/// 256 KiB: large enough that the loop is not the cost, small enough that a
/// player seeking away does not wait for a megabyte it has stopped wanting.
const CHUNK: usize = 256 * 1024;

/// A request line longer than this is not a request.
const LIMIT: usize = 8 * 1024;

/// How many headers are read before the request is abandoned.
///
/// A client that sends short headers for ever is a client that holds a task
/// open for ever, and the loop below has nothing else to stop it. Nothing a
/// player sends comes close to this.
const MAX_HEADERS: usize = 64;

/// What the window needs to build a URL, and what the loop needs to serve one.
pub struct Streamer {
    at: Stream,
}

impl Streamer {
    /// Bind, and start answering.
    ///
    /// `None` when the loopback will not give a port, which is a machine with
    /// no working network stack at all — the app runs, and the Play row is not
    /// offered. Also `None` when the operating system will not produce sixteen
    /// random bytes, which is not a thing to route around: without the secret
    /// every request is answered, and the answer is somebody's files.
    pub async fn start(lookup: Lookup) -> Option<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .inspect_err(|e| tracing::warn!("no local port for streaming: {e}"))
            .ok()?;
        let port = listener.local_addr().ok()?.port();
        let key = secret().inspect_err(|e| tracing::warn!("no randomness for the stream key: {e}")).ok()?;

        let guard: Arc<str> = key.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let (lookup, guard) = (lookup.clone(), guard.clone());
                // One task per player. A second request while the first is
                // streaming is what a seek looks like, and it must not wait.
                tokio::spawn(async move {
                    if let Err(e) = serve(socket, &lookup, &guard).await {
                        tracing::debug!("stream connection ended: {e:#}");
                    }
                });
            }
        });

        // The key is not logged. A log file is a thing people paste into bug
        // reports, and this one outlives nothing but the process.
        tracing::info!(port, "streaming on the loopback");
        Some(Self { at: Stream { port, key } })
    }

    /// Where it answers, and what gets past it.
    #[must_use]
    pub fn at(&self) -> Stream {
        self.at.clone()
    }
}

/// Sixteen bytes from the operating system, in hex.
///
/// A hundred and twenty-eight bits, which is not guessable, and hex because it
/// goes in a URL and survives every client, proxy and log on the way without
/// being escaped into something else.
fn secret() -> std::io::Result<Arc<str>> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)?;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        // Cannot fail: writing to a String is infallible.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex.into())
}

/// How the server finds a torrent, without holding the session's lock.
///
/// A closure rather than a reference to the session: the session is `&mut self`
/// on a single task and the server runs on its own, so what crosses is a
/// question and its answer.
pub type Lookup = Arc<dyn Fn(TorrentId) -> Option<Arc<ManagedTorrent>> + Send + Sync>;

// The stream is held to the end of the function on purpose, which is what the
// lint objects to. Its `Drop` is the point: while it is alive it is registered
// with the torrent, and that registration is what tells the piece picker to
// fetch what this reader is about to want. Letting it go earlier would end the
// prioritisation halfway through the film being watched.
#[allow(clippy::significant_drop_tightening, reason = "the stream's lifetime is the prioritisation")]
async fn serve(socket: TcpStream, lookup: &Lookup, key: &str) -> anyhow::Result<()> {
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);

    let mut line = String::new();
    if bounded(&mut reader, &mut line).await?.is_none() {
        return refuse(&mut write, "431 Request Header Fields Too Large").await;
    }
    // A wrong key and a malformed path are the same answer on purpose. `403`
    // would confirm to whoever is guessing that this is the right sort of
    // server and only the secret is missing, which is the one fact worth not
    // handing over.
    let Some((id, file)) = target(&line, key) else {
        return refuse(&mut write, "400 Bad Request").await;
    };

    // The headers, for the one that matters.
    let mut from = 0u64;
    for _ in 0..MAX_HEADERS {
        let mut header = String::new();
        let Some(read) = bounded(&mut reader, &mut header).await? else {
            return refuse(&mut write, "431 Request Header Fields Too Large").await;
        };
        if read == 0 || header.trim().is_empty() {
            break;
        }
        if let Some(start) = range_start(&header) {
            from = start;
        }
    }

    let Some(handle) = lookup(id) else {
        return refuse(&mut write, "404 Not Found").await;
    };
    let name = handle
        .with_metadata(|meta| {
            meta.file_infos.get(file).map(|f| f.relative_filename.to_string_lossy().into_owned())
        })
        .ok()
        .flatten()
        .unwrap_or_default();

    let mut stream = match handle.stream(file).await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::warn!(file, "cannot stream that file: {e:#}");
            return refuse(&mut write, "404 Not Found").await;
        }
    };

    let total = stream.len();
    let Some((remaining, partial)) = span(from, total) else {
        return refuse(&mut write, "416 Range Not Satisfiable").await;
    };
    if partial {
        stream.seek(std::io::SeekFrom::Start(from)).await?;
    }

    let kind = zerem_core::mime_of(&name).unwrap_or("application/octet-stream");
    let head = if partial {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: {kind}\r\nAccept-Ranges: bytes\r\n\
             Content-Length: {remaining}\r\nContent-Range: bytes {from}-{}/{total}\r\n\r\n",
            total - 1
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nAccept-Ranges: bytes\r\n\
             Content-Length: {remaining}\r\n\r\n"
        )
    };
    write.write_all(head.as_bytes()).await?;

    // And then the file, for as long as the player is still listening. It
    // hanging up mid-film is the normal way this ends, not a failure.
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if write.write_all(&buffer[..read]).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// Read one line, or refuse a line longer than one has any business being.
///
/// `read_line` has no bound of its own: it allocates whatever arrives, so a
/// local process that opens a socket and never sends a newline can grow this
/// one until it dies. The limit goes on the read rather than on the result,
/// because checking the length afterwards is checking after the damage.
///
/// `None` is a line that hit the ceiling.
async fn bounded(
    reader: &mut (impl AsyncBufReadExt + Unpin),
    into: &mut String,
) -> anyhow::Result<Option<usize>> {
    let read = reader.take(LIMIT as u64).read_line(into).await?;
    Ok((read < LIMIT).then_some(read))
}

/// How much is left to send, and whether the answer is a partial one.
///
/// `None` is a request that asked to start past the end — the one range answer
/// that has no body.
///
/// Its own function because the arithmetic here is a subtraction on unsigned
/// numbers, and the empty file is the case that gets it wrong: nothing rules
/// out a zero-byte file in a torrent, and `total - from` on one is a number
/// with eighteen digits in it.
const fn span(from: u64, total: u64) -> Option<(u64, bool)> {
    if from == 0 {
        return Some((total, false));
    }
    if from >= total {
        return None;
    }
    Some((total - from, true))
}

/// The torrent and file a request line names, for `GET /{key}/t/{id}/{file}`.
///
/// The key is checked here, before an id has been parsed and long before a
/// torrent has been looked up — so a request without it costs a string compare
/// and reaches nothing.
fn target(line: &str, key: &str) -> Option<(TorrentId, usize)> {
    let path = line.strip_prefix("GET ")?.split_whitespace().next()?;
    let (given, rest) = path.strip_prefix('/')?.split_once('/')?;
    if !same(given, key) {
        return None;
    }
    let rest = rest.strip_prefix("t/")?;
    let (id, file) = rest.split_once('/')?;
    Some((TorrentId(id.parse().ok()?), file.parse().ok()?))
}

/// Whether two secrets match, in time that does not depend on how far they
/// agree.
///
/// The length is allowed to leak — it is fixed and public. What must not is
/// *where* the first difference is: a compare that returns early tells whoever
/// is guessing that the first four characters were right, and that turns an
/// unguessable secret into thirty-two guessable ones. Loopback timing is noisy
/// enough that this is close to theoretical; it is also four lines.
fn same(given: &str, key: &str) -> bool {
    given.len() == key.len()
        && given.bytes().zip(key.bytes()).fold(0u8, |differs, (a, b)| differs | (a ^ b)) == 0
}

/// Where a `Range` header starts reading, if the header is one.
///
/// Only the open-ended form, `bytes=N-`, because it is the only one a player
/// sends: the end is the end of the file and asking for less would mean asking
/// again. A closed range is answered from its start, which is correct if
/// generous.
fn range_start(header: &str) -> Option<u64> {
    let value = header.trim().strip_prefix("Range:").or_else(|| header.trim().strip_prefix("range:"))?;
    let bytes = value.trim().strip_prefix("bytes=")?;
    bytes.split('-').next()?.trim().parse().ok()
}

async fn refuse(write: &mut (impl AsyncWriteExt + Unpin), status: &str) -> anyhow::Result<()> {
    write.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{range_start, same, secret, span, target};
    use zerem_core::TorrentId;

    /// Stands in for the sixteen random bytes, so the tests read.
    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_request_names_a_torrent_and_a_file_and_nothing_else() {
        let line = format!("GET /{KEY}/t/7/2 HTTP/1.1\r\n");
        assert_eq!(target(&line, KEY), Some((TorrentId(7), 2)));
    }

    #[test]
    fn anything_else_is_not_a_request_this_answers() {
        assert_eq!(target("GET / HTTP/1.1\r\n", KEY), None);
        assert_eq!(target(&format!("POST /{KEY}/t/7/2 HTTP/1.1\r\n"), KEY), None, "read only, on purpose");
        assert_eq!(target(&format!("GET /{KEY}/t/7 HTTP/1.1\r\n"), KEY), None, "a torrent is not a file");
        assert_eq!(target(&format!("GET /{KEY}/t/x/2 HTTP/1.1\r\n"), KEY), None);
        assert_eq!(target("", KEY), None);
    }

    #[test]
    fn a_request_without_the_secret_reaches_nothing() {
        // The whole of the fix. Every one of these was answered before: the
        // port is a loopback port anything can find by trying, torrent ids
        // count from one and file indexes from zero, so `GET /t/1/0` from any
        // program on the machine read whatever was downloading.
        assert_eq!(target("GET /t/7/2 HTTP/1.1\r\n", KEY), None, "the old shape, now refused");
        assert_eq!(target("GET //t/7/2 HTTP/1.1\r\n", KEY), None, "an empty key is a key");
        assert_eq!(target("GET /wrong/t/7/2 HTTP/1.1\r\n", KEY), None);
        let nearly = format!("{}0", &KEY[..KEY.len() - 1]);
        assert_eq!(target(&format!("GET /{nearly}/t/7/2 HTTP/1.1\r\n"), KEY), None, "one character");
    }

    #[test]
    fn the_secret_is_compared_whole_rather_than_up_to_the_first_difference() {
        assert!(same(KEY, KEY));
        assert!(!same("", KEY));
        assert!(!same(&KEY[..KEY.len() - 1], KEY), "a prefix is not a match");
        assert!(!same(&format!("{KEY}0"), KEY), "nor is the key plus something");
        // Differing in the first byte and in the last are the same answer, and
        // the loop that produces it reads both keys either way.
        assert!(!same(&format!("x{}", &KEY[1..]), KEY));
        assert!(!same(&format!("{}x", &KEY[..KEY.len() - 1]), KEY));
    }

    #[test]
    fn a_new_secret_is_long_enough_and_not_the_last_one() {
        let (first, second) = (secret().expect("randomness"), secret().expect("randomness"));
        assert_eq!(first.len(), 32, "sixteen bytes in hex");
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()), "url-safe without escaping");
        assert_ne!(first, second, "a fixed key would be no key at all");
    }

    #[test]
    fn an_empty_file_is_served_rather_than_subtracted_past_zero() {
        // Nothing rules out a zero-byte file in a torrent, and asking for one
        // used to fall past the guard and take `total - from` below zero.
        assert_eq!(span(0, 0), Some((0, false)), "a plain request for it is a body of nothing");
        assert_eq!(span(1, 0), None, "and any range into it is unsatisfiable");
        assert_eq!(span(4096, 0), None);
    }

    #[test]
    fn a_range_past_the_end_has_no_body() {
        assert_eq!(span(10, 10), None, "starting at the end is past it");
        assert_eq!(span(11, 10), None);
    }

    #[test]
    fn a_range_inside_the_file_sends_what_is_left() {
        assert_eq!(span(4, 10), Some((6, true)));
        assert_eq!(span(9, 10), Some((1, true)), "the last byte is still a range");
    }

    #[test]
    fn a_request_with_no_range_is_the_whole_file() {
        // And not a partial answer, which would put a Content-Range on a reply
        // that has nothing partial about it.
        assert_eq!(span(0, 10), Some((10, false)));
    }

    #[test]
    fn a_seek_is_a_range_header() {
        // The one that matters: answer it wrongly and a film plays for four
        // seconds and stops.
        assert_eq!(range_start("Range: bytes=1048576-\r\n"), Some(1_048_576));
        assert_eq!(range_start("range: bytes=0-\r\n"), Some(0), "headers are not case-sensitive");
        assert_eq!(
            range_start("Range: bytes=500-999\r\n"),
            Some(500),
            "a closed range starts where it starts"
        );
    }

    #[test]
    fn a_header_that_is_not_a_range_is_not_read_as_one() {
        assert_eq!(range_start("Accept: */*\r\n"), None);
        assert_eq!(range_start("Range: seconds=1-\r\n"), None, "bytes or nothing");
        assert_eq!(range_start("Range: bytes=abc-\r\n"), None);
        assert_eq!(range_start(""), None);
    }
}
