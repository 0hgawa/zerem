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

## Encryption is TCP only, and that is a finding rather than a choice

Over uTP the handshake completes and the message stream then stalls: both ends
report the other as silent. The adapters are not the cause — they are tested
against a writer that accepts seven bytes at a time, which is more hostile than
anything a socket does — and the cause is not known.

So `EngineConfig::to_session_options` turns uTP off whenever encryption is on,
and `manage_peer_outgoing` refuses to encrypt a connection that is not TCP even
if one arrives. Shipping a transport that quietly fails is worse than shipping
one fewer transport. The Connection panel says so on the card.

`test_e2e_download_tcp_encrypted` is the proof that the rest works: a real
download between two sessions, both refusing to speak in the clear.

## Not done yet

Nothing outstanding on the encryption itself, for TCP. The uTP stall is the one
open question, and it is open — not worked around in a way that hides it.

## Taking a new upstream release

1. Unpack the new version over `vendor/librqbit`.
2. Re-apply the table above. Every hunk is marked `NOT UPSTREAM`.
3. `cargo test --workspace`, and check that `zerem-mse`'s own tests still pass —
   they are what says the protocol is right, and nothing in this directory
   tests it.
