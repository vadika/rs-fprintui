use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Button, Box, Orientation};
use libadwaita as adw;
use anyhow::Result;
use zbus::dbus_proxy;

const APP_ID: &str = "org.example.fprintui";

#[dbus_proxy(
    interface = "net.reactivated.Fprint.Device",
    default_service = "net.reactivated.Fprint",
    default_path = "/net/reactivated/Fprint/Device/0"
)]
trait FprintDevice {
    async fn enroll(&self) -> zbus::Result<()>;
    async fn verify(&self) -> zbus::Result<()>;
    async fn delete_enrolled_fingers(&self, finger: &str) -> zbus::Result<()>;
    async fn list_enrolled_fingers(&self) -> zbus::Result<Vec<String>>;
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Fingerprint Manager")
        .default_width(400)
        .default_height(300)
        .build();

    let main_box = Box::new(Orientation::Vertical, 10);
    main_box.set_margin_start(10);
    main_box.set_margin_end(10);
    main_box.set_margin_top(10);
    main_box.set_margin_bottom(10);

    let enroll_button = Button::with_label("Enroll New Fingerprint");
    let verify_button = Button::with_label("Verify Fingerprint");
    let list_button = Button::with_label("List Enrolled Fingerprints");
    let delete_button = Button::with_label("Delete Fingerprints");

    main_box.append(&enroll_button);
    main_box.append(&verify_button);
    main_box.append(&list_button);
    main_box.append(&delete_button);

    window.set_child(Some(&main_box));
    window.present();
}

#[tokio::main]
async fn main() -> Result<()> {
    adw::init()?;

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(build_ui);
    app.run();

    Ok(())
}
