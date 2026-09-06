//! The half of the interface that Rust composes.
//!
//! The other half lives in `lang/*/LC_MESSAGES/zerem.po` and is Slint's to
//! translate. These are here instead because they are built from numbers, and
//! the rule that keeps the table fast is that the `.slint` never formats
//! anything: `@tr` with arguments is evaluated per frame inside a `for`, which
//! is precisely the cost that rule exists to avoid.
//!
//! So there are two catalogues. They are not two copies of one thing — the sets
//! are disjoint, one holding the chrome and the other the sentences made out of
//! figures — but a translator does have to visit both, and saying so beats
//! letting them find out.
//!
//! # Shapes
//!
//! A fixed string goes in [`catalogue::TABLE`] and is looked up by its English
//! self, exactly as gettext does it. A string with a value in it goes in one of
//! the pattern arrays, where `{0}` marks where the value lands — so a language
//! that wants the number last is free to put it last, and Hindi and Turkish
//! both do.
//!
//! `format!` was the obvious thing, and it does not survive eleven languages:
//! it needs a literal, so every sentence became a `match` arm per language, and
//! the check that bought is not the one that matters. The compiler can only
//! tell that the literal in front of it agrees with its arguments — it cannot
//! see that a translator dropped a `{1}`, because the arm they edited still
//! compiles. A test can, across all eleven at once, which is why the patterns
//! are data now.

mod catalogue;

use crate::language::SHIPPED;
use catalogue::{
    CLIPBOARD_FAILED, FETCHING_FIRST, FETCHING_FIRST_WAITING, FILES_PART, FILES_WHOLE, FINISHED_MANY,
    FINISHED_ONE, MATCHED, MOVE_FAILED, SHORTFALL, TABLE, WATCH_FAILED,
};
use std::sync::atomic::{AtomicU8, Ordering};

/// A language the binary carries, and its position in [`SHIPPED`].
///
/// The two orders are the same thing on purpose, and a test says so: it is what
/// lets [`set`] take a tag without a second table to forget to update. Adding a
/// language stays what `language.rs` promises — a folder, a line in `SHIPPED` —
/// plus the translations themselves, and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Lang {
    #[default]
    En,
    Ar,
    De,
    Es,
    Fr,
    Hi,
    Id,
    PtBr,
    Ru,
    Tr,
    Vi,
}

impl Lang {
    /// In [`SHIPPED`] order, which is also the order of every array in
    /// [`catalogue`].
    const ALL: [Self; 11] = [
        Self::En,
        Self::Ar,
        Self::De,
        Self::Es,
        Self::Fr,
        Self::Hi,
        Self::Id,
        Self::PtBr,
        Self::Ru,
        Self::Tr,
        Self::Vi,
    ];

    /// Where this language sits in those arrays.
    const fn at(self) -> usize {
        self as usize
    }
}

/// The language everything here answers in.
///
/// A global, which is what i18n is: threading a language through `fmt::speed`
/// would put a parameter on every call in the app to serve something that
/// changes twice in a session. `Relaxed` because a stale answer for one tick
/// after the user picks a language is a redraw, not a fault.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Adopt a resolved tag — the folder name under `lang/`, as
/// [`crate::language::resolve`] returns it.
///
/// A tag nothing ships is the source language, which is the answer `resolve`
/// gives and the one [`tr`] falls back to.
pub fn set(tag: &str) {
    let at = SHIPPED.iter().position(|(shipped, _)| *shipped == tag).unwrap_or(0);
    CURRENT.store(u8::try_from(at).unwrap_or(0), Ordering::Relaxed);
}

#[must_use]
pub fn current() -> Lang {
    Lang::ALL.get(CURRENT.load(Ordering::Relaxed) as usize).copied().unwrap_or(Lang::En)
}

