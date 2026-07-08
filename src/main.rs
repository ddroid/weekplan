mod data;
mod ui;

use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;

const APP_ID: &str = "org.weekplan.WeekPlan";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_startup(|_| ui::load_css());
    app.connect_activate(ui::build_ui);

    app.run()
}
