use adw::prelude::*;
use gtk4 as gtk;
use gtk::{gdk, gio, glib, Align, Orientation};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chrono::{Datelike, NaiveDate};

use crate::data::{self, DAY_NAMES, Draft, Repeat, Task, WeekData};

/// Widest the seven-column week grows before it stops stretching.
const WEEK_MAX_WIDTH: i32 = 1400;
/// Below this the window shows one day with a day switcher.
const NARROW: &str = "max-width: 720sp";

const CSS: &str = ".task-done .task-title { text-decoration-line: line-through; }";

pub fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[derive(Clone, Copy)]
enum Target {
    New(usize),
    Edit(u64),
}

struct Column {
    root: gtk::Box,
    label: gtk::Label,
    add: gtk::Button,
    list: gtk::ListBox,
    empty: gtk::Label,
}

struct Ui {
    data: RefCell<WeekData>,
    columns: Vec<Column>,
    clamp: adw::Clamp,
    header: adw::HeaderBar,
    week_title: adw::WindowTitle,
    switcher: adw::ToggleGroup,
    toasts: adw::ToastOverlay,
    window: adw::ApplicationWindow,
    narrow: Cell<bool>,
    selected: Cell<usize>,
    rendered_on: Cell<NaiveDate>,
}

pub fn build_ui(app: &adw::Application) {
    let today = data::today_index();

    let week_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(12)
        .margin_end(12)
        .build();
    let columns: Vec<Column> = (0..7).map(|_| build_column()).collect();
    for column in &columns {
        week_box.append(&column.root);
    }

    let clamp = adw::Clamp::builder()
        .maximum_size(WEEK_MAX_WIDTH)
        .tightening_threshold(WEEK_MAX_WIDTH)
        .child(&week_box)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scrolled));

    let switcher = adw::ToggleGroup::new();
    for (day, name) in DAY_NAMES.iter().enumerate() {
        switcher.add(adw::Toggle::builder().label(&name[..3]).name(day.to_string()).build());
    }
    switcher.set_active_name(Some(&today.to_string()));

    let week_title = adw::WindowTitle::new("Week Plan", "");
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&week_title));
    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Task (Ctrl+N)")
        .action_name("win.add-task")
        .build();
    header.pack_end(&add_button);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Week Plan")
        .default_width(1100)
        .default_height(640)
        .width_request(360)
        .content(&toolbar)
        .build();

    let ui = Rc::new(Ui {
        data: RefCell::new(WeekData::load()),
        columns,
        clamp,
        header,
        week_title,
        switcher,
        toasts,
        window: window.clone(),
        narrow: Cell::new(false),
        selected: Cell::new(today),
        rendered_on: Cell::new(data::today()),
    });

    for (day, column) in ui.columns.iter().enumerate() {
        {
            let ui = ui.clone();
            column.add.connect_clicked(move |_| ui.show_dialog(Target::New(day)));
        }
        // Rows carry their task id as the widget name.
        let ui = ui.clone();
        column.list.connect_row_activated(move |_, row| {
            if let Ok(id) = row.widget_name().parse() {
                ui.show_dialog(Target::Edit(id));
            }
        });
    }
    ui.refresh_all();

    let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::parse(NARROW).unwrap());
    {
        let ui = ui.clone();
        breakpoint.connect_apply(move |_| ui.set_narrow(true));
    }
    {
        let ui = ui.clone();
        breakpoint.connect_unapply(move |_| ui.set_narrow(false));
    }
    window.add_breakpoint(breakpoint);

    {
        let ui2 = ui.clone();
        ui.switcher.connect_active_name_notify(move |group| {
            if let Some(day) = group.active_name().and_then(|n| n.parse().ok()) {
                ui2.selected.set(day);
                ui2.update_columns();
            }
        });
    }

    let add_action = gio::SimpleAction::new("add-task", None);
    {
        let ui = ui.clone();
        add_action.connect_activate(move |_, _| {
            let day = if ui.narrow.get() { ui.selected.get() } else { data::today_index() };
            ui.show_dialog(Target::New(day));
        });
    }
    window.add_action(&add_action);
    app.set_accels_for_action("win.add-task", &["<Control>n"]);

    let keys = gtk::EventControllerKey::new();
    {
        let ui = ui.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if !ui.narrow.get() || !mods.is_empty() {
                return glib::Propagation::Proceed;
            }
            let day = ui.selected.get();
            let next = match key {
                gdk::Key::Left if day > 0 => day - 1,
                gdk::Key::Right if day < 6 => day + 1,
                _ => return glib::Propagation::Proceed,
            };
            ui.switcher.set_active_name(Some(&next.to_string()));
            glib::Propagation::Stop
        });
    }
    toolbar.add_controller(keys);

    // Weekly check marks and the today highlight go stale if the app stays open overnight.
    {
        let ui = ui.clone();
        window.connect_is_active_notify(move |w| {
            if w.is_active() && ui.rendered_on.get() != data::today() {
                ui.refresh_all();
            }
        });
    }

    window.present();
}

