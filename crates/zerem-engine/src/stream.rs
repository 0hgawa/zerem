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
//! There is no authentication and it does not need any: nothing outside this
//! machine can reach a loopback socket, and everything on this machine that
//! could guess the URL could read the files directly anyway.
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
use zerem_core::TorrentId;

/// How much is moved per write.
///
/// 256 KiB: large enough that the loop is not the cost, small enough that a
/// player seeking away does not wait for a megabyte it has stopped wanting.
const CHUNK: usize = 256 * 1024;

/// A request line longer than this is not a request.
const LIMIT: usize = 8 * 1024;

/// What the window needs to build a URL, and what the loop needs to serve one.
pub struct Streamer {
    port: u16,
}

impl Streamer {
    /// Bind, and start answering.
    ///
    /// `None` when the loopback will not give a port, which is a machine with
    /// no working network stack at all — the app runs, and the Play row is not
    /// offered.
    pub async fn start(lookup: Lookup) -> Option<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .inspect_err(|e| tracing::warn!("no local port for streaming: {e}"))
            .ok()?;
        let port = listener.local_addr().ok()?.port();

        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let lookup = lookup.clone();
                // One task per player. A second request while the first is
                // streaming is what a seek looks like, and it must not wait.
                tokio::spawn(async move {
                    if let Err(e) = serve(socket, &lookup).await {
                        tracing::debug!("stream connection ended: {e:#}");
                    }
                });
            }
        });

        tracing::info!(port, "streaming on the loopback");
        Some(Self { port })
    }

    /// The loopback port it answers on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }
}

/// How the server finds a torrent, without holding the session's lock.
///
/// A closure rather than a reference to the session: the session is `&mut self`
/// on a single task and the server runs on its own, so what crosses is a
/// question and its answer.
pub type Lookup = Arc<dyn Fn(TorrentId) -> Option<Arc<ManagedTorrent>> + Send + Sync>;

async fn serve(socket: TcpStream, lookup: &Lookup) -> anyhow::Result<()> {
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);

    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let Some((id, file)) = target(&line) else {
        return refuse(&mut write, "400 Bad Request").await;
    };

    // The headers, for the one that matters.
    let mut from = 0u64;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).await? == 0 || header.trim().is_empty() {
            break;
        }
        if header.len() > LIMIT {
            return refuse(&mut write, "431 Request Header Fields Too Large").await;
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
    if from >= total && total > 0 {
        return refuse(&mut write, "416 Range Not Satisfiable").await;
    }
    if from > 0 {
        stream.seek(std::io::SeekFrom::Start(from)).await?;
    }

    let kind = zerem_core::mime_of(&name).unwrap_or("application/octet-stream");
    let remaining = total - from;
    let head = if from > 0 {
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

/// The torrent and file a request line names, for `GET /t/{id}/{file}`.
fn target(line: &str) -> Option<(TorrentId, usize)> {
    let path = line.strip_prefix("GET ")?.split_whitespace().next()?;
    let rest = path.strip_prefix("/t/")?;
    let (id, file) = rest.split_once('/')?;
    Some((TorrentId(id.parse().ok()?), file.parse().ok()?))
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
    use super::{range_start, target};
    use zerem_core::TorrentId;

    #[test]
    fn a_request_names_a_torrent_and_a_file() {
        assert_eq!(target("GET /t/7/2 HTTP/1.1\r\n"), Some((TorrentId(7), 2)));
    }

    #[test]
    fn anything_else_is_not_a_request_this_answers() {
        assert_eq!(target("GET / HTTP/1.1\r\n"), None);
        assert_eq!(target("POST /t/7/2 HTTP/1.1\r\n"), None, "read only, on purpose");
        assert_eq!(target("GET /t/7 HTTP/1.1\r\n"), None, "a torrent is not a file");
        assert_eq!(target("GET /t/x/2 HTTP/1.1\r\n"), None);
        assert_eq!(target(""), None);
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
