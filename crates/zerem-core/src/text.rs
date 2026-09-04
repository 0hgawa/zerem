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
//! Two shapes, and the split is not stylistic. A fixed string goes in [`TABLE`]
//! and is looked up by its English self, exactly as gettext does it. A string
//! with a number in it cannot: `format!` needs a literal, so those are written
//! out per language in the functions below, where the word order is free to
//! differ and the compiler still checks the arguments.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Lang {
    #[default]
    En,
    PtBr,
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
pub fn set(tag: &str) {
    let lang = if tag == "pt-BR" { Lang::PtBr } else { Lang::En };
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

#[must_use]
pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::PtBr as u8 {
        Lang::PtBr
    } else {
        Lang::En
    }
}

/// Fixed strings, keyed by the English original — the same contract a `.po`
/// has, and for the same reason: the call site stays readable, and a key that
/// stops matching is a string somebody edited without looking here.
///
/// Sorted, and a test says so: the lookup is a binary search, and an unsorted
/// table would fail quietly, missing some entries while their neighbours work.
const TABLE: [(&str, &str); 19] = [
    ("Another program has one of the files open", "Outro programa está com um dos arquivos aberto"),
    ("Checking", "Verificando"),
    ("Connecting", "Conectando"),
    ("Downloading", "Baixando"),
    ("Error", "Erro"),
    ("Fetching metadata", "Buscando metadata"),
    ("No files selected", "Nenhum arquivo selecionado"),
    ("No limit", "Sem limite"),
    ("No one is sharing", "Ninguém está compartilhando"),
    ("No peers found", "Nenhum peer encontrado"),
    ("No permission to write in the download folder", "Sem permissão para escrever na pasta de destino"),
    ("Not enough space on the disk", "Sem espaço no disco"),
    ("Paused", "Pausado"),
    ("Queued", "Na fila"),
    ("Seeding", "Semeando"),
    ("System", "Sistema"),
    ("That drive is not available", "Essa unidade não está disponível"),
    ("The download folder is not there any more", "A pasta de destino não existe mais"),
    ("The download folder is read-only", "A pasta de destino é somente leitura"),
];