/// The translation of `source`, or `source` itself.
///
/// Falling back to the original rather than to a marker: a missing entry shows
/// English, which is readable, instead of `???`, which is a bug report from the
/// user about a word they cannot act on.
#[must_use]
pub fn tr(source: &'static str) -> &'static str {
    // English is the key, so there is no lookup at all on the source language.
    let Some(at) = current().at().checked_sub(1) else {
        return source;
    };
    TABLE.binary_search_by(|(key, _)| (*key).cmp(source)).map_or(source, |row| {
        let word = TABLE[row].1[at];
        if word.is_empty() {
            source
        } else {
            word
        }
    })
}

/// Whether the catalogue actually has an entry for `source`.
///
/// [`tr`] cannot answer this and should not try: it returns the source when it
/// finds nothing, and it returns the source *again* for a word a language
/// keeps — "Error" is Spanish, "System" is German. The two are the same string
/// and opposite facts, and the test that guards the catalogue is the one place
/// that needs to tell them apart.
#[must_use]
pub fn has(source: &str) -> bool {
    // Nothing to look up in the source language, and nothing missing either.
    current().at() == 0 || TABLE.binary_search_by(|(key, _)| (*key).cmp(source)).is_ok()
}

/// Which plural form `n` takes in this language.
///
/// An index into the arrays in [`catalogue`], which are as long as the sentence
/// needs rather than as long as the language's grammar — [`pick`] holds at the
/// last form for anything past the end, so a language that says one thing
/// however many there are declares one form and is done.
const fn plural_at(lang: Lang, n: usize) -> usize {
    match lang {
        // The noun does not change after a numeral. Arabic's does, six ways,
        // and its entries are written to not agree with the number instead —
        // which is what Arabic interface translators do.
        Lang::Ar | Lang::Id | Lang::Tr | Lang::Vi => 0,
        // One, a few, many.
        Lang::Ru => {
            let (ten, hundred) = (n % 10, n % 100);
            if ten == 1 && hundred != 11 {
                0
            } else if matches!(ten, 2..=4) && !matches!(hundred, 12..=14) {
                1
            } else {
                2
            }
        }
        // Two forms, and these two count zero as singular.
        Lang::Fr | Lang::Hi => {
            if n > 1 {
                1
            } else {
                0
            }
        }
        Lang::En | Lang::De | Lang::Es | Lang::PtBr => {
            if n == 1 {
                0
            } else {
                1
            }
        }
    }
}

/// The form at `at`, or the last one there is.
fn pick(forms: &'static [&'static str], at: usize) -> &'static str {
    forms.get(at).or_else(|| forms.last()).copied().unwrap_or_default()
}