fn build_column() -> Column {
    let label = gtk::Label::builder().xalign(0.0).hexpand(true).build();
    label.add_css_class("heading");
    let add = gtk::Button::builder().icon_name("list-add-symbolic").build();
    add.add_css_class("flat");
    add.add_css_class("circular");

    let head = gtk::Box::new(Orientation::Horizontal, 6);
    head.set_margin_start(6);
    head.append(&label);
    head.append(&add);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .valign(Align::Start)
        .build();
    list.add_css_class("boxed-list");

    let empty = gtk::Label::builder().label("Nothing planned").xalign(0.0).margin_start(6).build();
    empty.add_css_class("dim-label");

    let root = gtk::Box::new(Orientation::Vertical, 8);
    root.append(&head);
    root.append(&list);
    root.append(&empty);

    Column { root, label, add, list, empty }
}

impl Ui {
    fn refresh_all(self: &Rc<Self>) {
        self.rendered_on.set(data::today());
        let (first, last) = (data::date_of(0), data::date_of(6));
        let range = if first.month() == last.month() {
            format!("{}–{}", first.format("%-d"), last.format("%-d %B"))
        } else {
            format!("{}–{}", first.format("%-d %b"), last.format("%-d %b"))
        };
        self.week_title.set_subtitle(&range);
        for day in 0..7 {
            self.refresh(day);
        }
        self.update_columns();
    }

    fn refresh(self: &Rc<Self>, day: usize) {
        let column = &self.columns[day];
        while let Some(row) = column.list.row_at_index(0) {
            column.list.remove(&row);
        }
        let tasks = self.data.borrow().sorted_day(day);
        for task in &tasks {
            column.list.append(&self.task_row(task));
        }
        column.list.set_visible(!tasks.is_empty());
        column.empty.set_visible(tasks.is_empty());
    }

    fn set_narrow(self: &Rc<Self>, narrow: bool) {
        self.narrow.set(narrow);
        if narrow {
            self.header.set_title_widget(Some(&self.switcher));
            self.clamp.set_maximum_size(600);
        } else {
            self.header.set_title_widget(Some(&self.week_title));
            self.clamp.set_maximum_size(WEEK_MAX_WIDTH);
        }
        self.update_columns();
    }

    fn update_columns(&self) {
        let narrow = self.narrow.get();
        let today = data::today_index();
        for (day, column) in self.columns.iter().enumerate() {
            let date = data::date_of(day);
            let text = if narrow {
                format!("{} {}", DAY_NAMES[day], date.format("%-d %B"))
            } else {
                format!("{} {}", &DAY_NAMES[day][..3], date.format("%-d"))
            };
            column.label.set_label(&text);
            if day == today {
                column.label.add_css_class("accent");
            } else {
                column.label.remove_css_class("accent");
            }
            column.add.set_tooltip_text(Some(&format!("Add to {}", DAY_NAMES[day])));
            column.root.set_visible(!narrow || day == self.selected.get());
        }
    }

