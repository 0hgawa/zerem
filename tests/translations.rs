//! Every string the window draws, against every catalogue that claims to
//! translate it.
//!
//! Slint fails a missing translation *silently* — the source string is drawn
//! and nothing is logged. That is the right behaviour at runtime and the wrong
//! one to find out about from a screenshot, so the two sides are compared here
//! instead: a `@tr()` with no `msgid` is a gap, and a `msgid` with no `@tr()`
//! is a line somebody translated for nothing after the UI moved on.

use std::collections::BTreeSet;

/// The `.slint` files that hold prose. `icons.slint` is path data and
/// `theme.slint` is colours; neither has a word in it.
const SOURCES: [(&str, &str); 8] = [
    ("add.slint", include_str!("../ui/components/add.slint")),
    ("button.slint", include_str!("../ui/components/button.slint")),
    ("common.slint", include_str!("../ui/components/common.slint")),
    ("detail.slint", include_str!("../ui/components/detail.slint")),
    ("menu.slint", include_str!("../ui/components/menu.slint")),
    ("table.slint", include_str!("../ui/components/table.slint")),
    ("dialogs.slint", include_str!("../ui/windows/dialogs.slint")),
    ("main.slint", include_str!("../ui/windows/main.slint")),
];

const PT_BR: &str = include_str!("../lang/pt-BR/LC_MESSAGES/zerem.po");

/// Every `@tr("…")` in the UI.
fn marked() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (_, text) in SOURCES {
        let mut rest = text;
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
    let (ui, po) = (marked(), translated(PT_BR));
    let missing: Vec<&String> = ui.difference(&po).collect();
    assert!(
        missing.is_empty(),
        "pt-BR is missing {} of them:\n  {}",
        missing.len(),
        missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn nothing_is_translated_that_the_window_no_longer_says() {
    // The other direction, and the one that rots quietly: a string edited in
    // the `.slint` leaves its old translation behind, still looking correct,
    // while the new wording falls back to English on screen.
    let (ui, po) = (marked(), translated(PT_BR));
    let stale: Vec<&String> = po.difference(&ui).collect();
    assert!(
        stale.is_empty(),
        "pt-BR translates {} strings the UI does not have — edited or removed?\n  {}",
        stale.len(),
        stale.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn the_ui_has_prose_to_translate_at_all() {
    // Guards the parser rather than the translations: a `@tr` syntax change
    // would empty the set above and make both tests pass by saying nothing.
    assert!(marked().len() > 40, "only {} strings found — did the marking change?", marked().len());
}
