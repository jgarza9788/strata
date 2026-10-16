// SPDX-License-Identifier: MIT

use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
};

use crate::services::{PathMatcher, PathQuery, TextScore, fold_for_search};

const MAX_SCANNED_ENTRIES: usize = 10_000;
const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PathEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Default)]
pub(crate) struct PathListing {
    /// The typed folder itself, offered when nothing follows its last `/`.
    pub folder: Option<PathBuf>,
    pub entries: Vec<PathEntry>,
    /// The listed folder, abbreviated under home.
    pub hint: String,
}

/// Queries that browse the filesystem rather than search commands.
pub(crate) fn is_path_query(query: &str) -> bool {
    let query = query.trim_start();
    query.starts_with('/')
        || query.starts_with('~')
        || query.starts_with("./")
        || query.starts_with("../")
}

/// Entries of the folder `query` names that fuzzily match the text after its last
/// `/`, best match first and folders before files among equal matches.
pub(crate) fn list(
    query: &str,
    current_dir: Option<&Path>,
    home: &Path,
    show_hidden: bool,
) -> PathListing {
    let query = match query.trim() {
        "~" => "~/",
        query => query,
    };
    let Some(split) = crate::ui::browser::split_path_input(query, current_dir, home) else {
        return PathListing::default();
    };
    let base_dir = crate::model::lexically_normalized(&split.base_dir);
    let Ok(read) = fs::read_dir(&base_dir) else {
        return PathListing::default();
    };
    let include_hidden = show_hidden || split.leaf.starts_with('.');
    let mut matcher = PathMatcher::new(&PathQuery::parse(split.leaf));
    let fuzzy = !split.leaf.trim().is_empty();
    let mut ranked = read
        .flatten()
        .take(MAX_SCANNED_ENTRIES)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !include_hidden && name.starts_with('.') {
                return None;
            }
            let folded = fold_for_search(&name);
            let score = if fuzzy {
                matcher.score(&folded, 0)?
            } else {
                TextScore::default()
            };
            let path = entry.path();
            let is_dir = entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() || (kind.is_symlink() && path.is_dir()));
            Some((
                (Reverse(score), !is_dir, folded),
                PathEntry { name, path, is_dir },
            ))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(left, _), (right, _)| left.cmp(right));
    ranked.truncate(MAX_ENTRIES);
    PathListing {
        folder: split.leaf.is_empty().then(|| base_dir.clone()),
        entries: ranked.into_iter().map(|(_, entry)| entry).collect(),
        hint: crate::ui::browser::compact_hint(&base_dir, home),
    }
}

#[cfg(test)]
mod tests;
