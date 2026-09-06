//! A torrent's files as a tree, flattened into the rows that get drawn.
//!
//! The panel listed files flat, each printed as `folder › folder › name`. That
//! reads well for a season pack with one `Featurettes` folder in it and falls
//! apart completely on anything larger: a discography of seventeen hundred
//! tracks gave every row the same elided folder prefix, so the list was two
//! hundred identical lines and the name — the only part anybody is looking for
//! — was off the right edge of every one of them.
//!
//! So the folders become rows of their own, and the files sit under them. What
//! every other client does, and what the screenshot of qBittorrent that started
//! this shows.
//!
//! # Why the tree is built here
//!
//! The window formats nothing, and this is the rule's clearest case. A tree
//! rebuilt per frame is a tree rebuilt sixty times a second for an answer that
//! changes when somebody clicks a folder. It arrives ready: one flat list, each
//! row carrying how deep it is, and the `.slint` draws an indent.
//!
//! Flat rather than nested for the same reason the torrent list is flat — a
//! `ListView` can only recycle rows it can count, and a nested model would cost
//! the panel its scrolling on exactly the torrents that need it most.

use std::collections::HashMap;
use std::sync::Arc;

use crate::detail::FileRow;

/// How much of a folder is coming down.
///
/// Three states rather than a `bool`, because a folder where two of eleven
/// tracks are wanted is neither ticked nor empty, and showing it as either is
/// a lie somebody acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wanted {
    /// Nothing under it is being fetched.
    #[default]
    None,
    /// Some of it.
    Part,
    /// All of it.
    All,
}

impl Wanted {
    /// What a folder is, given what one more of its files is.
    const fn and(self, wanted: bool) -> Self {
        match (self, wanted) {
            (Self::All, true) | (Self::Part, _) => {
                if matches!(self, Self::Part) {
                    Self::Part
                } else {
                    Self::All
                }
            }
            (Self::None, false) => Self::None,
            _ => Self::Part,
        }
    }
}

/// One line of the list: a folder, or a file inside one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Node {
    /// How far in to draw it. Zero is the top of the torrent.
    pub depth: usize,
    /// What it is called — the last part of the path and nothing else. This is
    /// the fix: the row shows a name, not a route to one.
    pub name: Arc<str>,
    /// The whole relative path. What a file is opened by, what a screen reader
    /// is read, and the key a folder is remembered as when it is shut.
    pub path: Arc<str>,
    /// Where this file sits in the list the tree was built from, or `None` for
    /// a folder. Every action the window takes is by index, so the row has to
    /// carry the one it came from — sorting for display must not renumber
    /// anything.
    pub at: Option<usize>,
    /// Files underneath, however deep. Zero for a file.
    pub files: usize,
    pub size: u64,
    pub done: u64,
    pub wanted: Wanted,
    /// Set on a file being fetched first, and on every folder above it.
    pub first: bool,
    /// Folders only. A shut folder still draws; what it hides is everything
    /// under it.
    pub open: bool,
}

impl Node {
    /// Whether this row is a folder.
    #[must_use]
    pub const fn is_folder(&self) -> bool {
        self.at.is_none()
    }

    /// Progress in ten-thousandths, as everything else in this crate counts it.
    #[must_use]
    pub const fn progress_bp(&self) -> u64 {
        match (self.done * 10_000).checked_div(self.size) {
            Some(bp) => bp,
            None => 0,
        }
    }
}

/// Where each separator falls in a path.
fn separators(path: &str) -> Vec<usize> {
    path.char_indices().filter(|(_, c)| std::path::is_separator(*c)).map(|(at, _)| at).collect()
}

/// Order for drawing: folders before files at every level, then by name.
///
/// The order a torrent lists its files in is the order they were added to it,
/// which is nobody's idea of a sorted list. Folders first is what every file
/// manager on every desktop does, and it is what makes a discography scannable
/// — the albums are a short list at the top of each level rather than scattered
/// among loose files.
fn before(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut left = a.split(std::path::is_separator).peekable();
    let mut right = b.split(std::path::is_separator).peekable();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(one), Some(two)) => {
                if one == two {
                    continue;
                }
                // Whichever still has path left after this part is a folder.
                let (a_folder, b_folder) = (left.peek().is_some(), right.peek().is_some());
                if a_folder != b_folder {
                    return if a_folder { Ordering::Less } else { Ordering::Greater };
                }
                // Case-insensitively, so `alpha` and `Beta` read as a list
                // rather than as two lists. Exact order breaks the tie, so it
                // is still a total order and the sort is stable in fact.
                return one.to_lowercase().cmp(&two.to_lowercase()).then_with(|| one.cmp(two));
            }
        }
    }
}

