// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use gtk::{glib, prelude::*};

use crate::services::{NavigationHistory, SearchEvent, SearchHandle, index_trees_with_exclusions};

use super::keys::{
    self, Keys,
    bindings::{
        ARROW_SCOPE, COMMAND_PALETTE, COMPLETE, DOWN, ENTER_ANY_SHIFT, ESCAPE_ANY, JUMP_TO_FOLDER,
        REFRESH, SEARCH, TAB_CLOSE, TAB_NEW, TERMINAL, UP,
    },
};

mod paths;
mod pattern;
mod zoxide;

const MAX_IDLE_FOLDERS: usize = 6;
const MAX_FOLDERS: usize = 8;
const MAX_FILES: usize = 50;

/// A window action the palette can run. `label` is an English message key and
/// `action` the `win.*` action it activates.
pub(crate) struct Command {
    pub action: &'static str,
    pub label: &'static str,
    pub binding: crate::ui::keys::Binding,
    /// The 10xer map leaves the binding unbound, so no hint is shown there.
    pub default_only: bool,
}

pub(crate) const COMMANDS: &[Command] = &[
    Command {
        action: "search",
        label: "Open global search",
        binding: SEARCH,
        default_only: false,
    },
    Command {
        action: "jump-folder",
        label: "Jump to a recent folder",
        binding: JUMP_TO_FOLDER,
        default_only: true,
    },
    Command {
        action: "new-tab",
        label: "New tab",
        binding: TAB_NEW,
        default_only: false,
    },
    Command {
        action: "close-tab",
        label: "Close the active tab",
        binding: TAB_CLOSE,
        default_only: false,
    },
    Command {
        action: "open-terminal",
        label: "Open a terminal",
        binding: TERMINAL,
        default_only: true,
    },
    Command {
        action: "refresh",
        label: "Refresh",
        binding: REFRESH,
        default_only: false,
    },
    Command {
        action: "toggle-arrow-scope",
        label: "Keep arrows in the file list on or off",
        binding: ARROW_SCOPE,
        default_only: true,
    },
];

