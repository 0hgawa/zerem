//! The licence the manifest claims, against the licence in the tree.
//!
//! This exists because the two disagreed. `Cargo.toml` said `MIT` from the
//! first commit, there was no `LICENSE` file at all, and the toolkit underneath
//! required the binary to be covered by one of Slint's — so the manifest was
//! not making a permissive choice, it was making a claim nothing supported.
//!
//! Nothing in a build notices that. `cargo` takes the string on faith, no
//! linter reads it, and the file it names is not required to exist. It would
//! have shipped, and a licence is not the kind of mistake that can be quietly
//! corrected afterwards: by then other people have the binary, under terms that
//! were never true.

/// The identifier the workspace declares, straight out of `Cargo.toml`.
fn declared() -> String {
    let manifest =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the workspace manifest");
    manifest
        .lines()
        .map(str::trim)
        // Comment lines mention licences by name; only an assignment counts.
        .find_map(|line| line.strip_prefix("license = \"")?.strip_suffix('"'))
        .expect("`license` is declared")
        .to_owned()
}

fn licence_text() -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("LICENSE"))
        .expect("a LICENSE file sits beside the manifest")
}

#[test]
fn the_manifest_and_the_file_name_the_same_licence() {
    let (declared, text) = (declared(), licence_text());
    assert_eq!(declared, "GPL-3.0-only", "the declared licence changed; did LICENSE?");
    assert!(text.contains("GNU GENERAL PUBLIC LICENSE"), "LICENSE is not the GPL");
    assert!(text.contains("Version 3, 29 June 2007"), "LICENSE is not version 3");
}

#[test]
fn the_licence_is_the_whole_text_and_not_a_summary() {
    // A one-paragraph "this is GPLv3" notice is not the licence, and a file
    // that only names it grants nothing. The real text runs past six hundred
    // lines and ends with the section that tells people how to apply it.
    let text = licence_text();
    assert!(text.lines().count() > 600, "only {} lines; that is a summary", text.lines().count());
    assert!(text.contains("How to Apply These Terms to Your New Programs"));
    assert!(text.contains("TERMS AND CONDITIONS"));
}

#[test]
fn the_readme_says_what_the_manifest_says() {
    // The README is where a person looks, and it said MIT while the manifest
    // did. Two places naming the same thing is two places that drift, and the
    // one that drifts is the one nobody runs.
    let readme = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("the README");
    assert!(readme.contains("**GPLv3** © Ohgawa"), "the README does not name GPLv3 as the licence");
    assert!(!readme.contains("MIT © Ohgawa"), "the README still claims MIT");
}
