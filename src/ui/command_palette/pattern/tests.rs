// SPDX-License-Identifier: MIT

use std::fs;

use super::*;

fn tree() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp dir");
    let photos = root.path().join("photos").join("2024");
    fs::create_dir_all(&photos).expect("dirs");
    fs::create_dir(root.path().join("img_folder")).expect("dir");
    for file in ["IMG_0001.jpg", "IMG_0002.png", "notes.txt"] {
        fs::write(photos.join(file), "").expect("file");
    }
    fs::write(root.path().join("img_9.jpg"), "").expect("file");
    fs::write(root.path().join(".hidden.jpg"), "").expect("file");
    root
}

fn names(entries: &[PathEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

#[test]
fn only_re_prefixed_queries_are_patterns() {
    assert_eq!(pattern_of("re:^a b$"), Some("^a b$"));
    assert_eq!(pattern_of("  re:x"), Some("x"));
    assert_eq!(pattern_of("refresh"), None);
    assert_eq!(pattern_of("~/re:x"), None);
}

#[test]
fn lowercase_patterns_ignore_case_and_uppercase_ones_respect_it() {
    assert!(compile("img").expect("valid").is_match("IMG_1.jpg"));
    assert!(!compile("IMG").expect("valid").is_match("img_1.jpg"));
    assert!(compile("(unclosed").is_err());
}

#[test]
fn names_match_through_the_tree_with_shallow_entries_and_folders_first() {
    let root = tree();
    let found = search(root.path(), &compile(r"^img_\d*").expect("valid"), false);
    assert_eq!(
        names(&found),
        ["img_folder", "img_9.jpg", "IMG_0001.jpg", "IMG_0002.png"]
    );
    assert_eq!(
        found[2].path,
        root.path().join("photos").join("2024").join("IMG_0001.jpg")
    );

    let jpgs = search(root.path(), &compile(r"\.jpg$").expect("valid"), false);
    assert_eq!(names(&jpgs), ["img_9.jpg", "IMG_0001.jpg"]);
    let with_hidden = search(root.path(), &compile(r"\.jpg$").expect("valid"), true);
    assert!(names(&with_hidden).contains(&".hidden.jpg"));
}

#[test]
fn a_pattern_with_a_slash_matches_relative_paths() {
    let root = tree();
    let found = search(
        root.path(),
        &compile(r"^photos/\d{4}/.*\.png$").expect("valid"),
        false,
    );
    assert_eq!(names(&found), ["IMG_0002.png"]);
}
