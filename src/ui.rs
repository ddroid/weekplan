use gtk4 as gtk;
use adw::prelude::*;
use gtk::glib;
use gtk::{
    gdk, Align, Box as GtkBox, Button, ListBox, Orientation,
    ScrolledWindow, EventControllerKey, EventControllerScroll,
    EventControllerScrollFlags, SelectionMode, PolicyType,
};

use std::cell::RefCell;
use std::rc::Rc;

use crate::data::{self, DAY_NAMES, Task, WeekData};

// ─── Public Entry Points ────────────────────────────────────────────────────

pub fn load_css() {
    // No custom CSS needed — we rely entirely on Adwaita's built-in classes
    // (boxed-list, suggested-action, destructive-action, etc.)
}

pub fn build_ui(app: &adw::Application) {
    let week_data = Rc::new(RefCell::new(WeekData::load()));

    // ── ViewStack with 7 day pages ──────────────────────────────────────
    let view_stack = adw::ViewStack::new();
    view_stack.set_vexpand(true);

    // Store per-day list boxes so we can refresh them
    let day_list_boxes: Rc<RefCell<Vec<ListBox>>> = Rc::new(RefCell::new(Vec::new()));
    // Store per-day stack containers (GtkBox wrapping either list or status page)
    let day_containers: Rc<RefCell<Vec<GtkBox>>> = Rc::new(RefCell::new(Vec::new()));

    for day_idx in 0..7usize {
        let is_today = day_idx == data::today_index();
        let day_name = DAY_NAMES[day_idx];

        // Each page: ScrolledWindow → Clamp → vertical box
        let page_box = GtkBox::new(Orientation::Vertical, 0);
        page_box.set_vexpand(true);

        let content_box = GtkBox::new(Orientation::Vertical, 12);
        content_box.set_margin_top(24);
        content_box.set_margin_bottom(24);
        content_box.set_margin_start(12);
        content_box.set_margin_end(12);

        // Day subtitle label
        let day_label_text = format!("{}", day_name);
        let day_subtitle = gtk::Label::new(Some(&day_label_text));
        day_subtitle.add_css_class("title-4");
        day_subtitle.set_halign(Align::Start);
        day_subtitle.set_margin_start(4);
        day_subtitle.set_margin_bottom(4);

        // Task ListBox with boxed-list style
        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::None);
        list_box.add_css_class("boxed-list");

        content_box.append(&day_subtitle);
        content_box.append(&list_box);

        // Clamp to keep content well-sized on wide screens
        let clamp = adw::Clamp::builder()
            .maximum_size(600)
            .tightening_threshold(400)
            .child(&content_box)
            .build();

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .child(&clamp)
            .build();

        page_box.append(&scrolled);

        // Populate tasks or show empty state
        {
            let wd = week_data.borrow();
            if wd.days[day_idx].is_empty() {
                show_empty_state(&content_box, &list_box, day_name);
            } else {
                for task in &wd.days[day_idx] {
                    let row = build_task_row(task, day_idx, &week_data, &list_box, &content_box);
                    list_box.append(&row);
                }
            }
        }

        // Determine icon for the day tab
        let icon = if is_today {
            "starred-symbolic"
        } else if day_idx < 5 {
            "office-calendar-symbolic"
        } else {
            "weather-clear-symbolic"
        };

        // Short name for the tab (3 letters)
        let short_name = &day_name[..3];
        view_stack.add_titled_with_icon(&page_box, Some(day_name), short_name, icon);

        day_list_boxes.borrow_mut().push(list_box);
        day_containers.borrow_mut().push(content_box);
    }

    // Set today's page as visible
    view_stack.set_visible_child_name(DAY_NAMES[data::today_index()]);

    // ── Header bar with InlineViewSwitcher ─────────────────────────────
    let view_switcher = adw::InlineViewSwitcher::builder()
        .stack(&view_stack)
        .can_shrink(true)
        .build();

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&view_switcher));

    // ── Add Task button (header end) ────────────────────────────────────
    let add_button = Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Task")
        .build();
    add_button.add_css_class("flat");
    header.pack_end(&add_button);

    // Connect add button
    {
        let view_stack_ref = view_stack.clone();
        let week_data = week_data.clone();
        let day_list_boxes = day_list_boxes.clone();
        let day_containers = day_containers.clone();

        add_button.connect_clicked(move |btn| {
            // Determine which day is currently visible
            let active_name = view_stack_ref
                .visible_child_name()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let day_idx = DAY_NAMES
                .iter()
                .position(|&n| n == active_name)
                .unwrap_or(0);

            show_add_task_dialog(
                btn,
                day_idx,
                &week_data,
                &day_list_boxes,
                &day_containers,
            );
        });
    }

    // ── ToolbarView assembly ────────────────────────────────────────────
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&view_stack));

    // ── Keyboard navigation (← →) ──────────────────────────────────────
    let key_ctrl = EventControllerKey::new();
    {
        let view_stack = view_stack.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            let current_name = view_stack
                .visible_child_name()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let current_idx = DAY_NAMES
                .iter()
                .position(|&n| n == current_name)
                .unwrap_or(0);

            let new_idx = match key {
                gdk::Key::Left if current_idx > 0 => current_idx - 1,
                gdk::Key::Right if current_idx < 6 => current_idx + 1,
                _ => return glib::Propagation::Proceed,
            };

            view_stack.set_visible_child_name(DAY_NAMES[new_idx]);
            glib::Propagation::Stop
        });
    }
    toolbar_view.add_controller(key_ctrl);

    // ── Scroll navigation ───────────────────────────────────────────────
    let scroll_ctrl = EventControllerScroll::new(
        EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::HORIZONTAL,
    );
    {
        let view_stack = view_stack.clone();
        // Accumulate fractional scroll deltas before switching
        let scroll_accum: Rc<RefCell<f64>> = Rc::new(RefCell::new(0.0));

        scroll_ctrl.connect_scroll(move |_, dx, dy| {
            let delta = if dy.abs() > dx.abs() { dy } else { dx };
            let mut accum = scroll_accum.borrow_mut();
            *accum += delta;

            let threshold = 2.0;
            if accum.abs() < threshold {
                return glib::Propagation::Stop;
            }

            let direction = *accum;
            *accum = 0.0;
            drop(accum);

            let current_name = view_stack
                .visible_child_name()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let current_idx = DAY_NAMES
                .iter()
                .position(|&n| n == current_name)
                .unwrap_or(0);

            let new_idx = if direction > 0.0 && current_idx < 6 {
                current_idx + 1
            } else if direction < 0.0 && current_idx > 0 {
                current_idx - 1
            } else {
                return glib::Propagation::Stop;
            };

            view_stack.set_visible_child_name(DAY_NAMES[new_idx]);
            glib::Propagation::Stop
        });
    }
    toolbar_view.add_controller(scroll_ctrl);

    // ── Window ──────────────────────────────────────────────────────────
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Week Plan")
        .default_width(800)
        .default_height(600)
        .content(&toolbar_view)
        .build();

    window.present();
}

