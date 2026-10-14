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
    let palette = CommandPalette::new(run, Rc::new(|| {}));
    let window = gtk::Window::builder()
        .default_width(900)
        .default_height(600)
        .child(&palette.widget())
        .build();
    window.present();
    palette.show();
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

            palette.show();
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
