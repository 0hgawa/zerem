//! Signed self-update: where the new build comes from and why it is trusted.
//!
//! The app reads a small JSON feed published with each GitHub release and, when
//! a newer build exists, downloads just the new binary, checks its minisign
//! signature against a public key compiled into this file, and hands it to
//! [`zerem_shell::update`] to swap in. A download that is not signed by the key
//! below is thrown away before it goes anywhere near the disk the running
//! program lives on.
//!
//! Only the binary is fetched. The installer's shortcuts and file associations
//! are left where they are, and a release that needs to change either of those
//! ships a fresh installer instead.
//!
//! # Why a signature and not just HTTPS
//!
//! HTTPS says the bytes came from GitHub unaltered. It says nothing about who
//! put them there, which is the question that matters when the answer decides
//! what code runs as the user next time they open the app. An account taken
//! over, a token leaked from CI, a release asset replaced — every one of those
//! serves a perfectly valid certificate. The signature is made on a key that
//! never touches a build machine unencrypted, so it is the one check a stolen
//! GitHub account cannot pass.

use std::time::Duration;

/// The `owner/repo` that publishes releases, for the page offered as a way out
/// when the in-app update cannot be applied.
const REPO: &str = "0hgawa/Zerem";

/// The release feed: a small JSON manifest uploaded with each release.
///
/// `{ "version": "0.2.0", "platforms": { "windows-x86_64": { "url": …,
/// "signature": … } } }`, where the signature is the raw `.minisig` text for
/// that platform's binary.
const FEED: &str = "https://github.com/0hgawa/Zerem/releases/latest/download/latest.json";

/// The key this build looks itself up under. A build only ever updates from its
/// own platform's entry, so a feed carrying one platform reads as "nothing new"
/// to the others rather than as a broken feed.
const PLATFORM: &str = if cfg!(windows) { "windows-x86_64" } else { "linux-x86_64" };

/// What the download is called while it waits in the temp directory. Windows
/// will not execute an extension-less file and the swap runs it; on Unix an
/// `.exe` would just be a lie.
const STAGED: &str = if cfg!(windows) { "zerem-update.exe" } else { "zerem-update" };

/// The minisign public key the download must be signed with.
///
/// The matching secret key signs the binary at release time and lives nowhere
/// near this repository. **Unchanged once published**: every installed copy
/// verifies against this exact value, so replacing it strands all of them on a
/// manual reinstall.
const PUBKEY: &str = "RWQ23v5aafPrakrjOhIZo2NXp7CIqiZIaZjMHjFYVDMA86R8BwGUUdKc";

/// The environment variable that tells a fresh process which one it replaced.
pub const HANDOFF: &str = "ZEREM_RELAUNCH_AFTER";

/// Long enough for a slow link, short enough that a hung server does not leave
/// the About panel saying "checking" until the app is closed.
const PATIENCE: Duration = Duration::from_secs(30);

/// A newer release: the version, where its binary is, and its signature.
#[derive(Clone)]
pub struct Available {
    pub version: String,
    url: String,
    signature: String,
}

/// `(major, minor, patch)` from a `v1.2.3`-ish tag, ignoring pre-release and
/// build metadata.
fn semver(v: &str) -> (u32, u32, u32) {
    let mut parts = v.trim().trim_start_matches('v').split(['.', '-', '+']);
    let mut next = || parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (next(), next(), next())
}

/// Whether `offered` is a release worth taking over `running`.
///
/// Its own function because it is the whole decision, and because a comparison
/// that is wrong in the permissive direction offers a downgrade as an update.
fn is_newer(offered: &str, running: &str) -> bool {
    semver(offered) > semver(running)
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("Zerem/", env!("CARGO_PKG_VERSION")))
        .timeout(PATIENCE)
        .build()
        .map_err(|e| e.to_string())
}

