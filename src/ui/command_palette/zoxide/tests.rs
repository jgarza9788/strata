// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn scores_parse_with_padding_and_paths_containing_spaces() {
    let output = " 360.0 /home/me/.config/hypr\n  80.5 /home/me/My Projects/site\n";
    assert_eq!(
        parse_scores(output),
        [
            (PathBuf::from("/home/me/.config/hypr"), 360.0),
            (PathBuf::from("/home/me/My Projects/site"), 80.5),
        ]
    );
}

#[test]
fn malformed_relative_or_non_finite_lines_are_skipped() {
    let output = "garbage\n12.0 relative/path\nNaN /home/me/nan\n\n4.0 /ok\n";
    assert_eq!(parse_scores(output), [(PathBuf::from("/ok"), 4.0)]);
}
