# What this copy of librqbit changes

`vendor/librqbit` is **librqbit 9.0.1 from crates.io with a patch applied**, and
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