// ─── Task Row (AdwActionRow) ────────────────────────────────────────────────

fn build_task_row(
    task: &Task,
    day_idx: usize,
    week_data: &Rc<RefCell<WeekData>>,
    list_box: &ListBox,
    content_box: &GtkBox,
) -> adw::ActionRow {
    // Build subtitle: combine time and description
    let subtitle = match (task.time.is_empty(), task.description.is_empty()) {
        (false, false) => format!("🕐 {}  ·  {}", task.time, task.description),
        (false, true) => format!("🕐 {}", task.time),
        (true, false) => task.description.clone(),
        (true, true) => String::new(),
    };

    let row = adw::ActionRow::builder()
        .title(&task.title)
        .subtitle(&subtitle)
        .build();

    // Delete button as suffix
    let del_btn = Button::builder()
        .icon_name("edit-delete-symbolic")
        .valign(Align::Center)
        .tooltip_text("Remove task")
        .build();
    del_btn.add_css_class("flat");
    del_btn.add_css_class("circular");

    let task_id = task.id;
    let week_data = week_data.clone();
    let list_box = list_box.clone();
    let content_box = content_box.clone();
    let row_clone = row.clone();

    del_btn.connect_clicked(move |_| {
        week_data.borrow_mut().remove_task(day_idx, task_id);
        list_box.remove(&row_clone);

        // If list is now empty, show empty state
        if list_box.first_child().is_none() {
            show_empty_state(&content_box, &list_box, DAY_NAMES[day_idx]);
        }
    });

    row.add_suffix(&del_btn);
    row
}

// ─── Empty State (AdwStatusPage) ────────────────────────────────────────────

fn show_empty_state(content_box: &GtkBox, list_box: &ListBox, day_name: &str) {
    // Hide the list box, show a status page instead
    list_box.set_visible(false);

    // Check if a status page already exists
    let mut child = content_box.first_child();
    while let Some(c) = child {
        if c.css_classes().iter().any(|cls| cls.as_str() == "empty-status-page") {
            c.set_visible(true);
            return;
        }
        child = c.next_sibling();
    }

    let status = adw::StatusPage::builder()
        .icon_name("calendar-symbolic")
        .title(&format!("No tasks for {}", day_name))
        .description("Press + to add a recurring task")
        .vexpand(true)
        .build();
    status.add_css_class("empty-status-page");
    content_box.append(&status);
}

