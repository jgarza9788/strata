// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use super::keys::{
    self, Keys,
    bindings::{
        ARROW_SCOPE, COMMAND_PALETTE, DOWN, ENTER_ANY_SHIFT, ESCAPE_ANY, JUMP_TO_FOLDER, REFRESH,
        SEARCH, TAB_CLOSE, TAB_NEW, TERMINAL, UP,
    },
};

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
pub struct CommandPalette {
    state: Rc<PaletteState>,
}

struct PaletteState {
    layer: gtk::Box,
    field: gtk::Entry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    results: gtk::Stack,
    visible: RefCell<Vec<&'static Command>>,
    run: Rc<dyn Fn(&'static str)>,
    dismiss: Rc<dyn Fn()>,
}

impl CommandPalette {
    /// `run` receives the `win.*` action name once the palette has closed.
    pub fn new(run: Rc<dyn Fn(&'static str)>, dismiss: Rc<dyn Fn()>) -> Self {
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
            .placeholder_text(crate::i18n::tr("Search commands…"))
            .hexpand(true)
            .build();
        field.add_css_class("search-field");
        crate::ui::accessibility::set_description(
            &field,
            Some(&crate::i18n::tr("Command palette")),
        );
        search_bar.append(&field);
        panel.append(&search_bar);

        let status = gtk::Label::new(Some(&crate::i18n::tr("No matching commands")));
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
        let open = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        open.set_valign(gtk::Align::Center);
        open.append(&crate::assets::primary_icon(
            crate::assets::icons::CORNER_DOWN_LEFT,
            13,
        ));
        open.append(&gtk::Label::new(Some(&crate::i18n::tr("run"))));
        open.add_css_class("search-hint");
        footer.append(&navigation);
        footer.append(&open);
        panel.append(&footer);
        super::modal::layout::install(&layer, &panel);

        let state = Rc::new(PaletteState {
            layer,
            field,
            list,
            scroller,
            results,
            visible: RefCell::new(Vec::new()),
            run,
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

    pub fn show(&self) {
        self.state.field.set_text("");
        render(&self.state, "");
        self.state.layer.set_visible(true);
        super::browser::animate_in(&self.state.layer);
        self.state.field.grab_focus_without_selecting();
    }

    pub fn hide(&self) {
        hide(&self.state);
    }

    pub fn is_visible(&self) -> bool {
        self.state.layer.is_visible()
    }
}

fn render(state: &PaletteState, query: &str) {
    let tenxer = super::tenxer_mode::chrome_suppressed();
    let commands = filter(query, crate::i18n::tr);
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }
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
    state.scroller.vadjustment().set_value(0.0);
    state
        .results
        .set_visible_child_name(if commands.is_empty() {
            "status"
        } else {
            "results"
        });
    if let Some(row) = state.list.row_at_index(0) {
        state.list.select_row(Some(&row));
    }
    state.visible.replace(commands);
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

fn activate_position(state: &Rc<PaletteState>, position: i32) {
    let Some(command) = usize::try_from(position)
        .ok()
        .and_then(|position| state.visible.borrow().get(position).copied())
    else {
        return;
    };
    let run = state.run.clone();
    hide_then(state, move || run(command.action));
}

fn hide(state: &PaletteState) {
    hide_then(state, || {});
}

fn hide_then(state: &PaletteState, after_dismiss: impl FnOnce() + 'static) {
    if state.layer.has_css_class("dismissing") {
        return;
    }
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