fn normalized(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scores one query against the labels a command answers to. Every query word
/// must match: a word prefix scores above a plain substring.
fn score(query: &str, labels: &[String]) -> Option<u32> {
    let labels = labels
        .iter()
        .map(|label| normalized(label))
        .collect::<Vec<_>>();
    let mut total = 0;
    for word in query.split_whitespace() {
        let best = labels
            .iter()
            .filter_map(|label| {
                if label.split_whitespace().any(|part| part.starts_with(word)) {
                    Some(2)
                } else {
                    label.contains(word).then_some(1)
                }
            })
            .max()?;
        total += best;
    }
    Some(total)
}

/// Commands matching `query`, best first. An empty query keeps table order, and
/// ties keep it too. `localize` maps a label key to the text the user sees.
pub(crate) fn filter(query: &str, localize: impl Fn(&str) -> String) -> Vec<&'static Command> {
    let query = normalized(query);
    let mut matches = COMMANDS
        .iter()
        .filter_map(|command| {
            let labels = [command.label.to_owned(), localize(command.label)];
            score(&query, &labels).map(|score| (score, command))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    matches.into_iter().map(|(_, command)| command).collect()
}

#[derive(Clone)]
enum PaletteRow {
    Command(&'static Command),
    /// The folder a path query names, opened as typed.
    Folder(PathBuf),
    Entry(paths::PathEntry),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Commands,
    Path,
    Pattern,
}

impl Mode {
    fn of(query: &str) -> Self {
        if pattern::pattern_of(query).is_some() {
            Self::Pattern
        } else if paths::is_path_query(query) {
            Self::Path
        } else {
            Self::Commands
        }
    }
}

/// Receives a chosen path and whether it is a folder, once the palette has closed.
pub type OpenPath = Rc<dyn Fn(PathBuf, bool)>;

/// Where the palette finds folders and files besides its commands.
pub struct PaletteSources {
    pub history: Rc<NavigationHistory>,
    /// Also rank folders from zoxide's database when zoxide is installed.
    pub zoxide: bool,
    /// Roots and exclusions for the file index, read when a file search starts.
    pub index_scope: Rc<dyn Fn() -> (Vec<PathBuf>, Vec<String>)>,
}

struct IndexSearch {
    handle: SearchHandle,
    poll: glib::SourceId,
}

#[derive(Clone)]
pub struct CommandPalette {
    state: Rc<PaletteState>,
}

struct PaletteState {
    layer: gtk::Box,
    field: gtk::Entry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    results: gtk::Stack,
    status: gtk::Label,
    complete_hint: gtk::Label,
    visible: RefCell<Vec<PaletteRow>>,
    /// Rows rendered with the query; background searches append after them.
    base_len: Cell<usize>,
    generation: Cell<u64>,
    current_dir: RefCell<Option<PathBuf>>,
    show_hidden: Cell<bool>,
    zoxide_folders: RefCell<Vec<(PathBuf, f64)>>,
    index: RefCell<Option<IndexSearch>>,
    sources: PaletteSources,
    run: Rc<dyn Fn(&'static str)>,
    open_path: OpenPath,
    dismiss: Rc<dyn Fn()>,
}

impl CommandPalette {
    /// `run` receives the `win.*` action name once the palette has closed.
    pub fn new(
        run: Rc<dyn Fn(&'static str)>,
        open_path: OpenPath,
        dismiss: Rc<dyn Fn()>,
        sources: PaletteSources,
    ) -> Self {
        let layer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        layer.add_css_class("search-backdrop");
        layer.add_css_class("app-modal-layer");
        layer.set_halign(gtk::Align::Fill);
        layer.set_valign(gtk::Align::Fill);
        layer.set_hexpand(true);
        layer.set_vexpand(true);
        layer.set_focusable(true);
        layer.set_visible(false);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("search-dialog");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::Center);
        panel.set_size_request(760, 452);
        panel.set_vexpand(false);
        panel.set_overflow(gtk::Overflow::Hidden);

        let search_bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        search_bar.add_css_class("search-bar");
        search_bar.append(&crate::assets::primary_icon(
            crate::assets::icons::SEARCH,
            20,
        ));
        let field = gtk::Entry::builder()
            .placeholder_text(crate::i18n::tr(
                "Search commands and files, or type a path…",
            ))
            .hexpand(true)
            .build();
        field.add_css_class("search-field");
        crate::ui::accessibility::set_description(
            &field,
            Some(&crate::i18n::tr("Command palette")),
        );
        search_bar.append(&field);
        panel.append(&search_bar);

        let status = gtk::Label::new(Some(&crate::i18n::tr("No results")));
        status.add_css_class("search-status");
        let list = gtk::ListBox::new();
        list.add_css_class("search-results");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&list)
            .build();
        scroller.add_css_class("search-results-scroll");
        let results = gtk::Stack::new();
        results.add_css_class("search-results-stack");
        results.set_size_request(-1, 360);
        results.add_named(&status, Some("status"));
        results.add_named(&scroller, Some("results"));
        panel.append(&results);

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        footer.add_css_class("search-footer");
        let navigation = gtk::Label::new(Some(&super::shortcut_reference::with_keys(
            "%{keys}  navigate",
            Keys::Any(&[UP, DOWN]),
        )));
        navigation.add_css_class("search-hint");
        let complete_hint = gtk::Label::new(Some(&super::shortcut_reference::with_keys(
            "%{keys}  complete",
            Keys::One(COMPLETE),
        )));
        complete_hint.add_css_class("search-hint");
        complete_hint.set_visible(false);
        let open = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        open.set_valign(gtk::Align::Center);
        open.append(&crate::assets::primary_icon(
            crate::assets::icons::CORNER_DOWN_LEFT,
            13,
        ));
        open.append(&gtk::Label::new(Some(&crate::i18n::tr("run"))));
        open.add_css_class("search-hint");
        footer.append(&navigation);
        footer.append(&complete_hint);
        footer.append(&open);
        panel.append(&footer);
        super::modal::layout::install(&layer, &panel);

        let state = Rc::new(PaletteState {
            layer,
            field,
            list,
            scroller,
            results,
            status,
            complete_hint,
            visible: RefCell::new(Vec::new()),
            base_len: Cell::new(0),
            generation: Cell::new(0),
            current_dir: RefCell::new(None),
            show_hidden: Cell::new(false),
            zoxide_folders: RefCell::new(Vec::new()),
            index: RefCell::new(None),
            sources,
            run,
            open_path,
            dismiss,
        });

        let changed = Rc::downgrade(&state);
        state.field.connect_changed(move |field| {
            if let Some(state) = changed.upgrade() {
                render(&state, &field.text());
            }
        });
        let entered = Rc::downgrade(&state);
        state.field.connect_activate(move |_| {
            if let Some(state) = entered.upgrade() {
                activate_selected(&state);
            }
        });
        let activated = Rc::downgrade(&state);
        state.list.connect_row_activated(move |_, row| {
            if let Some(state) = activated.upgrade() {
                activate_position(&state, row.index());
            }
        });
        let pressed = gtk::EventControllerKey::new();
        pressed.set_propagation_phase(gtk::PropagationPhase::Capture);
        let keyed = Rc::downgrade(&state);
        keys::on_press(&pressed, move |_, press| {
            let Some(state) = keyed.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if press.is(ESCAPE_ANY) || press.is(COMMAND_PALETTE) {
                hide(&state);
                return glib::Propagation::Stop;
            }
            if press.is_any(&[DOWN, UP]) {
                move_selection(&state, if press.is(DOWN) { 1 } else { -1 });
                return glib::Propagation::Stop;
            }
            if press.is(COMPLETE)
                && (complete_selected(&state) || Mode::of(&state.field.text()) == Mode::Path)
            {
                return glib::Propagation::Stop;
            }
            if press.is(ENTER_ANY_SHIFT) && activate_selected(&state) {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        state.layer.add_controller(pressed);

        let click_state = Rc::downgrade(&state);
        let click_panel = panel.clone();
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(move |_, _, x, y| {
            let Some(state) = click_state.upgrade() else {
                return;
            };
            let on_panel = click_panel
                .compute_bounds(&state.layer)
                .is_some_and(|bounds| {
                    bounds.contains_point(&gtk::graphene::Point::new(x as f32, y as f32))
                });
            if !on_panel {
                hide(&state);
            }
        });
        state.layer.add_controller(click);

        Self { state }
    }

    pub fn widget(&self) -> gtk::Widget {
        self.state.layer.clone().upcast()
    }

    /// `current_dir` anchors `./`, `../` and `re:` searches; `show_hidden` mirrors
    /// the browser.
    pub fn show(&self, current_dir: Option<PathBuf>, show_hidden: bool) {
        let state = &self.state;
        state.current_dir.replace(current_dir);
        state.show_hidden.set(show_hidden);
        state.field.set_text("");
        render(state, "");
        state.layer.set_visible(true);
        super::browser::animate_in(&state.layer);
        state.field.grab_focus_without_selecting();
        if state.sources.zoxide {
            let weak = Rc::downgrade(state);
            zoxide::load(move |folders| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                state.zoxide_folders.replace(folders);
                let query = state.field.text();
                if state.layer.is_visible() && Mode::of(&query) == Mode::Commands {
                    render(&state, &query);
                }
            });
        }
    }

    pub fn hide(&self) {
        hide(&self.state);
    }

    pub fn is_visible(&self) -> bool {
        self.state.layer.is_visible()
    }
}

fn render(state: &Rc<PaletteState>, query: &str) {
    state.generation.set(state.generation.get().wrapping_add(1));
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }
    let mode = Mode::of(query);
    let (rows, status) = match mode {
        Mode::Commands => (command_rows(state, query), "No results"),
        Mode::Path => (path_rows(state, query), "No matching files or folders"),
        Mode::Pattern => (Vec::new(), start_pattern_search(state, query)),
    };
    state.status.set_text(&crate::i18n::tr(status));
    state.base_len.set(rows.len());
    state.visible.replace(rows);
    state.scroller.vadjustment().set_value(0.0);
    show_rows(state);
    if mode == Mode::Commands && !query.trim().is_empty() {
        query_index(state, query);
    }
}

fn show_rows(state: &PaletteState) {
    let (empty, has_entries) = {
        let visible = state.visible.borrow();
        (
            visible.is_empty(),
            visible
                .iter()
                .any(|row| matches!(row, PaletteRow::Entry(_))),
        )
    };
    state
        .results
        .set_visible_child_name(if empty { "status" } else { "results" });
    state.complete_hint.set_visible(has_entries);
    if state.list.selected_row().is_none()
        && let Some(row) = state.list.row_at_index(0)
    {
        state.list.select_row(Some(&row));
    }
}

fn command_rows(state: &PaletteState, query: &str) -> Vec<PaletteRow> {
    let tenxer = super::tenxer_mode::chrome_suppressed();
    let commands = filter(query, crate::i18n::tr);
    for command in &commands {
        let hint = if command.default_only && tenxer {
            String::new()
        } else {
            keys::label(&Keys::One(command.binding))
        };
        state
            .list
            .append(&command_row(&crate::i18n::tr(command.label), &hint));
    }
    let mut rows = commands
        .into_iter()
        .map(PaletteRow::Command)
        .collect::<Vec<_>>();
    let limit = if query.trim().is_empty() {
        MAX_IDLE_FOLDERS
    } else {
        MAX_FOLDERS
    };
    let folders = state
        .sources
        .history
        .search_with(query, &state.zoxide_folders.borrow());
    for item in folders
        .into_iter()
        .filter(|item| item.path.is_dir())
        .take(limit)
    {
        let entry = paths::PathEntry {
            name: item.name,
            path: item.path,
            is_dir: true,
        };
        state.list.append(&entry_row(&entry));
        rows.push(PaletteRow::Entry(entry));
    }
    rows
}

fn path_rows(state: &PaletteState, query: &str) -> Vec<PaletteRow> {
    let listing = paths::list(
        query,
        state.current_dir.borrow().as_deref(),
        &glib::home_dir(),
        state.show_hidden.get(),
    );
    let mut rows = Vec::with_capacity(listing.entries.len() + 1);
    if let Some(folder) = listing.folder {
        state.list.append(&path_row(
            &listing.hint,
            &folder,
            crate::assets::icons::FOLDER_OPEN,
            "",
        ));
        rows.push(PaletteRow::Folder(folder));
    }
    for entry in listing.entries {
        state.list.append(&entry_row(&entry));
        rows.push(PaletteRow::Entry(entry));
    }
    rows
}

/// Starts a background regex search and returns the status shown until it ends.
fn start_pattern_search(state: &Rc<PaletteState>, query: &str) -> &'static str {
    let pattern = pattern::pattern_of(query).unwrap_or_default();
    if pattern.is_empty() {
        return "No results";
    }
    let Ok(regex) = pattern::compile(pattern) else {
        return "Invalid regular expression";
    };
    let Some(root) = state.current_dir.borrow().clone() else {
        return "No results";
    };
    let show_hidden = state.show_hidden.get();
    let generation = state.generation.get();
    let weak = Rc::downgrade(state);
    glib::spawn_future_local(async move {
        let Ok(found) =
            gtk::gio::spawn_blocking(move || pattern::search(&root, &regex, show_hidden)).await
        else {
            return;
        };
        if let Some(state) = weak.upgrade()
            && state.generation.get() == generation
        {
            set_late_rows(&state, found);
        }
    });
    "No matching files or folders"
}

fn query_index(state: &Rc<PaletteState>, query: &str) {
    if state.index.borrow().is_none() {
        let (roots, exclusions) = (state.sources.index_scope)();
        if roots.is_empty() {
            return;
        }
        let (handle, receiver) =
            index_trees_with_exclusions(roots, state.show_hidden.get(), exclusions);
        let weak = Rc::downgrade(state);
        let poll =
            glib::timeout_add_local(Duration::from_millis(16), move || match weak.upgrade() {
                Some(state) => poll_index(&state, &receiver),
                None => glib::ControlFlow::Break,
            });
        state.index.replace(Some(IndexSearch { handle, poll }));
    }
    if let Some(index) = state.index.borrow().as_ref() {
        index.handle.query(query.trim());
    }
}

fn poll_index(state: &PaletteState, receiver: &Receiver<SearchEvent>) -> glib::ControlFlow {
    let mut latest = None;
    for _ in 0..8 {
        match receiver.try_recv() {
            Ok(SearchEvent::Results { query, items, .. }) => latest = Some((query, items)),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                // Returning `Break` removes the source, so it must not be removed again.
                state.index.take();
                return glib::ControlFlow::Break;
            }
        }
    }
    let current = state.field.text();
    if let Some((query, items)) = latest
        && Mode::of(&current) == Mode::Commands
        && !query.is_empty()
        && query == current.trim()
    {
        let entries = items
            .into_iter()
            .take(MAX_FILES)
            .map(|item| paths::PathEntry {
                name: item.name,
                path: item.path,
                is_dir: item.is_directory,
            })
            .collect();
        set_late_rows(state, entries);
    }
    glib::ControlFlow::Continue
}

fn stop_index(state: &PaletteState) {
    if let Some(index) = state.index.take() {
        index.poll.remove();
    }
}

/// Replaces the rows background searches added, keeping a selection among them.
fn set_late_rows(state: &PaletteState, entries: Vec<paths::PathEntry>) {
    let base = state.base_len.get();
    let selected = state.list.selected_row().map(|row| row.index());
    while let Some(row) = state.list.row_at_index(base as i32) {
        state.list.remove(&row);
    }
    {
        let mut visible = state.visible.borrow_mut();
        visible.truncate(base);
        let known = visible
            .iter()
            .filter_map(|row| match row {
                PaletteRow::Entry(entry) => Some(entry.path.clone()),
                _ => None,
            })
            .collect::<HashSet<_>>();
        for entry in entries
            .into_iter()
            .filter(|entry| !known.contains(&entry.path))
        {
            state.list.append(&entry_row(&entry));
            visible.push(PaletteRow::Entry(entry));
        }
    }
    if state.list.selected_row().is_none()
        && let Some(index) = selected
    {
        let last = state.visible.borrow().len() as i32 - 1;
        if let Some(row) = state.list.row_at_index(index.min(last)) {
            state.list.select_row(Some(&row));
        }
    }
    show_rows(state);
}

fn entry_row(entry: &paths::PathEntry) -> gtk::ListBoxRow {
    let icon = if entry.is_dir {
        crate::assets::icons::FOLDER
    } else {
        super::browser::icon_for_name(&entry.name)
    };
    let parent = entry
        .path
        .parent()
        .map(|parent| super::browser::compact_hint(parent, &glib::home_dir()))
        .unwrap_or_default();
    path_row(&entry.name, &entry.path, icon, &parent)
}

fn path_row(label: &str, path: &Path, icon: &str, hint: &str) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let slot = super::thumbnail::ThumbnailSlot::new(19);
    slot.add_css_class("search-result-thumbnail");
    super::thumbnail::show_customized_icon(&slot, path, icon, 19);
    content.append(&slot);
    let name = gtk::Label::new(Some(label));
    name.add_css_class("search-result-name");
    name.set_hexpand(true);
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    content.append(&name);
    if !hint.is_empty() {
        let hint = gtk::Label::new(Some(hint));
        hint.add_css_class("search-result-path");
        hint.set_halign(gtk::Align::End);
        hint.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        hint.set_max_width_chars(28);
        content.append(&hint);
    }
    let row = gtk::ListBoxRow::new();
    row.add_css_class("search-result");
    crate::ui::accessibility::set_label(&row, &path.to_string_lossy());
    row.set_child(Some(&content));
    row
}

fn command_row(label: &str, hint: &str) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let name = gtk::Label::new(Some(label));
    name.add_css_class("search-result-name");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&name);
    if !hint.is_empty() {
        let hint = gtk::Label::new(Some(hint));
        hint.add_css_class("search-result-path");
        hint.set_halign(gtk::Align::End);
        content.append(&hint);
    }
    let row = gtk::ListBoxRow::new();
    row.add_css_class("search-result");
    row.set_child(Some(&content));
    row
}