/// Put `args` where the pattern's `{0}`, `{1}` … say to.
///
/// A placeholder nobody passed an argument for disappears rather than printing
/// itself: a translation with one brace too many should read a word short, not
/// show somebody `{2}`.
fn fill(pattern: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            // An unclosed brace is a typo in a translation, and the rest of the
            // sentence is still worth showing.
            out.push_str(&rest[open..]);
            return out;
        };
        if let Ok(at) = after[..close].parse::<usize>() {
            out.push_str(args.get(at).copied().unwrap_or_default());
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// "12 of 300" — how much of the list a filter is showing.
#[must_use]
pub fn matched(shown: usize, total: usize) -> String {
    fill(MATCHED[current().at()], &[&shown.to_string(), &total.to_string()])
}

/// "1 file first · 11 waiting", and the plural of it.
#[must_use]
pub fn fetching_first(pinned: usize, waiting: usize) -> String {
    let lang = current();
    let forms = if waiting == 0 { FETCHING_FIRST[lang.at()] } else { FETCHING_FIRST_WAITING[lang.at()] };
    fill(pick(forms, plural_at(lang, pinned)), &[&pinned.to_string(), &waiting.to_string()])
}

/// What the add dialog says when the download will not fit.
#[must_use]
pub fn shortfall(short: &str) -> String {
    fill(SHORTFALL[current().at()], &[short])
}

/// A file in the watched folder that could not be added, and which one.
///
/// Named rather than counted: a folder somebody drops torrents into can hold
/// several, and "one of them failed" is not something anybody can act on.
#[must_use]
pub fn watch_failed(name: &str) -> String {
    fill(WATCH_FAILED[current().at()], &[name])
}

/// The clipboard would not answer, and what it said about it.
///
/// A pattern rather than a table row because the reason comes from the
/// operating system and cannot be known in advance — which is exactly why it is
/// worth showing rather than swallowing.
#[must_use]
pub fn clipboard_failed(why: &str) -> String {
    fill(CLIPBOARD_FAILED[current().at()], &[why])
}

/// A finished download that could not be moved to where finished ones are
/// kept, and why not.
///
/// The torrent has not moved and is still in the list, so this is news rather
/// than an emergency — but the disk that filled up or the file somebody has
/// open is something only the person at the machine can fix.
#[must_use]
pub fn move_failed(why: &str) -> String {
    fill(MOVE_FAILED[current().at()], &[why])
}

/// What the status bar says when a download lands.
#[must_use]
pub fn finished_one(name: &str) -> String {
    fill(FINISHED_ONE[current().at()], &[name])
}

/// And when several land in the same second — one line rather than four that
/// push each other off before any is read.
#[must_use]
pub fn finished_many(count: usize) -> String {
    let lang = current();
    fill(pick(FINISHED_MANY[lang.at()], plural_at(lang, count)), &[&count.to_string()])
}

/// "12 files · 3.72 GB", or the same with a choice made in it.
#[must_use]
pub fn files_choice(chosen: usize, total: usize, picked: &str, whole: &str) -> String {
    let lang = current();
    let at = plural_at(lang, total);
    if chosen == total {
        fill(pick(FILES_WHOLE[lang.at()], at), &[&total.to_string(), whole])
    } else {
        fill(pick(FILES_PART[lang.at()], at), &[&chosen.to_string(), &total.to_string(), picked, whole])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        current, fetching_first, files_choice, finished_many, finished_one, matched, plural_at, set,
        shortfall, tr, Lang, CLIPBOARD_FAILED, FETCHING_FIRST, FETCHING_FIRST_WAITING, FILES_PART,
        FILES_WHOLE, FINISHED_MANY, FINISHED_ONE, MATCHED, MOVE_FAILED, SHIPPED, SHORTFALL, TABLE,
        WATCH_FAILED,
    };

    /// One at a time, because the language is one global for the whole process
    /// and cargo runs these on several threads at once.
    ///
    /// Setting it and putting it back was not enough, and the failure was the
    /// worst kind: the suite passed on its own and failed inside a workspace
    /// run, roughly one time in three. A test that fails sometimes teaches
    /// people to run it again, which is how a real failure gets waved through.
    ///
    /// The lock is taken past a poisoning, on purpose. A panic in one of these
    /// leaves the language wherever it was, and every one of them sets what it
    /// wants on the way in — so the second failure would be an artefact of the
    /// first rather than a finding.
    fn with(tag: &str, body: impl FnOnce()) {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set(tag);
        body();
        set("en");
    }

    /// Every one-form pattern array, named, so a new one is listed once.
    fn flat() -> Vec<(&'static str, [&'static str; 11])> {
        vec![
            ("MATCHED", MATCHED),
            ("SHORTFALL", SHORTFALL),
            ("WATCH_FAILED", WATCH_FAILED),
            ("CLIPBOARD_FAILED", CLIPBOARD_FAILED),
            ("MOVE_FAILED", MOVE_FAILED),
            ("FINISHED_ONE", FINISHED_ONE),
        ]
    }

    /// And every one that counts something.
    fn plural() -> Vec<(&'static str, [&'static [&'static str]; 11])> {
        vec![
            ("FINISHED_MANY", FINISHED_MANY),
            ("FETCHING_FIRST", FETCHING_FIRST),
            ("FETCHING_FIRST_WAITING", FETCHING_FIRST_WAITING),
            ("FILES_WHOLE", FILES_WHOLE),
            ("FILES_PART", FILES_PART),
        ]
    }

    /// Which `{n}` a pattern uses, sorted and deduplicated.
    fn slots(pattern: &str) -> Vec<usize> {
        let mut found: Vec<usize> = pattern
            .split('{')
            .skip(1)
            .filter_map(|piece| piece.split_once('}'))
            .filter_map(|(digits, _)| digits.parse().ok())
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    #[test]
    fn the_language_list_and_the_catalogue_are_in_the_same_order() {
        // `set` looks a tag up in SHIPPED and stores the position, so a
        // catalogue in a different order would hand every language its
        // neighbour's words — silently, and only past the one that moved.
        assert_eq!(SHIPPED.len(), Lang::ALL.len(), "a language is in one list and not the other");
        for (at, (tag, _)) in SHIPPED.iter().enumerate() {
            with(tag, || {
                assert_eq!(current().at(), at, "{tag} does not sit where SHIPPED puts it");
            });
        }
    }

    #[test]
    fn every_language_has_every_string() {
        for (key, row) in TABLE {
            for (at, word) in row.iter().enumerate() {
                assert!(!word.is_empty(), "{key:?} has nothing in {:?}", SHIPPED[at + 1].0);
            }
        }
        for (name, array) in flat() {
            for (at, pattern) in array.iter().enumerate() {
                assert!(!pattern.is_empty(), "{name} has nothing in {:?}", SHIPPED[at].0);
            }
        }
        for (name, array) in plural() {
            for (at, forms) in array.iter().enumerate() {
                assert!(!forms.is_empty(), "{name} has no forms in {:?}", SHIPPED[at].0);
                assert!(
                    forms.iter().all(|form| !form.is_empty()),
                    "{name} has an empty form in {:?}",
                    SHIPPED[at].0
                );
            }
        }
    }

    #[test]
    fn the_placeholders_survive_translation() {
        // The check `format!` could never do. A translator drops a `{1}` now
        // and then, and the sentence still reads like a sentence — right up to
        // the moment a number is missing from it on somebody's screen.
        for (name, array) in flat() {
            let want = slots(array[0]);
            for (at, pattern) in array.iter().enumerate() {
                assert_eq!(slots(pattern), want, "{name} in {:?}: {pattern:?}", SHIPPED[at].0);
            }
        }
        for (name, array) in plural() {
            let want = slots(array[0][0]);
            for (at, forms) in array.iter().enumerate() {
                for form in *forms {
                    assert_eq!(slots(form), want, "{name} in {:?}: {form:?}", SHIPPED[at].0);
                }
            }
        }
    }

    #[test]
    fn a_plural_index_always_lands_on_a_form() {
        // `pick` holds at the last form, so this cannot panic. What it would do
        // instead is quietly show one language's grammar with another's count,
        // which is worth a test rather than a comment.
        for (name, array) in plural() {
            for (at, lang) in Lang::ALL.iter().enumerate() {
                let forms = array[at];
                for n in 0..=200usize {
                    let form = plural_at(*lang, n);
                    assert!(
                        form < forms.len() || forms.len() == 1,
                        "{name} in {:?}: {n} asks for form {form} of {}",
                        SHIPPED[at].0,
                        forms.len()
                    );
                }
            }
        }
    }

    #[test]
    fn the_table_is_sorted_because_the_lookup_assumes_it() {
        // An unsorted table fails quietly: the binary search misses some
        // entries and they fall back to English while their neighbours do not.
        let mut sorted = TABLE;
        sorted.sort_unstable_by_key(|(key, _)| *key);
        assert_eq!(sorted, TABLE, "TABLE is out of order");
    }

    #[test]
    fn no_key_is_translated_twice() {
        let mut keys: Vec<&str> = TABLE.iter().map(|(key, _)| *key).collect();
        keys.dedup();
        assert_eq!(keys.len(), TABLE.len(), "a duplicated key shadows one of the two");
    }

    #[test]
    fn the_source_language_is_the_strings_themselves() {
        // No lookup at all in English: the argument *is* the answer, which is
        // what keeps the default path free.
        with("en", || {
            assert_eq!(current(), Lang::En);
            assert_eq!(tr("Downloading"), "Downloading");
            assert_eq!(matched(12, 300), "12 of 300");
        });
    }

    #[test]
    fn a_translated_string_comes_back_translated() {
        with("pt-BR", || {
            assert_eq!(tr("Downloading"), "Baixando");
            assert_eq!(tr("Not enough space on the disk"), "Sem espaço no disco");
            assert_eq!(matched(12, 300), "12 de 300");
        });
        with("de", || assert_eq!(tr("Downloading"), "Wird heruntergeladen"));
        with("ru", || assert_eq!(matched(12, 300), "12 из 300"));
    }

    #[test]
    fn a_language_may_put_the_number_where_it_belongs() {
        // The reason the patterns are data. Hindi and Turkish both name the
        // whole before the part, and no amount of shuffling arguments around a
        // fixed English template would let them.
        with("hi", || assert_eq!(matched(12, 300), "300 में से 12"));
        with("tr", || assert_eq!(matched(12, 300), "300 içinde 12"));
    }

    #[test]
    fn a_string_nobody_translated_falls_back_to_english() {
        // Readable rather than a marker: `???` on screen is a bug report from
        // the user about a word they cannot act on.
        with("pt-BR", || assert_eq!(tr("Some sentence added later"), "Some sentence added later"));
    }

    #[test]
    fn an_unknown_tag_is_the_source_language() {
        with("kl-GL", || assert_eq!(tr("Seeding"), "Seeding"));
    }

    #[test]
    fn the_plural_is_chosen_in_each_language_rather_than_bolted_on() {
        with("en", || {
            assert_eq!(fetching_first(1, 11), "1 file first · 11 waiting");
            assert_eq!(fetching_first(3, 0), "3 files first");
        });
        with("pt-BR", || {
            assert_eq!(fetching_first(1, 11), "1 arquivo na frente · 11 esperando");
            assert_eq!(fetching_first(3, 0), "3 arquivos na frente");
        });
        // Three forms, and the third is not simply "more than four": 21 takes
        // the same shape as 1, which a two-form language cannot say.
        with("ru", || {
            assert_eq!(fetching_first(1, 0), "1 файл первым");
            assert_eq!(fetching_first(3, 0), "3 файла первыми");
            assert_eq!(fetching_first(7, 0), "7 файлов первыми");
            assert_eq!(fetching_first(21, 0), "21 файл первым");
        });
        // One form, because the noun does not change after a numeral.
        with("id", || {
            assert_eq!(fetching_first(1, 0), "1 berkas didahulukan");
            assert_eq!(fetching_first(9, 0), "9 berkas didahulukan");
        });
    }

    #[test]
    fn the_shortfall_reads_as_a_sentence_in_every_language() {
        // Not one template with the words shuffled around it: Portuguese puts a
        // verb where English puts a noun, and a placeholder-swapping translator
        // would have produced something nobody says out loud.
        with("en", || {
            assert_eq!(shortfall("2.51 GB"), "Not enough room in this folder — 2.51 GB short");
        });
        with("pt-BR", || assert_eq!(shortfall("2,51 GB"), "Não cabe nesta pasta — faltam 2,51 GB"));
        with("fr", || {
            assert_eq!(shortfall("2,51 Go"), "Pas assez de place dans ce dossier — il manque 2,51 Go");
        });
    }

    #[test]
    fn a_download_landing_is_named_and_several_are_counted() {
        // One line however many landed at once: four notices in four seconds
        // would push each other off before any of them was read.
        with("en", || {
            assert_eq!(finished_one("Some.Show.S01"), "Some.Show.S01 finished");
            assert_eq!(finished_many(3), "3 downloads finished");
        });
        with("pt-BR", || {
            assert_eq!(finished_one("Some.Show.S01"), "Some.Show.S01 terminou");
            assert_eq!(finished_many(3), "3 downloads terminaram");
        });
        with("ru", || {
            assert_eq!(finished_many(3), "3 загрузки завершены");
            assert_eq!(finished_many(8), "8 загрузок завершено");
        });
    }

    #[test]
    fn a_whole_torrent_is_counted_differently_from_part_of_one() {
        with("en", || {
            assert_eq!(files_choice(12, 12, "", "3.72 GB"), "12 files · 3.72 GB");
            assert_eq!(files_choice(3, 12, "1.44 GB", "3.72 GB"), "3 of 12 files · 1.44 GB of 3.72 GB");
        });
        with("pt-BR", || {
            assert_eq!(files_choice(12, 12, "", "3,72 GB"), "12 arquivos · 3,72 GB");
            assert_eq!(files_choice(3, 12, "1,44 GB", "3,72 GB"), "3 de 12 arquivos · 1,44 GB de 3,72 GB");
        });
    }
}
