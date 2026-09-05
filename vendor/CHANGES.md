# What these copies of librqbit change

Two crates are vendored, and this file covers both. `vendor/librqbit` is
**librqbit 9.0.1 from crates.io with a patch applied**, and
`[patch.crates-io]` in the workspace manifest is what makes the build use it.

Read this file before taking a new upstream release. Everything below is marked
in the source with `NOT UPSTREAM` so a merge conflict is legible; this is the
list, and the reason.

## Why there is a patch at all

Protocol encryption. A plain BitTorrent connection announces itself in its first
nineteen bytes, equipment that shapes traffic reads them at line rate, and
several private trackers refuse clients that cannot obfuscate. librqbit has none
— a search of the whole crate for `mse`, `rc4` and `encrypt` returns nothing.

It could not be added from outside. `PeerConnection` holds an
`Arc<StreamConnector>`, which is a concrete struct with no trait behind it and
no factory in front of it, and the handshake is written directly after `connect`
returns. There is nowhere to wrap a stream.

**The protocol itself is not here.** It is in `crates/zerem-mse`, where it is
tested against published vectors and against itself over a pipe. What is in this
copy is the glue that can only live here.

## The changes

| File | What |
| --- | --- |
| `Cargo.toml` | A `zerem-mse` path dependency. |
| `src/lib.rs` | Declares `pub mod encryption`. |
| `src/encryption.rs` | **New.** The `Encryption` policy, and the two adapters that put an RC4 keystream between this crate's streams and the wire. |
| `src/peer_connection.rs` | `PeerConnectionOptions` gains `encryption`. `manage_peer_outgoing` runs the encrypted handshake before the BitTorrent one, carrying it inside; `encrypt` is the helper that does it. |
| `src/session.rs` | `merge_peer_opts` carries the policy through. |
| `webui/` | **Deleted.** 557 KB of a TypeScript application behind a feature this build does not enable (`default-features = false`). It never compiled here, and a web front end is not something to carry in a repository that has no use for it. Deleting it is why a fresh unpack diffs as two hundred removals. |

## The two things worth knowing before changing any of it

**The greeting goes inside the exchange.** The BitTorrent handshake is passed to
the encrypted handshake as its payload rather than written after it. That is
what the specification asks for, and it is why nothing an inspecting box could
match on ever reaches the wire. `manage_peer_outgoing` therefore skips its own
`write_all` when the encrypted path ran.

**A failed encrypted handshake cannot be retried on the same connection.** The
peer has already been sent bytes it could not read, and there is no taking them
back. So `Prefer` reconnects and tries again in the clear, which is what every
client that does this does.

## The one that took two attempts: reading a vectored buffer

Over uTP every encrypted connection finished its handshake and then talked
nonsense. The adapters were not obviously at fault — they pass against a writer
that accepts seven bytes at a time, which is more hostile than any socket — and
the first answer was to turn uTP off whenever encryption was on.

That was wrong, and the cause is worth writing down because it is a trap the
trait sets for anyone who implements it.

`IoSliceMut::advance` shrinks a slice from the front. **uTP calls it on every
buffer it fills; TCP does not touch them.** So after `poll_read_vectored`
returns, the slices are not a map of where the bytes landed — on uTP they point
at the space *after* them. The deciphering reader was handing the whole set down
and then walking the slices in order to decipher what had arrived, which on uTP
deciphered empty space and left the real bytes enciphered.

It reads into one buffer at a time now, through its own `poll_read`, where the
deciphering already was. An occasional extra call, and no way to be wrong about
where the bytes are.

`test_e2e_download_tcp_encrypted` and `test_e2e_download_utp_encrypted` are the
proof: real downloads between two sessions, both refusing to speak in the clear,
over each transport.

## Not done yet

Nothing outstanding on the encryption.

## Taking a new upstream release

1. Unpack the new version over `vendor/librqbit`.
2. Re-apply the table above. Every hunk is marked `NOT UPSTREAM`.
3. `cargo test --workspace`, and check that `zerem-mse`'s own tests still pass —
   they are what says the protocol is right, and nothing in this directory
   tests it.

---

# `vendor/librqbit-dualstack-sockets`

**librqbit-dualstack-sockets 0.7.0 from crates.io, with one call added.** It is
the crate every UDP socket in the tree is born in — the DHT's, uTP's, the
trackers', local discovery's — which is why the fix belongs here and not in any
one of them.

## The bug it fixes

Windows answers an inbound ICMP "port unreachable" by failing the **next**
`recv_from` on the socket that sent the datagram, with `WSAECONNRESET`. Nothing
was connected and nothing was reset; it is behaviour from the nineties that
survives for compatibility.

A DHT bootstrap fires at a table full of nodes and some of them are gone. One
ICMP came back, the next read failed, and `librqbit-dht` treats a read error as
the end of the DHT:

```text
21:21:06.870950  INFO  DHT listening on [::]:58485
21:21:06.873392  ERROR dht: dht finished with error: framer failed: Recv(Os { code: 10054 })
```

Two milliseconds of DHT, every launch, on every Windows machine. Everything
after it logged `dht is dead`. A magnet whose trackers answer is unaffected —
which is why this hid for so long — but one whose trackers are down or slow then
has nowhere left to ask, and the window sits on *Fetching the file list from the
swarm…* until the person gives up. That is the report this came from.

## The change

| File | What |
| --- | --- |
| `Cargo.toml` | `windows-sys` on Windows targets. Already in the tree at 0.59, so the build compiles nothing new. |
| `src/lib.rs` | Declares `mod connreset`. |
| `src/connreset.rs` | **New.** Clears `SIO_UDP_CONNRESET`, and is a no-op everywhere else. |
| `src/socket.rs` | `bind_udp` calls it, before the socket can be read from. |
| `src/error.rs` | `Error::UdpConnReset`, so a failing ioctl is named rather than guessed at. |
| `src/bind_device.rs` | An underscore on an unused argument in a Windows stub. Upstream's warning, which `--cap-lints` hid while this came from the registry; CI builds with `-D warnings` and a path dependency has no such cover. |
| `.github/` | **Deleted.** Upstream's own test workflow. GitHub only reads `.github` at the root of a repository, so a nested copy never runs; what it would do is sit in the tree looking like ours. |

## What it measures

Same machine, same seconds after launch, one build apart:

| | DHT died | `dht is dead` | Distinct nodes answering |
| --- | --- | --- | --- |
| Before | 2 ms in | 16 | 0 |
| After | no | 0 | 298 |
