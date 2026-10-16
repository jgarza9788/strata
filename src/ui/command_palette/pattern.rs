// SPDX-License-Identifier: MIT

use std::path::Path;

use regex::{Regex, RegexBuilder};

use super::paths::PathEntry;

const PREFIX: &str = "re:";
const MAX_DEPTH: usize = 8;
const MAX_SCANNED_ENTRIES: usize = 20_000;
const MAX_ENTRIES: usize = 200;

/// The pattern of a `re:` query, untrimmed so spaces can be matched.
pub(crate) fn pattern_of(query: &str) -> Option<&str> {
    query.trim_start().strip_prefix(PREFIX)
}

/// Case-insensitive unless the pattern contains an uppercase letter.
pub(crate) fn compile(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .case_insensitive(!pattern.chars().any(char::is_uppercase))
        .size_limit(1 << 20)
        .build()
}

/// Entries below `root` whose names match, or whose paths relative to `root`
/// match when the pattern contains `/`. Shallower entries and folders come first.
pub(crate) fn search(root: &Path, regex: &Regex, show_hidden: bool) -> Vec<PathEntry> {
    let match_paths = regex.as_str().contains('/');
    let mut found = ignore::WalkBuilder::new(root)
        .follow_links(false)
        .standard_filters(true)
        // `standard_filters` resets hidden-file filtering.
        .hidden(!show_hidden)
        .require_git(false)
        .max_depth(Some(MAX_DEPTH))
        .build()
        .flatten()
        .filter(|entry| entry.depth() > 0)
        .take(MAX_SCANNED_ENTRIES)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let matched = if match_paths {
                let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
                regex.is_match(&relative.to_string_lossy())
            } else {
                regex.is_match(&name)
            };
            if !matched {
                return None;
            }
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            Some((
                (entry.depth(), !is_dir, name.to_lowercase()),
                PathEntry {
                    name,
                    path: entry.into_path(),
                    is_dir,
                },
            ))
        })
        .collect::<Vec<_>>();
    found.sort_by(|(left, _), (right, _)| left.cmp(right));
    found.truncate(MAX_ENTRIES);
    found.into_iter().map(|(_, entry)| entry).collect()
}

#[cfg(test)]
mod tests;