fn move_selection(state: &PaletteState, direction: i32) {
    let count = state.visible.borrow().len() as i32;
    if count == 0 {
        return;
    }
    let next = state
        .list
        .selected_row()
        .map_or(0, |row| (row.index() + direction).clamp(0, count - 1));
    if let Some(row) = state.list.row_at_index(next) {
        state.list.select_row(Some(&row));
        scroll_row_into_view(state, &row);
    }
}

fn scroll_row_into_view(state: &PaletteState, row: &gtk::ListBoxRow) {
    let Some(bounds) = row.compute_bounds(&state.list) else {
        return;
    };
    let adjustment = state.scroller.vadjustment();
    let top = f64::from(bounds.y());
    let bottom = top + f64::from(bounds.height());
    if top < adjustment.value() {
        adjustment.set_value(top);
    } else if bottom > adjustment.value() + adjustment.page_size() {
        adjustment.set_value(bottom - adjustment.page_size());
    }
}

fn activate_selected(state: &Rc<PaletteState>) -> bool {
    let Some(row) = state.list.selected_row() else {
        return false;
    };
    activate_position(state, row.index());
    true
}

fn row_at(state: &PaletteState, position: i32) -> Option<PaletteRow> {
    let position = usize::try_from(position).ok()?;
    state.visible.borrow().get(position).cloned()
}

