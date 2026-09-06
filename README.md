<div align="center">

# Zerem

**A native BitTorrent client. Light, fast, and with a table that never stutters.**

Rust + [Slint](https://slint.dev) · one process · no WebView · software rendering

![Rust](https://img.shields.io/badge/Rust-1.98-CE422B)
![Slint](https://img.shields.io/badge/UI-Slint%201.17-2379F4)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0078D6)
![Licence](https://img.shields.io/badge/licence-GPLv3-blue)

[Roadmap](ROADMAP.md) · [Architecture](ARCHITECTURE.md) · [Benchmarks](docs/benchmarks.md) · [Releasing](docs/releasing.md)

</div>

---

> **Status: usable every day.** The engine is [librqbit](https://github.com/ikatson/rqbit),
> vendored and patched. Everything below works end to end; what is left is
> polish and the items in the [roadmap](ROADMAP.md).

## What it does

**Adding**
Paste a magnet with `Ctrl+V` or open a `.torrent`. The add dialog shows the file
list first, so nothing downloads that you did not ask for, and it says up front
when the download will not fit on the disk. A watched folder picks up `.torrent`
files dropped into it.

**While it runs**
A queue with a maximum number of active torrents. Categories, each with its own
folder. Per-file selection you can change at any time, and a *fetch this first*
pin for the one file you actually want now. Country flags beside every peer.
A torrent that stalls says why, in words.

**Speed**
Global limits plus a second, slower pair you switch to from the footer when the
line is needed for something else. uTP yields to other traffic. Protocol
encryption (MSE/PE) — off, preferred or required.

**Finishing**
Move to another folder when complete. Stream a file to your media player
*before* it finishes — the engine fetches the pieces that player is about to
want. Signed self-update over minisign.

**The window**
Light and dark themes. Eleven languages. Tray with single-instance handoff.
A details panel with files as a tree, peers, and a live speed sparkline.
Screen-reader labels throughout, and contrast measured rather than eyeballed.

## Install

Download `Zerem-Setup.exe` from the [latest release](https://github.com/0hgawa/zerem/releases/latest).
It installs under `%LOCALAPPDATA%\Programs\Zerem` — no administrator, no UAC —
and registers `magnet:` and `.torrent`, which is what a bare executable in your
Downloads folder cannot do.

The app checks for updates on its own and verifies the signature before
replacing itself.

## Build

```bat
dev.bat              debug, with per-tick logging
dev.bat release      release — the only build worth measuring
```

Or directly:

```powershell
cargo run --release
$env:ZEREM_LOG = "debug"   # prints what each tick cost
```

The installer needs [NSIS](https://nsis.sourceforge.io) on `PATH`:

```powershell
powershell -ExecutionPolicy Bypass -File installer\build.ps1
```

## Keyboard

| Key | Action |
|---|---|
| `Ctrl+V` | paste a magnet from the clipboard |
| `Ctrl+O` | open a `.torrent` |
| `Ctrl+F` | jump to the filter; `Esc` goes back to the list |
| `Ctrl+A` | select everything the list is showing |
| `Ctrl+I` | open or close the details panel |
| `Ctrl+,` | preferences |
| `Space` | pause or start the selection |
| `Enter` | open the selection's folder |
| `Delete` | remove the selection — **asks first** |
| `↑` `↓` | move through the list; `Shift` extends the selection |
| `Home` `End` | top and bottom |
| `T` | switch theme |

Right-click a row for start/pause, open folder, copy magnet, remove. Clicking
the header sorts; clicking again reverses. Every row carries its own start/pause
button at the front and remove at the far end — what you do constantly sits
where the hand is, and what cannot be undone sits as far from it as the row
allows.

## Measured, not promised

Method in [benchmarks.md](docs/benchmarks.md).

| | Target | Measured |
|---|---|---|
| Binary | ≤ 19 MB | **18.54 MB** — librqbit is 4.7 of it |
| Cold start to a window | ≤ 300 ms | **92 ms**, median of five |
| Working set, live session | ≤ 60 MB | **57.6 MB** |
| CPU downloading | ≤ 1.5 % of a core per MB/s | **1.45 %** |
| CPU seeding | ≤ 1 % of a core | **0.77 %** |
| Cost of a tick | ≤ 1 ms | **p95 60 µs, max 71 µs** |
| Window hidden | ≤ 0.1 % | **0.000 %** — the tick stops, the downloads do not |

## How it is built

```
zerem (bin)     Slint + bridge/ — knows the engine and the core
zerem-engine    session, tick, the Snapshot/Command contract — never knows Slint
zerem-core      types, sorting, formatting — no async, no I/O, no UI
zerem-mse       protocol encryption, on its own and tested against published vectors
zerem-shell     desktop plumbing (single instance, associations) — does not know Zerem
```

The dependency arrow only ever points down. That is what keeps the core
testable with no runtime and no window, and `zerem-shell` names Zerem nowhere,
so promoting it to a shared repository is moving a folder.

**State goes down, commands come up.** The engine publishes an immutable
`Snapshot` from its own thread; the UI compares it against what it already drew
and notifies only the rows that changed. No click waits for the engine: it is
applied optimistically at once, and the next snapshot is the truth — including
when the engine refused, and then the row puts itself back.

**The window formats nothing.** Sorting, filtering, the file tree, every string
built from a number: all of it arrives ready from Rust. A `.slint` that computes
is a computation done sixty times a second for an answer that changed once.

## Quality gate

Everything here passes before any merge, on Windows and on Linux — see
[CI](.github/workflows/ci.yml):

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets    # pedantic + nursery, -D warnings
cargo test --workspace
cd vendor/librqbit; cargo test --lib -- --test-threads=1
```

The last runs separately because `vendor/` sits outside the workspace — our
lints have no business with somebody else's code. It carries the one test that
proves the encryption on a wire: two sessions, both refusing to speak in the
clear, downloading from each other.

The compiler is pinned in [`rust-toolchain.toml`](rust-toolchain.toml). Without
it, every new Rust release turns CI red in code nobody touched.

## Third-party components

| Component | Terms |
|---|---|
| **Slint** (UI toolkit) | GPLv3, a royalty-free desktop licence, or a commercial one. A distributed binary has to be covered by one of them — see [slint.dev](https://slint.dev). Zerem takes the GPLv3, which is why Zerem is GPLv3. |
| **librqbit** (BitTorrent engine) | Apache-2.0, from [ikatson/rqbit](https://github.com/ikatson/rqbit). A patched copy lives in [`vendor/librqbit`](vendor/librqbit): it has no seam to pass protocol encryption through. Every change is listed in [`vendor/CHANGES.md`](vendor/CHANGES.md). |
| **librqbit-dualstack-sockets** | Apache-2.0, patched for one Windows socket option that was killing the DHT two milliseconds into every launch. Same file. |

## Known limitations

**Private trackers filter by `peer_id`**, and a client of one's own is not on
their whitelist. Protocol encryption removed one of the two reasons they would
refuse Zerem; the `peer_id` is still the other.

Two more live in the engine: piece selection is by file order rather than
**rarest-first**, and there is no *fast extension*. Neither is a decision of
this project — they are what librqbit covers today.

## Licence

**GPLv3** © Ohgawa — the full text is in [LICENSE](LICENSE).

The choice comes from the toolkit. Slint requires a distributed binary to be
covered by one of its licences, and GPLv3 is the one with nothing to keep track
of afterwards: no attribution to remember, no terms to re-read each release. It
is also what every client this one gets compared to already uses — qBittorrent,
Transmission, Deluge.

What it asks, this project already does: the source is public, and whoever
receives the binary receives the right to the source that made it.
