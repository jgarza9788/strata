// SPDX-License-Identifier: MIT

use std::fs;

use super::*;

fn tree() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp dir");
    for dir in ["Documents", "docs-archive", "Music", ".config", "work"] {
        fs::create_dir(root.path().join(dir)).expect("dir");
    }
    for file in ["doc.txt", "my-docs.md", ".bashrc", "notes.txt"] {
        fs::write(root.path().join(file), "").expect("file");
    }
    fs::write(root.path().join("work").join("plan.md"), "").expect("file");
    root
}

fn names(listing: &PathListing) -> Vec<&str> {
    listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

#[test]
fn path_queries_start_with_a_root_home_or_relative_marker() {
    for query in ["/", "/usr", "~", "~/Doc", "./x", "../", "  ~/"] {
        assert!(is_path_query(query), "{query}");
    }
    for query in ["", "tab", "refresh", "a/b", ".bashrc", "..x"] {
        assert!(!is_path_query(query), "{query}");
    }
}

#[test]
fn the_leaf_matches_names_fuzzily_and_drops_non_matches() {
    let root = tree();
    let base = root.path().display().to_string();
    let home = Path::new("/nonexistent-home");

    let abbreviated = list(&format!("{base}/dcmts"), None, home, false);
    assert_eq!(names(&abbreviated).first(), Some(&"Documents"));
    assert_eq!(abbreviated.folder, None);

    let mut matched = names(&list(&format!("{base}/doc"), None, home, false))
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    matched.sort();
    assert_eq!(
        matched,
        ["Documents", "doc.txt", "docs-archive", "my-docs.md"]
    );

    assert!(
        list(&format!("{base}/qqq"), None, home, false)
            .entries
            .is_empty()
    );
}

#[test]
fn equally_good_matches_list_folders_before_files() {
    let root = tree();
    let listing = list(
        &format!("{}/notes", root.path().display()),
        None,
        Path::new("/nonexistent-home"),
        false,
    );
    assert_eq!(names(&listing).first(), Some(&"notes.txt"));
    std::fs::create_dir(root.path().join("notes")).expect("dir");
    let listing = list(
        &format!("{}/notes", root.path().display()),
        None,
        Path::new("/nonexistent-home"),
        false,
    );
    assert_eq!(names(&listing)[..2], ["notes", "notes.txt"]);
}

#[test]
fn a_trailing_slash_lists_the_folder_and_offers_the_folder_itself() {
    let root = tree();
    let query = format!("{}/", root.path().display());
    let listing = list(&query, None, Path::new("/nonexistent-home"), false);
    assert_eq!(listing.folder.as_deref(), Some(root.path()));
    assert_eq!(
        names(&listing),
        [
            "docs-archive",
            "Documents",
            "Music",
            "work",
            "doc.txt",
            "my-docs.md",
            "notes.txt"
        ]
    );
}

#[test]
fn hidden_entries_follow_the_preference_unless_the_leaf_starts_with_a_dot() {
    let root = tree();
    let home = root.path();
    assert!(!names(&list("~/", None, home, false)).contains(&".config"));
    let shown = list("~/", None, home, true);
    assert!(names(&shown).contains(&".config") && names(&shown).contains(&".bashrc"));
    assert_eq!(
        names(&list("~/.", None, home, false))[..2],
        [".config", ".bashrc"]
    );
}

#[test]
fn home_and_relative_paths_resolve_against_home_and_the_current_folder() {
    let root = tree();
    let home = root.path();
    let work = home.join("work");

    let tilde = list("~", None, home, false);
    assert_eq!(tilde.folder.as_deref(), Some(home));
    assert_eq!(tilde.hint, "~");

    let nested = list("~/work/", None, home, false);
    assert_eq!(names(&nested), ["plan.md"]);
    assert_eq!(nested.hint, "~/work");

    let current = list("./pl", Some(&work), home, false);
    assert_eq!(names(&current), ["plan.md"]);

    let parent = list("../Mu", Some(&work), home, false);
    assert_eq!(names(&parent), ["Music"]);
    assert_eq!(parent.entries[0].path, home.join("Music"));

    assert!(list("./", None, home, false).entries.is_empty());
}

#[test]
fn a_missing_folder_or_unsupported_shorthand_lists_nothing() {
    let root = tree();
    let home = root.path();
    let missing = list("~/nope/", None, home, false);
    assert!(missing.entries.is_empty() && missing.folder.is_none());
    assert!(list("~other/", None, home, false).entries.is_empty());
}