fn activate_position(state: &Rc<PaletteState>, position: i32) {
    let Some(row) = row_at(state, position) else {
        return;
    };
    let open = state.open_path.clone();
    match row {
        PaletteRow::Command(command) => {
            let run = state.run.clone();
            hide_then(state, move || run(command.action));
        }
        PaletteRow::Folder(path) => hide_then(state, move || open(path, true)),
        PaletteRow::Entry(entry) => hide_then(state, move || open(entry.path, entry.is_dir)),
    }
}

/// Puts the selected file or folder's path in the field; a folder then lists its
/// contents.
fn complete_selected(state: &PaletteState) -> bool {
    let Some(PaletteRow::Entry(entry)) = state
        .list
        .selected_row()
        .and_then(|row| row_at(state, row.index()))
    else {
        return false;
    };
    let mut text = super::browser::compact_hint(&entry.path, &glib::home_dir());
    if entry.is_dir && !text.ends_with('/') {
        text.push('/');
    }
    state.field.set_text(&text);
    state.field.set_position(-1);
    true
}

fn hide(state: &PaletteState) {
    hide_then(state, || {});
}

fn hide_then(state: &PaletteState, after_dismiss: impl FnOnce() + 'static) {
    if state.layer.has_css_class("dismissing") {
        return;
    }
    state.generation.set(state.generation.get().wrapping_add(1));
    stop_index(state);
    state.layer.add_css_class("dismissing");
    state.layer.set_sensitive(false);
    let layer = state.layer.clone();
    let dismiss = state.dismiss.clone();
    super::browser::animate_out(&state.layer, move || {
        layer.set_visible(false);
        layer.remove_css_class("dismissing");
        layer.set_sensitive(true);
        dismiss();
        after_dismiss();
    });
}

#[cfg(test)]
mod tests;
