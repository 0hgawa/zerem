//! Every string the window draws, against every catalogue that claims to
//! translate it.
//!
//! Slint fails a missing translation *silently* — the source string is drawn
//! and nothing is logged. That is the right behaviour at runtime and the wrong
//! one to find out about from a screenshot, so the two sides are compared here
//! instead: a `@tr()` with no `msgid` is a gap, and a `msgid` with no `@tr()`
//! is a line somebody translated for nothing after the UI moved on.

use std::collections::BTreeSet;

/// Every `.slint` in the tree, found rather than listed.
///
/// It was a list of `include_str!`, and a list is a thing to forget: the window
/// controls had four labels nobody had ever checked because `chrome.slint` was
/// never added to it, and three of them were untranslated for as long as they
/// had existed. A new file is exactly when the check matters and exactly when
/// somebody is thinking about something else.
///
/// Read at run time, so `include_str!` cannot be used and a file with no prose
/// in it costs a read and contributes nothing — which is cheaper than a list
/// that is right today.
fn sources() -> Vec<(String, String)> {
    let mut found = Vec::new();
    walk(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui").as_path(), &mut found);
    assert!(found.len() >= 8, "only {} .slint files were found; is the path right?", found.len());
    found
}

fn walk(at: &std::path::Path, into: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(at) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, into);
        } else if path.extension().is_some_and(|e| e == "slint") {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            into.push((name, std::fs::read_to_string(&path).unwrap_or_default()));
        }
    }
}

/// Every catalogue the binary ships, read from disk and named by its tag.
///
/// `include_str!` named one file, and one file is what got checked: pt-BR was
/// held to every rule below while the nine languages added beside it were held
/// to none. The list comes from `SHIPPED` so a language cannot be shipped and
/// unchecked at the same time — the folder has to be there for the build to
/// bundle it, and it has to be listed for the app to offer it.
fn catalogues() -> Vec<(&'static str, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    zerem_core::language::SHIPPED
        .iter()
        .filter(|(tag, _)| *tag != "en")
        .map(|(tag, _)| {
            let at = root.join("lang").join(tag).join("LC_MESSAGES/zerem.po");
            let text = std::fs::read_to_string(&at)
                .unwrap_or_else(|why| panic!("{tag} is shipped but {} would not open: {why}", at.display()));
            (*tag, text)
        })
        .collect()
}

/// Every `@tr("…")` in the UI.
fn marked() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (_, text) in sources() {
        let mut rest = text.as_str();
        while let Some(at) = rest.find("@tr(\"") {
            rest = &rest[at + 5..];
            let Some(end) = rest.find('"') else { break };
            found.insert(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    found
}

/// Every `msgid` in a catalogue, minus the empty one that carries the header.
fn translated(po: &str) -> BTreeSet<String> {
    po.lines()
        .filter_map(|line| line.trim().strip_prefix("msgid \"")?.strip_suffix('"'))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn nothing_the_window_says_is_left_untranslated() {
    let ui = marked();
    for (tag, po) in catalogues() {
        let po = translated(&po);
        let missing: Vec<&String> = ui.difference(&po).collect();
        assert!(
            missing.is_empty(),
            "{tag} is missing {} of them:\n  {}",
            missing.len(),
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
        );
    }
}

#[test]
fn nothing_is_translated_that_the_window_no_longer_says() {
    // The other direction, and the one that rots quietly: a string edited in
    // the `.slint` leaves its old translation behind, still looking correct,
    // while the new wording falls back to English on screen.
    let ui = marked();
    for (tag, po) in catalogues() {
        let po = translated(&po);
        let stale: Vec<&String> = po.difference(&ui).collect();
        assert!(
            stale.is_empty(),
            "{tag} translates {} strings the UI does not have — edited or removed?\n  {}",
            stale.len(),
            stale.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
        );
    }
}

#[test]
fn the_ui_has_prose_to_translate_at_all() {
    // Guards the parser rather than the translations: a `@tr` syntax change
    // would empty the set above and make both tests pass by saying nothing.
    assert!(marked().len() > 40, "only {} strings found — did the marking change?", marked().len());
}

#[test]
fn no_string_is_translated_twice() {
    // A duplicate `msgid` makes the file invalid, and neither test above can
    // see one: both compare sets, and a set quietly swallows the second copy.
    // This nearly shipped — "Paused" is both a state name and a status-bar
    // counter, and adding the counter added a second entry for it.
    for (tag, po) in catalogues() {
        let mut seen = BTreeSet::new();
        let repeated: Vec<&str> = po
            .lines()
            .filter_map(|line| line.trim().strip_prefix("msgid \"")?.strip_suffix('"'))
            .filter(|id| !id.is_empty() && !seen.insert((*id).to_owned()))
            .collect();
        assert!(repeated.is_empty(), "{tag} translates these twice:\n  {}", repeated.join("\n  "));
    }
}

/// Every string the Rust side passes through `tr` is actually in the catalogue.
///
/// The `.slint` had this covered from the start and the Rust side never did, so
/// five status-bar messages sat in English inside an app that claims to be
/// translated — nobody noticed because nothing looked. `tr` returns its
/// argument when it finds no entry, which is the right behaviour and the reason
/// the gap was silent.
#[test]
fn nothing_the_rust_side_says_falls_back_to_english() {
    // Gathered once, then asked of every language: the phrases are the same set
    // whichever catalogue is answering.
    let mut phrases: Vec<(String, &'static str)> = Vec::new();
    for file in rust_sources(std::path::Path::new("src")) {
        let text = std::fs::read_to_string(&file).expect("read a source file");
        for phrase in marked_in_rust(&text) {
            // Leaked so the borrow outlives the loop; a test process is about
            // to end and this is a handful of short strings.
            phrases.push((file.display().to_string(), Box::leak(phrase.into_boxed_str())));
        }
    }

    // `has` rather than a comparison with what `tr` gives back. A language is
    // allowed to keep a word — "Error" is Spanish, "System" is German — and
    // comparing strings would call that a gap and send somebody looking for a
    // bug that is a translation.
    let mut untranslated: Vec<String> = Vec::new();
    for (tag, _) in zerem_core::language::SHIPPED.iter().filter(|(tag, _)| *tag != "en") {
        zerem_core::text::set(tag);
        for (file, phrase) in &phrases {
            if !zerem_core::text::has(phrase) {
                untranslated.push(format!("{tag} — {file}: {phrase}"));
            }
        }
    }

    zerem_core::text::set("en");
    assert!(
        untranslated.is_empty(),
        "{} strings reach the window untranslated:\n  {}",
        untranslated.len(),
        untranslated.join("\n  ")
    );
}

/// Every `.rs` file under a folder, however deep.
fn rust_sources(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found
}

/// The literals handed to `tr`, which is the only marker the Rust side has.
fn marked_in_rust(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("tr(\"") {
        rest = &rest[at + 4..];
        if let Some(end) = rest.find('"') {
            found.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    found
}
