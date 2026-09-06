# Releasing

A release is a tag. Push `v0.2.0` and `.github/workflows/release.yml` builds it,
signs it, writes the update feed and opens a draft release; publishing that draft
is what makes every installed copy of Zerem offer the update.

## The signing key

The app will not install anything that is not signed by one specific key. Its
public half is compiled into `src/update.rs` as `PUBKEY`; the secret half was
generated once and lives in two places and no others:

- `.keys/zerem.key` in this working tree, which `.gitignore` keeps out of the
  repository. **Back it up somewhere that is not this machine.**
- the `MINISIGN_SECRET_KEY` secret on the GitHub repository, as the exact
  contents of that file. It has an empty passphrase, which is what lets CI sign
  without a person present; `MINISIGN_PASSWORD` can be left unset.

It was made with:

```
rsign generate -s .keys/zerem.key -p .keys/zerem.pub -W
```

**Do not rotate it.** Every copy of Zerem anybody has installed verifies against
the value compiled into the build they are running. A new key means none of them
can take an update again — they are stranded on a manual reinstall, and there is
no way to tell them so from inside the app. Losing the secret key has the same
effect, which is why the backup matters more than it looks.

## What the workflow does

1. Runs the same gate every commit runs. A tag is the one build that cannot be
   fixed afterwards, because it is the one that installs itself on other
   people's machines.
2. Builds `zerem.exe` in release, then the NSIS installer around it —
   `installer/build.ps1 -NoBuild -Version <tag>`, so the installer is stamped
   with the tag rather than with `Cargo.toml`.
3. Signs it, then **verifies the signature against the key read out of
   `src/update.rs`** — not against a copy pasted into the workflow. A second
   copy of that key is a second thing to get wrong, and what it buys is a
   release every client downloads and then rejects.
4. Writes `latest.json`:

   ```json
   {
     "version": "0.2.0",
     "platforms": {
       "windows-x86_64": {
         "url": "https://github.com/0hgawa/Zerem/releases/latest/download/zerem.exe",
         "signature": "<the whole .minisig text>"
       }
     }
   }
   ```

5. Signs the installer too, and verifies that as well. Nothing in the app
   checks it — the updater only ever fetches the bare exe — but it is the file
   most people download, and an unsigned installer is one nobody can check came
   from here.
6. Creates a **draft** release with the installer, the binary, both `.minisig`
   files and the feed.

The draft is the safety catch. `releases/latest/download/` resolves to the newest
*published* release, so nothing reaches anybody until a person presses publish.

## Two downloads, and which is which

`Zerem-Setup.exe` is for a person. It installs under `%LOCALAPPDATA%\Programs`
without administrator or UAC, and it is what makes the app able to claim
`magnet:` — the app writes those associations itself on first run and refuses to
unless it is running from that location, so a bare exe left in the Downloads
folder never becomes the handler for a magnet link.

`zerem.exe` is for the updater. It replaces a binary that is already installed,
in place, which is why the feed points at it and not at the installer.

## The version number

The tag decides it; `latest.json` carries it without the `v`. Bump
`version` in the workspace `Cargo.toml` in the same commit as the tag, because
that is the number a running build compares against and the number the About
panel shows.

## Adding a platform

`PLATFORM` in `src/update.rs` is `linux-x86_64` on anything that is not Windows,
and the feed simply has no such key yet — which a Linux build reads as "nothing
new" rather than as a broken feed. Adding one means building it, signing it, and
adding its entry to the `jq` call. Nothing in the app needs to change.