/// The translation of `source`, or `source` itself.
///
/// Falling back to the original rather than to a marker: a missing entry shows
/// English, which is readable, instead of `???`, which is a bug report from the
/// user about a word they cannot act on.
#[must_use]
pub fn tr(source: &'static str) -> &'static str {
    match current() {
        Lang::En => source,
        Lang::PtBr => TABLE.binary_search_by(|(key, _)| (*key).cmp(source)).map_or(source, |at| TABLE[at].1),
    }
}

/// "12 of 300" — how much of the list a filter is showing.
#[must_use]
pub fn matched(shown: usize, total: usize) -> String {
    match current() {
        Lang::En => format!("{shown} of {total}"),
        Lang::PtBr => format!("{shown} de {total}"),
    }
}

/// "1 file first · 11 waiting", and the plural of it.
#[must_use]
pub fn fetching_first(pinned: usize, waiting: usize) -> String {
    match current() {
        Lang::En => {
            let files = if pinned == 1 { "file" } else { "files" };
            if waiting == 0 {
                format!("{pinned} {files} first")
            } else {
                format!("{pinned} {files} first · {waiting} waiting")
            }
        }
        Lang::PtBr => {
            let files = if pinned == 1 { "arquivo" } else { "arquivos" };
            if waiting == 0 {
                format!("{pinned} {files} na frente")
            } else {
                format!("{pinned} {files} na frente · {waiting} esperando")
            }
        }
    }
}

/// What the add dialog says when the download will not fit.
#[must_use]
pub fn shortfall(short: &str) -> String {
    match current() {
        Lang::En => format!("Not enough room in this folder — {short} short"),
        Lang::PtBr => format!("Não cabe nesta pasta — faltam {short}"),
    }
}

/// What the status bar says when a download lands.
#[must_use]
pub fn finished_one(name: &str) -> String {
    match current() {
        Lang::En => format!("{name} finished"),
        Lang::PtBr => format!("{name} terminou"),
    }
}

/// And when several land in the same second — one line rather than four that
/// push each other off before any is read.
#[must_use]
pub fn finished_many(count: usize) -> String {
    match current() {
        Lang::En => format!("{count} downloads finished"),
        Lang::PtBr => format!("{count} downloads terminaram"),
    }
}

/// "12 files · 3.72 GB", or the same with a choice made in it.
#[must_use]
pub fn files_choice(chosen: usize, total: usize, picked: &str, whole: &str) -> String {
    match (current(), chosen == total) {
        (Lang::En, true) => format!("{total} files · {whole}"),
        (Lang::En, false) => format!("{chosen} of {total} files · {picked} of {whole}"),
        (Lang::PtBr, true) => format!("{total} arquivos · {whole}"),
        (Lang::PtBr, false) => format!("{chosen} de {total} arquivos · {picked} de {whole}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        current, fetching_first, files_choice, finished_many, finished_one, matched, set, shortfall, tr,
        Lang, TABLE,
    };

    /// The tests share one process and one global, so each says what it wants
    /// and puts the source language back.
    fn with(tag: &str, body: impl FnOnce()) {
        set(tag);
        body();
        set("en");
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
        let mut keys: Vec<&str> = TABLE.iter().map(|(k, _)| *k).collect();
        keys.dedup();
        assert_eq!(keys.len(), TABLE.len(), "a duplicated key shadows one of the two");
    }

    #[test]
    fn the_source_language_is_the_strings_themselves() {
        // No lookup at all in English: the argument *is* the answer, which is
        // what keeps the default path free.
        set("en");
        assert_eq!(current(), Lang::En);
        assert_eq!(tr("Downloading"), "Downloading");
        assert_eq!(matched(12, 300), "12 of 300");
    }

    #[test]
    fn a_translated_string_comes_back_translated() {
        with("pt-BR", || {
            assert_eq!(tr("Downloading"), "Baixando");
            assert_eq!(tr("Not enough space on the disk"), "Sem espaço no disco");
            assert_eq!(matched(12, 300), "12 de 300");
        });
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
        assert_eq!(fetching_first(1, 11), "1 file first · 11 waiting");
        assert_eq!(fetching_first(3, 0), "3 files first");
        with("pt-BR", || {
            assert_eq!(fetching_first(1, 11), "1 arquivo na frente · 11 esperando");
            assert_eq!(fetching_first(3, 0), "3 arquivos na frente");
        });
    }

    #[test]
    fn the_shortfall_reads_as_a_sentence_in_both() {
        // Not one template with the words shuffled around it: Portuguese puts a
        // verb where English puts a noun, and a placeholder-swapping translator
        // would have produced something nobody says out loud.
        assert_eq!(shortfall("2.51 GB"), "Not enough room in this folder — 2.51 GB short");
        with("pt-BR", || assert_eq!(shortfall("2,51 GB"), "Não cabe nesta pasta — faltam 2,51 GB"));
    }

    #[test]
    fn a_download_landing_is_named_and_several_are_counted() {
        // One line however many landed at once: four notices in four seconds
        // would push each other off before any of them was read.
        assert_eq!(finished_one("Some.Show.S01"), "Some.Show.S01 finished");
        assert_eq!(finished_many(3), "3 downloads finished");
        with("pt-BR", || {
            assert_eq!(finished_one("Some.Show.S01"), "Some.Show.S01 terminou");
            assert_eq!(finished_many(3), "3 downloads terminaram");
        });
    }

    #[test]
    fn a_whole_torrent_is_counted_differently_from_part_of_one() {
        assert_eq!(files_choice(12, 12, "", "3.72 GB"), "12 files · 3.72 GB");
        assert_eq!(files_choice(3, 12, "1.44 GB", "3.72 GB"), "3 of 12 files · 1.44 GB of 3.72 GB");
        with("pt-BR", || {
            assert_eq!(files_choice(12, 12, "", "3,72 GB"), "12 arquivos · 3,72 GB");
            assert_eq!(files_choice(3, 12, "1,44 GB", "3,72 GB"), "3 de 12 arquivos · 1,44 GB de 3,72 GB");
        });
    }
}
