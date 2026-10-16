// SPDX-License-Identifier: MIT

use std::cell::{Cell, RefCell};

use super::*;

fn actions(query: &str) -> Vec<&'static str> {
    filter(query, str::to_owned)
        .into_iter()
        .map(|command| command.action)
        .collect()
}

#[test]
fn empty_query_lists_every_command_in_table_order() {
    let expected = COMMANDS
        .iter()
        .map(|command| command.action)
        .collect::<Vec<_>>();
    assert_eq!(actions(""), expected);
    assert_eq!(actions("  "), expected);
}

#[test]
fn query_words_must_all_match_and_prefixes_rank_first() {
    assert_eq!(actions("refresh"), ["refresh"]);
    assert_eq!(actions("zzz"), Vec::<&str>::new());
    // "tab" starts a word in both tab commands and only appears mid-word nowhere else.
    assert_eq!(actions("tab"), ["new-tab", "close-tab"]);
    assert_eq!(actions("close tab"), ["close-tab"]);
    // A prefix of a word outranks a substring inside one.
    assert_eq!(actions("ter").first(), Some(&"open-terminal"));
    assert!(actions("erm").contains(&"open-terminal"));
}

#[test]
fn matching_ignores_case_and_punctuation() {
    assert_eq!(actions("REFRESH"), ["refresh"]);
    assert_eq!(actions("global-search"), ["search"]);
}

#[test]
fn localized_labels_match_as_well_as_english_ones() {
    let localize = |label: &str| match label {
        "Refresh" => "Aktualisieren".to_owned(),
        other => other.to_owned(),
    };
    let found = |query: &str| {
        filter(query, localize)
            .into_iter()
            .map(|command| command.action)
            .collect::<Vec<_>>()
    };
    assert_eq!(found("aktual"), ["refresh"]);
    assert_eq!(found("refresh"), ["refresh"]);
}

#[test]
fn every_command_targets_a_distinct_window_action() {
    let mut seen = std::collections::HashSet::new();
    for command in COMMANDS {
        assert!(seen.insert(command.action), "{}", command.action);
    }
}

fn mapped_palette(run: Rc<dyn Fn(&'static str)>) -> (CommandPalette, gtk::Window) {
    mapped_palette_opening(run, Rc::new(|_: std::path::PathBuf, _: bool| {}))
}

fn mapped_palette_opening(
    run: Rc<dyn Fn(&'static str)>,
    open_path: OpenPath,
) -> (CommandPalette, gtk::Window) {
    mapped_palette_with(run, open_path, isolated_sources(None, Vec::new()))
}

/// No zoxide, and a history and index limited to what the test provides.
fn isolated_sources(
    history: Option<Rc<NavigationHistory>>,
    index_roots: Vec<std::path::PathBuf>,
) -> PaletteSources {
    PaletteSources {
        history: history.unwrap_or_else(|| {
            Rc::new(NavigationHistory::open(std::path::PathBuf::from(
                "/nonexistent/strata-palette-history.json",
            )))
        }),
        zoxide: false,
        index_scope: Rc::new(move || (index_roots.clone(), Vec::new())),
    }
}

fn mapped_palette_with(
    run: Rc<dyn Fn(&'static str)>,
    open_path: OpenPath,
    sources: PaletteSources,
) -> (CommandPalette, gtk::Window) {
    let palette = CommandPalette::new(run, open_path, Rc::new(|| {}), sources);
    let window = gtk::Window::builder()
        .default_width(900)
        .default_height(600)
        .child(&palette.widget())
        .build();
    window.present();
    palette.show(None, false);
    settle();
    (palette, window)
}

fn settle() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
    while std::time::Instant::now() < deadline {
        while glib::MainContext::default().iteration(false) {}
    }
}

fn press(palette: &CommandPalette, key: gtk::gdk::Key) -> bool {
    press_with(palette, key, gtk::gdk::ModifierType::empty())
}

fn press_with(
    palette: &CommandPalette,
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> bool {
    let controllers = palette.state.layer.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| {
            controllers
                .item(index)
                .and_downcast::<gtk::EventControllerKey>()
        })
        .next()
        .expect("palette key controller")
        .emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
}

#[test]
fn typing_then_enter_runs_the_matching_action_after_closing() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::typing_then_enter_runs_the_matching_action_after_closing",
        || {
            let ran = Rc::new(RefCell::new(Vec::new()));
            let recorded = ran.clone();
            let (palette, window) =
                mapped_palette(Rc::new(move |action| recorded.borrow_mut().push(action)));
            palette.state.field.set_text("refresh");
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();
            assert_eq!(*ran.borrow(), ["refresh"]);
            assert!(!palette.is_visible());
            window.destroy();
        },
    );
}