/// Read the feed. `Ok(None)` means there is nothing newer than this build.
///
/// Blocking — call it from a worker thread, never from the event loop.
///
/// # Errors
///
/// When the feed cannot be fetched, is not the shape it should be, or names no
/// binary for this platform.
pub fn check() -> Result<Option<Available>, String> {
    let response = client()?.get(FEED).send().map_err(|e| e.to_string())?;
    // Nothing released yet, or a release with no feed attached. Not an error to
    // put in front of somebody: there is simply nothing newer than what they
    // are running, which is what the panel would say anyway.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("the update server answered {}", response.status().as_u16()));
    }
    let body = response.text().map_err(|e| e.to_string())?;
    let feed: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let version =
        feed.get("version").and_then(serde_json::Value::as_str).ok_or("the feed names no version")?;
    if !is_newer(version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let platform = feed
        .pointer(&format!("/platforms/{PLATFORM}"))
        .ok_or_else(|| format!("the release has no build for {PLATFORM}"))?;
    let url = platform.get("url").and_then(serde_json::Value::as_str).ok_or("the feed names no download")?;
    let signature =
        platform.get("signature").and_then(serde_json::Value::as_str).ok_or("the release is not signed")?;

    Ok(Some(Available { version: version.to_owned(), url: url.to_owned(), signature: signature.to_owned() }))
}

/// Download it, check the signature, and put it in place of this program.
///
/// Blocking — call it from a worker thread. The caller then relaunches.
///
/// # Errors
///
/// When the download fails, the signature does not match, or the swap is
/// refused. Every one of those is a sentence meant to be shown to somebody.
pub fn fetch_and_apply(update: &Available) -> Result<(), String> {
    let bytes = client()?
        .get(&update.url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;

    // Before it is written anywhere. A file that fails this check should never
    // have existed on disk at all.
    verify(&bytes, &update.signature)?;

    let staged = std::env::temp_dir().join(STAGED);
    std::fs::write(&staged, &bytes).map_err(|e| format!("could not save the download: {e}"))?;
    let swapped = zerem_shell::update::swap(&staged);
    // Either way: a failed swap should not leave a whole verified binary in the
    // temp directory for the rest of the machine's life.
    let _ = std::fs::remove_file(&staged);
    swapped
}

/// Refuse a download that is not signed by the key above.
fn verify(bytes: &[u8], signature: &str) -> Result<(), String> {
    use minisign_verify::{PublicKey, Signature};
    let key = PublicKey::from_base64(PUBKEY).map_err(|e| format!("the built-in key is unusable: {e}"))?;
    let signature = Signature::decode(signature).map_err(|e| format!("the signature is malformed: {e}"))?;
    key.verify(bytes, &signature, false)
        .map_err(|_| "the download is not signed by Zerem — it has not been installed".to_owned())
}

/// The releases page, for when the app cannot update itself.
pub fn release_page() {
    zerem_shell::open_url(&format!("https://github.com/{REPO}/releases"));
}

#[cfg(test)]
mod tests {
    use super::{is_newer, semver, PUBKEY};

    #[test]
    fn a_tag_parses_with_or_without_its_decoration() {
        assert_eq!(semver("v1.2.3"), (1, 2, 3));
        assert_eq!(semver("1.2.3"), (1, 2, 3));
        // Pre-release and build metadata are dropped at the first separator.
        assert_eq!(semver("v2.0.0-rc1"), (2, 0, 0));
        assert_eq!(semver("1.4.0+build7"), (1, 4, 0));
        // A missing component is zero, and so is anything unreadable — which
        // makes a garbled feed compare as older, never as newer.
        assert_eq!(semver("v2"), (2, 0, 0));
        assert_eq!(semver("garbage"), (0, 0, 0));
        assert_eq!(semver(""), (0, 0, 0));
    }

    #[test]
    fn only_a_higher_version_is_offered() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(is_newer("2.0.0", "1.9.9"));
    }

    #[test]
    fn the_same_build_is_not_an_update() {
        // The common answer, and the one a wrong comparison turns into an
        // install loop: offered, applied, offered again on the next launch.
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("v1.0.0", "1.0.0"), "a tag and a version are the same number");
    }

    #[test]
    fn a_downgrade_is_never_offered() {
        // A feed rolled back by hand, or a stale CDN copy of it. Taking the
        // older build would undo an update the user already has.
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn this_build_would_not_offer_itself_an_update() {
        // The feed is compared against exactly this constant, so a typo that
        // made it unparseable would make every published version look newer.
        assert!(semver(env!("CARGO_PKG_VERSION")) > (0, 0, 0), "the crate version does not parse");
    }

    #[test]
    fn the_built_in_key_is_a_key() {
        // A mistyped public key fails at the last possible moment otherwise:
        // after the user has clicked, waited out a download, and been told
        // something that sounds like the release was tampered with.
        assert!(minisign_verify::PublicKey::from_base64(PUBKEY).is_ok());
    }
}