/// What a folder adds up to.
#[derive(Default, Clone, Copy)]
struct Sum {
    files: usize,
    size: u64,
    done: u64,
    wanted: Option<Wanted>,
    first: bool,
}

/// The rows to draw, in order.
///
/// `shut` holds the paths of folders somebody has closed. Closed rather than
/// open, so a torrent nobody has touched shows its whole tree — which is what
/// the panel did before this existed, and the answer somebody expects the first
/// time they open one.
#[must_use]
pub fn flatten(files: &[FileRow], shut: &dyn Fn(&str) -> bool) -> Vec<Node> {
    // Every folder's totals, worked out before anything is drawn: a folder's
    // row comes *above* its files, so its size cannot be accumulated on the way
    // past them.
    let mut sums: HashMap<&str, Sum> = HashMap::new();
    for file in files {
        for at in separators(&file.path) {
            let folder = &file.path[..at];
            let sum = sums.entry(folder).or_default();
            sum.files += 1;
            sum.size += file.size;
            sum.done += file.done;
            sum.first |= file.first;
            sum.wanted =
                Some(sum.wanted.map_or(if file.wanted { Wanted::All } else { Wanted::None }, |so_far| {
                    so_far.and(file.wanted)
                }));
        }
    }

    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| before(&files[a].path, &files[b].path));

    let mut out: Vec<Node> = Vec::with_capacity(files.len() + sums.len());
    // The folders currently open above the row being written, by path. Their
    // paths and not the offsets that end them: `Disc 1` and `Disc 2` both end
    // at the same byte, and comparing offsets said they were the same folder.
    let mut stack: Vec<Arc<str>> = Vec::new();
    // The depth of the outermost shut folder, while inside one.
    let mut hidden: Option<usize> = None;

    for at in order {
        let file = &files[at];
        let ends = separators(&file.path);

        // How much of the open stack this file is still inside.
        let same =
            stack.iter().zip(&ends).take_while(|(open, end)| open.as_ref() == &file.path[..**end]).count();
        stack.truncate(same);
        if hidden.is_some_and(|depth| depth >= same) {
            hidden = None;
        }

        for (depth, &end) in ends.iter().enumerate().skip(same) {
            let path = &file.path[..end];
            stack.push(Arc::from(path));
            if hidden.is_some() {
                continue;
            }
            let sum = sums.get(path).copied().unwrap_or_default();
            let open = !shut(path);
            let start = ends.get(depth.wrapping_sub(1)).map_or(0, |before| before + 1);
            out.push(Node {
                depth,
                name: Arc::from(&file.path[start..end]),
                path: Arc::from(path),
                at: None,
                files: sum.files,
                size: sum.size,
                done: sum.done,
                wanted: sum.wanted.unwrap_or_default(),
                first: sum.first,
                open,
            });
            if !open {
                hidden = Some(depth);
            }
        }

        if hidden.is_some() {
            continue;
        }
        let start = ends.last().map_or(0, |end| end + 1);
        out.push(Node {
            depth: ends.len(),
            name: Arc::from(&file.path[start..]),
            path: Arc::clone(&file.path),
            at: Some(at),
            files: 0,
            size: file.size,
            done: file.done,
            wanted: if file.wanted { Wanted::All } else { Wanted::None },
            first: file.first,
            open: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{flatten, Node, Wanted};
    use crate::detail::FileRow;
    use std::sync::Arc;

    fn file(path: &str, size: u64) -> FileRow {
        FileRow { path: Arc::from(path), size, done: 0, wanted: true, first: false }
    }

    fn shape(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| format!("{}{}{}", "  ".repeat(n.depth), n.name, if n.is_folder() { "/" } else { "" }))
            .collect()
    }

    /// Nothing is shut unless a test says so.
    fn all_open(_: &str) -> bool {
        false
    }

    #[test]
    fn a_flat_torrent_is_still_a_flat_list() {
        let files = [file("b.mkv", 2), file("a.mkv", 1)];
        assert_eq!(shape(&flatten(&files, &all_open)), ["a.mkv", "b.mkv"]);
    }

    #[test]
    fn folders_become_rows_and_their_files_sit_under_them() {
        // The bug, in miniature. Every one of these used to be one row reading
        // "Disc 1 › Disc 1 › …" with the track name off the edge.
        let files = [
            file("Disc 1/02. Second.mp3", 2),
            file("Disc 1/01. First.mp3", 1),
            file("Disc 2/01. Third.mp3", 3),
        ];
        assert_eq!(
            shape(&flatten(&files, &all_open)),
            ["Disc 1/", "  01. First.mp3", "  02. Second.mp3", "Disc 2/", "  01. Third.mp3",]
        );
    }

    #[test]
    fn folders_come_before_files_at_every_level() {
        // What makes a large torrent scannable: the albums are a short list at
        // the top of each level rather than scattered among loose files.
        let files = [file("zz.txt", 1), file("Album/01.mp3", 2), file("aa.txt", 3)];
        assert_eq!(shape(&flatten(&files, &all_open)), ["Album/", "  01.mp3", "aa.txt", "zz.txt"]);
    }

    #[test]
    fn a_folder_adds_up_what_is_under_it() {
        let files = [file("Album/01.mp3", 10), file("Album/02.mp3", 30)];
        let nodes = flatten(&files, &all_open);
        let album = &nodes[0];
        assert!(album.is_folder());
        assert_eq!(album.files, 2, "it counts the files, not the rows");
        assert_eq!(album.size, 40);
    }

    #[test]
    fn a_folder_says_when_only_part_of_it_is_coming_down() {
        // Neither ticked nor empty, because it is neither — and drawing it as
        // either is a lie somebody would act on.
        let mut files = [file("Album/01.mp3", 1), file("Album/02.mp3", 1)];
        assert_eq!(flatten(&files, &all_open)[0].wanted, Wanted::All);
        files[1].wanted = false;
        assert_eq!(flatten(&files, &all_open)[0].wanted, Wanted::Part);
        files[0].wanted = false;
        assert_eq!(flatten(&files, &all_open)[0].wanted, Wanted::None);
    }

    #[test]
    fn a_shut_folder_keeps_its_own_row_and_hides_the_rest() {
        let files = [file("Album/01.mp3", 1), file("Album/Extras/02.mp3", 1), file("loose.txt", 1)];
        let nodes = flatten(&files, &|path| path == "Album");
        assert_eq!(shape(&nodes), ["Album/", "loose.txt"]);
        assert!(!nodes[0].open, "and it says it is shut, so the chevron can turn");
        assert_eq!(nodes[0].files, 2, "the count is still the truth about what is in there");
    }

    #[test]
    fn shutting_an_inner_folder_leaves_its_neighbours_alone() {
        let files = [file("Album/Extras/02.mp3", 1), file("Album/01.mp3", 1), file("Other/03.mp3", 1)];
        let nodes = flatten(&files, &|path| path == "Album/Extras" || path == r"Album\Extras");
        assert_eq!(shape(&nodes), ["Album/", "  Extras/", "  01.mp3", "Other/", "  03.mp3"]);
    }

    #[test]
    fn a_file_keeps_the_index_it_came_in_at() {
        // Sorting is for the eye. Every action the window takes is by index, so
        // renumbering here would toggle the wrong file — quietly, and only in
        // torrents whose own order was not already sorted.
        let files = [file("z.mp3", 1), file("a.mp3", 2)];
        let nodes = flatten(&files, &all_open);
        assert_eq!(nodes[0].name.as_ref(), "a.mp3");
        assert_eq!(nodes[0].at, Some(1), "the second file, drawn first");
        assert_eq!(nodes[1].at, Some(0));
    }

    #[test]
    fn a_pinned_file_marks_every_folder_above_it() {
        // So a shut folder still says there is something being fetched first
        // inside it, which is the one thing worth knowing about a shut folder.
        let mut files = [file("Album/Extras/01.mp3", 1)];
        files[0].first = true;
        let nodes = flatten(&files, &all_open);
        assert!(nodes[0].first, "Album");
        assert!(nodes[1].first, "Album/Extras");
    }

    #[test]
    fn nothing_at_all_draws_nothing_at_all() {
        assert!(flatten(&[], &all_open).is_empty());
    }
}
