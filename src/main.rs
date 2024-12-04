use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Button, Box, Orientation, ComboBoxText, Label};
use gtk4::glib;
use libadwaita as adw;
use anyhow::Result;
use zbus::{dbus_proxy, Connection};

const APP_ID: &str = "org.example.fprintui";

#[dbus_proxy(
    interface = "net.reactivated.Fprint.Device",
    default_service = "net.reactivated.Fprint",
    default_path = "/net/reactivated/Fprint/Device/0"
)]
trait FprintDevice {
    async fn enroll(&self, finger_name: &str) -> zbus::Result<()>;
    async fn verify(&self) -> zbus::Result<()>;
    async fn delete_enrolled_fingers(&self, finger: &str) -> zbus::Result<()>;
    async fn list_enrolled_fingers(&self) -> zbus::Result<Vec<String>>;
}

fn create_finger_selector() -> ComboBoxText {
    let combo = ComboBoxText::new();
    let fingers = [
        "left-thumb", "left-index-finger", "left-middle-finger", "left-ring-finger", "left-little-finger",
        "right-thumb", "right-index-finger", "right-middle-finger", "right-ring-finger", "right-little-finger"
    ];
    for finger in fingers {
        combo.append(Some(finger), finger);
    }
    combo.set_active(Some(0));
    combo
}

async fn handle_enrollment(window: &ApplicationWindow, finger_name: String) -> Result<()> {
    let conn = Connection::system().await?;
    let proxy = FprintDeviceProxy::new(&conn).await?;
    
    let dialog = gtk4::MessageDialog::new(
        Some(window),
        gtk4::DialogFlags::MODAL,
        gtk4::MessageType::Info,
        gtk4::ButtonsType::Cancel,
        "Place your finger on the sensor"
    );
    
    let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
    
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Cancel {
            dialog.destroy();
        }
    });
    
    dialog.show();
    
    // Start enrollment in a separate thread to not block the UI
    let dialog_weak = dialog.downgrade();
    let window_weak = window.downgrade();
    let sender = sender.clone();
    glib::spawn_future_local(async move {
        let result = proxy.enroll(&finger_name.as_str()).await;
        let _ = sender.send(result); // Send result back to main thread
    });

    receiver.attach(None, move |result| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.destroy();
            if let Some(window) = window_weak.upgrade() {
                match result {
                    Ok(_) => {
                        let success_dialog = gtk4::MessageDialog::new(
                            Some(&window),
                            gtk4::DialogFlags::MODAL,
                            gtk4::MessageType::Info,
                            gtk4::ButtonsType::Ok,
                            "Enrollment successful!"
                        );
                        success_dialog.show();
                    }
                    Err(e) => {
                        let error_dialog = gtk4::MessageDialog::new(
                            Some(&window),
                            gtk4::DialogFlags::MODAL,
                            gtk4::MessageType::Error,
                            gtk4::ButtonsType::Ok,
                            &format!("Enrollment failed: {}", e)
                        );
                        error_dialog.show();
                    }
                }
            }
        }
        glib::Continue(false)
    });

    Ok(())
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

    let finger_label = Label::new(Some("Select finger:"));
    let finger_selector = create_finger_selector();
    let enroll_button = Button::with_label("Enroll Selected Finger");
    let verify_button = Button::with_label("Verify Fingerprint");
    let list_button = Button::with_label("List Enrolled Fingerprints");
    let delete_button = Button::with_label("Delete Fingerprints");

    main_box.append(&finger_label);
    main_box.append(&finger_selector);
    main_box.append(&enroll_button);

    let window_weak = window.downgrade();
    enroll_button.connect_clicked(move |_| {
        if let Some(window) = window_weak.upgrade() {
            if let Some(finger) = finger_selector.active_text() {
                let finger_str = finger.to_string();
                glib::spawn_future_local(async move {
                    if let Err(e) = handle_enrollment(&window, finger_str).await {
                        let error_dialog = gtk4::MessageDialog::new(
                            Some(&window),
                            gtk4::DialogFlags::MODAL,
                            gtk4::MessageType::Error,
                            gtk4::ButtonsType::Ok,
                            &format!("Error: {}", e)
                        );
                        error_dialog.show();
                    }
                });
            }
        }
    });
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