fn hide_empty_state(content_box: &GtkBox, list_box: &ListBox) {
    list_box.set_visible(true);

    let mut child = content_box.first_child();
    while let Some(c) = child {
        if c.css_classes().iter().any(|cls| cls.as_str() == "empty-status-page") {
            c.set_visible(false);
        }
        child = c.next_sibling();
    }
}

// ─── Add Task Dialog (AdwDialog) ────────────────────────────────────────────

fn show_add_task_dialog(
    parent: &impl IsA<gtk::Widget>,
    day_idx: usize,
    week_data: &Rc<RefCell<WeekData>>,
    day_list_boxes: &Rc<RefCell<Vec<ListBox>>>,
    day_containers: &Rc<RefCell<Vec<GtkBox>>>,
) {
    let day_name = DAY_NAMES[day_idx];

    // ── Entry rows ──
    let title_row = adw::EntryRow::builder()
        .title("Task Title")
        .build();

    let time_row = adw::EntryRow::builder()
        .title("Time (optional)")
        .build();

    let notes_row = adw::EntryRow::builder()
        .title("Notes (optional)")
        .build();

    // ── Preferences group ──
    let pref_group = adw::PreferencesGroup::builder()
        .title(&format!("New task for {}", day_name))
        .description("This task will repeat every week")
        .build();
    pref_group.add(&title_row);
    pref_group.add(&time_row);
    pref_group.add(&notes_row);

    // ── Action buttons ──
    let cancel_btn = Button::builder()
        .label("Cancel")
        .hexpand(true)
        .build();

    let add_btn = Button::builder()
        .label("Add Task")
        .hexpand(true)
        .build();
    add_btn.add_css_class("suggested-action");

    let btn_box = GtkBox::new(Orientation::Horizontal, 12);
    btn_box.set_homogeneous(true);
    btn_box.set_margin_top(12);
    btn_box.append(&cancel_btn);
    btn_box.append(&add_btn);

    // ── Dialog content ──
    let dialog_content = GtkBox::new(Orientation::Vertical, 0);
    dialog_content.set_margin_start(24);
    dialog_content.set_margin_end(24);
    dialog_content.set_margin_top(24);
    dialog_content.set_margin_bottom(24);
    dialog_content.append(&pref_group);
    dialog_content.append(&btn_box);

    // ── Header bar for dialog ──
    let dialog_header = adw::HeaderBar::new();
    dialog_header.set_show_end_title_buttons(false);
    dialog_header.set_show_start_title_buttons(false);
    let dialog_title = adw::WindowTitle::new("Add Task", day_name);
    dialog_header.set_title_widget(Some(&dialog_title));

    let dialog_toolbar = adw::ToolbarView::new();
    dialog_toolbar.add_top_bar(&dialog_header);
    dialog_toolbar.set_content(Some(&dialog_content));

    // ── The dialog itself ──
    let dialog = adw::Dialog::builder()
        .title("Add Task")
        .content_width(420)
        .content_height(380)
        .child(&dialog_toolbar)
        .build();

    // ── Cancel ──
    {
        let dialog = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog.close();
        });
    }

    // ── Add ──
    let title_row_for_activate = title_row.clone();
    {
        let dialog = dialog.clone();
        let title_row = title_row.clone();
        let time_row = time_row.clone();
        let notes_row = notes_row.clone();
        let week_data = week_data.clone();
        let day_list_boxes = day_list_boxes.clone();
        let day_containers = day_containers.clone();

        let do_add: Rc<dyn Fn()> = Rc::new(move || {
            let title = title_row.text().to_string();
            if title.trim().is_empty() {
                title_row.grab_focus();
                return;
            }
            let time = time_row.text().to_string();
            let notes = notes_row.text().to_string();

            let task = week_data.borrow_mut().add_task(day_idx, title, time, notes);

            // Add to the correct list box
            let list_boxes = day_list_boxes.borrow();
            let containers = day_containers.borrow();
            let lb = &list_boxes[day_idx];
            let cb = &containers[day_idx];

            // Hide empty state, show list
            hide_empty_state(cb, lb);

            let row = build_task_row(&task, day_idx, &week_data, lb, cb);
            lb.append(&row);

            dialog.close();
        });

        add_btn.connect_clicked({
            let do_add = do_add.clone();
            move |_| do_add()
        });

        // Submit on Enter from title row
        let s = do_add.clone();
        title_row_for_activate.connect_apply(move |_| s());
    }

    dialog.present(Some(parent));
}