#[test]
fn arrow_keys_move_the_selection_and_enter_runs_it() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::arrow_keys_move_the_selection_and_enter_runs_it",
        || {
            let ran = Rc::new(RefCell::new(Vec::new()));
            let recorded = ran.clone();
            let (palette, window) =
                mapped_palette(Rc::new(move |action| recorded.borrow_mut().push(action)));
            assert!(press(&palette, gtk::gdk::Key::Down));
            assert!(press(&palette, gtk::gdk::Key::Down));
            assert!(press(&palette, gtk::gdk::Key::Up));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();
            assert_eq!(*ran.borrow(), [COMMANDS[1].action]);
            window.destroy();
        },
    );
}

#[test]
fn escape_and_a_second_open_key_close_without_running_anything() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::escape_and_a_second_open_key_close_without_running_anything",
        || {
            let runs = Rc::new(Cell::new(0));
            let counted = runs.clone();
            let (palette, window) =
                mapped_palette(Rc::new(move |_| counted.set(counted.get() + 1)));
            assert!(press(&palette, gtk::gdk::Key::Escape));
            settle();
            assert!(!palette.is_visible());

            palette.show(None, false);
            settle();
            assert!(palette.is_visible());
            assert!(press_with(
                &palette,
                gtk::gdk::Key::p,
                gtk::gdk::ModifierType::CONTROL_MASK
            ));
            settle();
            assert!(!palette.is_visible());
            assert_eq!(runs.get(), 0);
            window.destroy();
        },
    );
}

#[test]
fn a_query_with_no_matches_shows_status_and_enter_does_nothing() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::a_query_with_no_matches_shows_status_and_enter_does_nothing",
        || {
            let runs = Rc::new(Cell::new(0));
            let counted = runs.clone();
            let (palette, window) =
                mapped_palette(Rc::new(move |_| counted.set(counted.get() + 1)));
            palette.state.field.set_text("zzzz");
            assert_eq!(
                palette.state.results.visible_child_name().as_deref(),
                Some("status")
            );
            assert!(!press(&palette, gtk::gdk::Key::Return));
            settle();
            assert!(palette.is_visible());
            assert_eq!(runs.get(), 0);
            window.destroy();
        },
    );
}

type Opened = Rc<RefCell<Vec<(std::path::PathBuf, bool)>>>;

fn opened_paths() -> (Opened, OpenPath) {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let recorded = opened.clone();
    (
        opened,
        Rc::new(move |path: std::path::PathBuf, is_dir: bool| {
            recorded.borrow_mut().push((path, is_dir));
        }),
    )
}

fn folder_with_files() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir(root.path().join("photos")).expect("dir");
    std::fs::write(root.path().join("photos").join("beach.jpg"), "").expect("file");
    std::fs::write(root.path().join("report.pdf"), "").expect("file");
    root
}

#[test]
fn enter_on_a_typed_path_opens_the_folder_or_reveals_the_file() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::enter_on_a_typed_path_opens_the_folder_or_reveals_the_file",
        || {
            let root = folder_with_files();
            let base = root.path().display().to_string();
            let ran = Rc::new(Cell::new(0));
            let counted = ran.clone();
            let (opened, open_path) = opened_paths();
            let (palette, window) =
                mapped_palette_opening(Rc::new(move |_| counted.set(counted.get() + 1)), open_path);

            palette.state.field.set_text(&format!("{base}/rep"));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();
            assert!(!palette.is_visible());

            palette.show(None, false);
            palette.state.field.set_text(&format!("{base}/"));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();

            assert_eq!(
                *opened.borrow(),
                [
                    (root.path().join("report.pdf"), false),
                    (root.path().to_path_buf(), true)
                ]
            );
            assert_eq!(ran.get(), 0);
            window.destroy();
        },
    );
}