    fn task_row(self: &Rc<Self>, task: &Task) -> gtk::ListBoxRow {
        let done = task.is_done_this_week();
        let check = gtk::CheckButton::builder()
            .active(done)
            .valign(Align::Start)
            .tooltip_text(match task.repeat {
                Repeat::Weekly => "Done for this week",
                Repeat::Once => "Done",
            })
            .build();

        let title = gtk::Label::builder()
            .label(&task.title)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("task-title");

        let text = gtk::Box::new(Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(Align::Center);
        text.append(&title);

        let when = if task.all_day { "All day" } else { task.time.as_str() };
        let meta = [when, task.description.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        if !meta.is_empty() || task.repeat == Repeat::Weekly {
            let line = gtk::Box::new(Orientation::Horizontal, 4);
            line.add_css_class("dim-label");
            if task.repeat == Repeat::Weekly {
                let icon = gtk::Image::from_icon_name("media-playlist-repeat-symbolic");
                icon.set_pixel_size(12);
                icon.set_valign(Align::Start);
                icon.set_margin_top(2);
                icon.set_tooltip_text(Some("Every week"));
                line.append(&icon);
            }
            if !meta.is_empty() {
                let label = gtk::Label::builder()
                    .label(&meta)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .lines(2)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build();
                label.add_css_class("caption");
                line.append(&label);
            }
            text.append(&line);
        }

        let content = gtk::Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(10)
            .margin_end(10)
            .build();
        content.append(&check);
        content.append(&text);

        let id = task.id;
        let row = gtk::ListBoxRow::builder().child(&content).name(id.to_string()).build();
        if done {
            row.add_css_class("task-done");
            row.add_css_class("dim-label");
        }

        {
            let ui = self.clone();
            check.connect_toggled(move |c| ui.set_done(id, c.is_active()));
        }

        let menu_click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
        {
            let ui = self.clone();
            let row = row.clone();
            menu_click.connect_pressed(move |_, _, x, y| ui.show_menu(&row, id, x, y));
        }
        row.add_controller(menu_click);

        row
    }

    fn show_menu(self: &Rc<Self>, row: &gtk::ListBoxRow, id: u64, x: f64, y: f64) {
        let Some((day, _)) = self.data.borrow().find(id) else {
            return;
        };
        let list = &self.columns[day].list;

        let actions = gio::SimpleActionGroup::new();
        let edit = gio::SimpleAction::new("edit", None);
        {
            let ui = self.clone();
            edit.connect_activate(move |_, _| ui.show_dialog(Target::Edit(id)));
        }
        let delete = gio::SimpleAction::new("delete", None);
        {
            let ui = self.clone();
            delete.connect_activate(move |_, _| ui.delete(id));
        }
        let move_to = gio::SimpleAction::new("move", Some(glib::VariantTy::UINT32));
        {
            let ui = self.clone();
            move_to.connect_activate(move |_, param| {
                if let Some(to) = param.and_then(|p| p.get::<u32>()) {
                    ui.move_task(id, to as usize);
                }
            });
        }
        actions.add_action(&edit);
        actions.add_action(&delete);
        actions.add_action(&move_to);

        let days = gio::Menu::new();
        for (to, name) in DAY_NAMES.iter().enumerate().filter(|(to, _)| *to != day) {
            let item = gio::MenuItem::new(Some(name), None);
            item.set_action_and_target_value(Some("task.move"), Some(&(to as u32).to_variant()));
            days.append_item(&item);
        }
        let menu = gio::Menu::new();
        menu.append(Some("Edit"), Some("task.edit"));
        menu.append_submenu(Some("Move to"), &days);
        let danger = gio::Menu::new();
        danger.append(Some("Delete"), Some("task.delete"));
        menu.append_section(None, &danger);

        // Parented to the list, not the row, because moving or deleting rebuilds the rows.
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("task", Some(&actions));
        popover.set_parent(list);
        popover.set_has_arrow(false);
        let (px, py) = row.translate_coordinates(list, x, y).unwrap_or((0.0, 0.0));
        popover.set_pointing_to(Some(&gdk::Rectangle::new(px as i32, py as i32, 1, 1)));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }

    fn set_done(self: &Rc<Self>, id: u64, done: bool) {
        let Some((day, repeat)) = self.data.borrow().find(id).map(|(d, t)| (d, t.repeat)) else {
            return;
        };
        match repeat {
            Repeat::Weekly => {
                self.data.borrow_mut().set_done(id, done);
                self.refresh(day);
            }
            Repeat::Once if done => self.remove_with_undo(id, "Done"),
            Repeat::Once => {}
        }
    }

    fn delete(self: &Rc<Self>, id: u64) {
        self.remove_with_undo(id, "Deleted");
    }

    fn remove_with_undo(self: &Rc<Self>, id: u64, verb: &str) {
        let Some((day, idx, task)) = self.data.borrow_mut().remove(id) else {
            return;
        };
        self.refresh(day);
        let toast = adw::Toast::builder()
            .title(format!("{verb}: {}", task.title))
            .use_markup(false)
            .button_label("Undo")
            .timeout(5)
            .build();
        let task = RefCell::new(Some(task));
        let ui = self.clone();
        toast.connect_button_clicked(move |_| {
            if let Some(task) = task.take() {
                ui.data.borrow_mut().restore(day, idx, task);
                ui.refresh(day);
            }
        });
        self.toasts.add_toast(toast);
    }

    fn move_task(self: &Rc<Self>, id: u64, to: usize) {
        let Some(from) = self.data.borrow_mut().move_to(id, to) else {
            return;
        };
        self.refresh(from);
        self.refresh(to);
        if self.narrow.get() {
            self.toasts.add_toast(adw::Toast::new(&format!("Moved to {}", DAY_NAMES[to])));
        }
    }

    fn show_dialog(self: &Rc<Self>, target: Target) {
        let (day, task) = match target {
            Target::New(day) => (day, None),
            Target::Edit(id) => match self.data.borrow().find(id) {
                Some((day, task)) => (day, Some(task.clone())),
                None => return,
            },
        };
        let editing = task.is_some();

        let title = adw::EntryRow::builder().title("Title").build();
        let notes = adw::EntryRow::builder().title("Notes").build();
        let day_row = adw::ComboRow::builder()
            .title("Day")
            .model(&gtk::StringList::new(&DAY_NAMES))
            .selected(day as u32)
            .build();
        let weekly = adw::SwitchRow::builder().title("Repeat every week").build();
        let all_day = adw::SwitchRow::builder().title("All day").build();
        let time = adw::EntryRow::builder().title("Time, like 14:30 or 2pm").build();
        if let Some(task) = &task {
            title.set_text(&task.title);
            notes.set_text(&task.description);
            weekly.set_active(task.repeat == Repeat::Weekly);
            all_day.set_active(task.all_day);
            time.set_text(&task.time);
        }
        time.set_visible(!all_day.is_active());

        let details = adw::PreferencesGroup::new();
        details.add(&title);
        details.add(&notes);
        let when = adw::PreferencesGroup::new();
        when.add(&day_row);
        when.add(&weekly);
        when.add(&all_day);
        when.add(&time);

        let content = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        content.append(&details);
        content.append(&when);

        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::with_label(if editing { "Save" } else { "Add" });
        save.add_css_class("suggested-action");
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content));

        let dialog = adw::Dialog::builder()
            .title(if editing { "Edit Task" } else { "New Task" })
            .content_width(420)
            .child(&toolbar)
            .build();

        let repeat_hint = {
            let (weekly, day_row) = (weekly.clone(), day_row.clone());
            move || {
                let day = DAY_NAMES[day_row.selected() as usize];
                weekly.set_subtitle(&if weekly.is_active() {
                    format!("Every {day}")
                } else {
                    format!("This {day} only")
                });
            }
        };
        repeat_hint();
        {
            let hint = repeat_hint.clone();
            weekly.connect_active_notify(move |_| hint());
        }
        day_row.connect_selected_notify(move |_| repeat_hint());
        {
            let time = time.clone();
            all_day.connect_active_notify(move |s| time.set_visible(!s.is_active()));
        }
        time.connect_changed(|row| {
            let text = row.text();
            if text.trim().is_empty() || data::parse_time(&text).is_some() {
                row.remove_css_class("error");
            } else {
                row.add_css_class("error");
            }
        });

        let submit: Rc<dyn Fn()> = {
            let ui = self.clone();
            let dialog = dialog.clone();
            let (title, notes, day_row, weekly, all_day, time) =
                (title.clone(), notes.clone(), day_row.clone(), weekly.clone(), all_day.clone(), time.clone());
            Rc::new(move || {
                let name = title.text().trim().to_string();
                if name.is_empty() {
                    title.add_css_class("error");
                    title.grab_focus();
                    return;
                }
                let clock = if all_day.is_active() {
                    String::new()
                } else {
                    let text = time.text();
                    if text.trim().is_empty() {
                        String::new()
                    } else if let Some((h, m)) = data::parse_time(&text) {
                        format!("{h:02}:{m:02}")
                    } else {
                        time.grab_focus();
                        return;
                    }
                };
                let draft = Draft {
                    title: name,
                    time: clock,
                    description: notes.text().trim().to_string(),
                    all_day: all_day.is_active(),
                    repeat: if weekly.is_active() { Repeat::Weekly } else { Repeat::Once },
                };
                let to = day_row.selected() as usize;
                match target {
                    Target::New(_) => {
                        ui.data.borrow_mut().add(to, draft);
                        ui.refresh(to);
                    }
                    Target::Edit(id) => {
                        if let Some(from) = ui.data.borrow_mut().update(id, to, draft) {
                            ui.refresh(from);
                        }
                        ui.refresh(to);
                    }
                }
                if ui.narrow.get() && to != ui.selected.get() {
                    ui.switcher.set_active_name(Some(&to.to_string()));
                }
                dialog.close();
            })
        };

        {
            let submit = submit.clone();
            save.connect_clicked(move |_| submit());
        }
        for row in [&title, &notes, &time] {
            let submit = submit.clone();
            row.connect_entry_activated(move |_| submit());
        }
        title.connect_changed(|row| row.remove_css_class("error"));
        {
            let dialog = dialog.clone();
            cancel.connect_clicked(move |_| {
                dialog.close();
            });
        }

        if let Target::Edit(id) = target {
            let delete = adw::ButtonRow::builder().title("Delete Task").build();
            delete.add_css_class("destructive-action");
            let group = adw::PreferencesGroup::new();
            group.add(&delete);
            content.append(&group);
            let ui = self.clone();
            let dialog = dialog.clone();
            delete.connect_activated(move |_| {
                dialog.close();
                ui.delete(id);
            });
        }

        dialog.present(Some(&self.window));
        title.grab_focus();
    }
}