#[test]
fn tab_completes_a_folder_and_lists_its_contents() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::tab_completes_a_folder_and_lists_its_contents",
        || {
            let root = folder_with_files();
            let base = root.path().display().to_string();
            let (opened, open_path) = opened_paths();
            let (palette, window) = mapped_palette_opening(Rc::new(|_| {}), open_path);

            palette.state.field.set_text(&format!("{base}/ph"));
            assert!(press(&palette, gtk::gdk::Key::Tab));
            assert_eq!(palette.state.field.text(), format!("{base}/photos/"));
            assert!(press(&palette, gtk::gdk::Key::Down));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();

            assert_eq!(
                *opened.borrow(),
                [(root.path().join("photos").join("beach.jpg"), false)]
            );
            window.destroy();
        },
    );
}

#[test]
fn tab_is_left_alone_outside_path_mode() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::tab_is_left_alone_outside_path_mode",
        || {
            let (palette, window) = mapped_palette(Rc::new(|_| {}));
            palette.state.field.set_text("tab");
            assert!(!press(&palette, gtk::gdk::Key::Tab));
            assert_eq!(palette.state.field.text(), "tab");
            window.destroy();
        },
    );
}

fn settle_until(done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done() && std::time::Instant::now() < deadline {
        while glib::MainContext::default().iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn row_paths(palette: &CommandPalette) -> Vec<std::path::PathBuf> {
    palette
        .state
        .visible
        .borrow()
        .iter()
        .filter_map(|row| match row {
            PaletteRow::Entry(entry) => Some(entry.path.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plain_query_lists_matching_frecent_folders_after_commands() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::a_plain_query_lists_matching_frecent_folders_after_commands",
        || {
            let root = folder_with_files();
            let history = Rc::new(NavigationHistory::open(root.path().join("history.json")));
            let photos = root.path().join("photos");
            history.record(&photos);
            history.record(&root.path().join("deleted-since"));
            let (opened, open_path) = opened_paths();
            let (palette, window) = mapped_palette_with(
                Rc::new(|_| {}),
                open_path,
                isolated_sources(Some(history), Vec::new()),
            );

            assert_eq!(row_paths(&palette), std::slice::from_ref(&photos));
            palette.state.field.set_text("phts");
            assert_eq!(row_paths(&palette), std::slice::from_ref(&photos));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();
            assert_eq!(*opened.borrow(), [(photos, true)]);
            window.destroy();
        },
    );
}

#[test]
fn a_plain_query_streams_in_files_from_the_index() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::a_plain_query_streams_in_files_from_the_index",
        || {
            let root = folder_with_files();
            let beach = root.path().join("photos").join("beach.jpg");
            let (opened, open_path) = opened_paths();
            let (palette, window) = mapped_palette_with(
                Rc::new(|_| {}),
                open_path,
                isolated_sources(None, vec![root.path().to_path_buf()]),
            );

            palette.state.field.set_text("bech");
            settle_until(|| row_paths(&palette).contains(&beach));
            assert_eq!(row_paths(&palette).first(), Some(&beach));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();
            assert_eq!(*opened.borrow(), [(beach, false)]);
            window.destroy();
        },
    );
}

#[test]
fn a_re_query_searches_the_current_folder_tree_by_regex() {
    crate::test_support::gtk_test(
        "ui::command_palette::tests::a_re_query_searches_the_current_folder_tree_by_regex",
        || {
            let root = folder_with_files();
            let (opened, open_path) = opened_paths();
            let (palette, window) = mapped_palette_opening(Rc::new(|_| {}), open_path);
            palette.show(Some(root.path().to_path_buf()), false);
            settle();

            palette.state.field.set_text("re:(");
            assert_eq!(
                palette.state.results.visible_child_name().as_deref(),
                Some("status")
            );
            assert_eq!(palette.state.status.text(), "Invalid regular expression");

            palette.state.field.set_text(r"re:^b.*\.jpg$");
            let beach = root.path().join("photos").join("beach.jpg");
            settle_until(|| !row_paths(&palette).is_empty());
            assert_eq!(row_paths(&palette), std::slice::from_ref(&beach));
            assert!(press(&palette, gtk::gdk::Key::Return));
            settle();
            assert_eq!(*opened.borrow(), [(beach, false)]);
            window.destroy();
        },
    );
}
