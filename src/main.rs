use futures_util::StreamExt;
use gtk4::glib::{self, ControlFlow};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GBox, Button, ComboBoxText, Image, Label, Orientation, Stack,
};
use libadwaita as adw;
use zbus::{proxy, Connection};

const APP_ID: &str = "org.example.fprintui";

#[proxy(
    default_service = "net.reactivated.Fprint",
    interface = "net.reactivated.Fprint.Device",
    default_path = "/net/reactivated/Fprint/Device/0",
)]
trait FPrintDevice {
    fn list_enrolled_fingers(&self, username: &str) -> zbus::Result<Vec<String>>;

    fn delete_enrolled_fingers(&self, finger: &str) -> zbus::Result<()>;

    fn claim(&self, username: &str) -> zbus::Result<()>;
    fn release(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn enroll_status(&self, result: String, done: bool) -> zbus::Result<()>;

    fn enroll_start(&self, finger_name: &str) -> zbus::Result<()>;
    fn enroll_stop(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn verify_status(&self, result: String, done: bool) -> zbus::Result<()>;

    fn verify_start(&self, finger_name: &str) -> zbus::Result<()>;
    fn verify_stop(&self) -> zbus::Result<()>;
}

fn get_finger_icon(finger: &str) -> &str {
    match finger {
        "left-thumb" => "input-touchpad-symbolic",
        "left-index-finger" => "input-mouse-symbolic",
        "left-middle-finger" => "input-keyboard-symbolic",
        "left-ring-finger" => "input-tablet-symbolic",
        "left-little-finger" => "input-gaming-symbolic",
        "right-thumb" => "input-touchpad-symbolic",
        "right-index-finger" => "input-mouse-symbolic",
        "right-middle-finger" => "input-keyboard-symbolic",
        "right-ring-finger" => "input-tablet-symbolic",
        "right-little-finger" => "input-gaming-symbolic",
        _ => "dialog-question-symbolic",
    }
}

fn create_finger_selector() -> ComboBoxText {
    let combo = ComboBoxText::new();
    let fingers = [
        "left-thumb",
        "left-index-finger",
        "left-middle-finger",
        "left-ring-finger",
        "left-little-finger",
        "right-thumb",
        "right-index-finger",
        "right-middle-finger",
        "right-ring-finger",
        "right-little-finger",
    ];

    for finger in fingers {
        combo.append(Some(finger), finger);
        if let Some(cell) = combo.last_child() {
            if let Some(box_) = cell.first_child() {
                if let Ok(box_container) = box_.downcast::<gtk4::Box>() {
                    let icon = Image::from_icon_name(get_finger_icon(finger));
                    icon.set_pixel_size(24);
                    box_container.prepend(&icon);
                }
            }
        }
    }

    combo.set_active(Some(0));
    combo
}

async fn handle_verification(window: &ApplicationWindow, finger_name: String) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
    let proxy = FPrintDeviceProxy::new(&conn).await?;

    let dialog = gtk4::MessageDialog::new(
        Some(window),
        gtk4::DialogFlags::MODAL,
        gtk4::MessageType::Info,
        gtk4::ButtonsType::Cancel,
        "Place your finger on the sensor to verify",
    );

    let (sender, receiver) = async_channel::unbounded();

    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Cancel {
            dialog.destroy();
        }
    });

    dialog.show();

    // Start verification in a separate thread
    let sender = sender.clone();
    glib::spawn_future_local(async move {
        proxy.claim(&whoami::username()).await.unwrap();
        let _ = proxy.verify_start(&finger_name.as_str()).await;
        let mut verify_status_stream = proxy.receive_verify_status().await.unwrap();

        let result = loop {if let Some(msg) = verify_status_stream.next().await {
            // struct `JobNewArgs` is generated from `job_new` signal function arguments
            let args = msg.args().expect("Error parsing message");

            if !args.done {
                continue;
            }

            match dbg!(args.result.as_str()) {
                "verify-match" => {
                    break Ok(());
                },
                "verify-retry-scan" |
                "verify-swipe-too-short" |
                "verify-finger-not-centered" |
                "verify-remove-and-retry" => continue,
                _ => {
                    break Err(args.result);
                }
            }
        }};

        let _ = proxy.verify_stop().await;
        let _ = proxy.release().await;
        let _ = sender.send(result).await; // Send result back to main thread
        drop(proxy);
    });

    // Set up a recurring check for messages
    let dialog_weak = dialog.downgrade();
    let window_weak = window.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let receiver = receiver.clone();
        let dialog_weak = dialog_weak.clone();
        let window_weak = window_weak.clone();

        glib::spawn_future_local(async move {
            if let Ok(result) = receiver.try_recv() {
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
                                    "Verification successful!",
                                );
                                success_dialog.show();
                            }
                            Err(e) => {
                                let conn = Connection::system().await.unwrap();
                                let proxy = FPrintDeviceProxy::new(&conn).await.unwrap();
                                proxy.verify_stop().await.unwrap();
                                proxy.release().await.unwrap();
                                let error_dialog = gtk4::MessageDialog::new(
                                    Some(&window),
                                    gtk4::DialogFlags::MODAL,
                                    gtk4::MessageType::Error,
                                    gtk4::ButtonsType::Ok,
                                    &format!("Verification failed: {}", e),
                                );
                                error_dialog.show();
                            }
                        }
                    }
                }
                ControlFlow::Break
            } else {
                ControlFlow::Continue
            }
        });

        ControlFlow::Continue
    });

    Ok(())
}

async fn handle_enrollment(window: &ApplicationWindow, finger_name: String) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
    let proxy = FPrintDeviceProxy::new(&conn).await?;

    let dialog = gtk4::MessageDialog::new(
        Some(window),
        gtk4::DialogFlags::MODAL,
        gtk4::MessageType::Info,
        gtk4::ButtonsType::Cancel,
        "Place your finger on the sensor",
    );

    let (sender, receiver) = async_channel::unbounded();

    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Cancel {
            dialog.destroy();
        }
    });

    dialog.show();

    // Start enrollment in a separate thread to not block the UI
    let _dialog_weak = dialog.downgrade();
    let _window_weak = window.downgrade();
    let sender = sender.clone();
    glib::spawn_future_local(async move {
        proxy.claim(&whoami::username()).await.unwrap();
        let _ = proxy.enroll_start(&finger_name.as_str()).await;
        let mut enroll_status_stream = proxy.receive_enroll_status().await.unwrap();

        let result = loop {if let Some(msg) = enroll_status_stream.next().await {
            // struct `JobNewArgs` is generated from `job_new` signal function arguments
            let args = msg.args().expect("Error parsing message");

            if !args.done {
                continue;
            }



            match dbg!(args.result.as_str()) {

                "enroll-completed" => {
                    break Ok(());
                },
                "enroll-stage-passed" |
                "enroll-retry-scan" |
                "enroll-swipe-too-short" |
                "enroll-finger-not-centered" |
                "enroll-remove-and-retry" => continue,
                _ => {
                    break Err(args.result);
                }
            }
        }};

        let _ = proxy.enroll_stop().await;

                // "verify-match" => {
                //     break Ok(());
                // },
                // "verify-retry-scan" |
                // "verify-swipe-too-short" |
                // "verify-finger-not-centered" |
                // "verify-remove-and-retry" => continue,
                // _ => {
                //     break Err(args.result);
        // }
        let _ = proxy.release().await;
        let _ = sender.send(result).await; // Send result back to main thread
        drop(proxy);
    });

    // Set up a recurring check for messages
    let dialog_weak2 = dialog.downgrade();
    let window_weak2 = window.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let receiver = receiver.clone();
        let dialog_weak = dialog_weak2.clone();
        let window_weak = window_weak2.clone();

        glib::spawn_future_local(async move {
            let Ok(result) = receiver.try_recv() else {
                return ControlFlow::Continue; // Keep checking for messages
            };
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
                                "Enrollment successful!",
                            );
                            success_dialog.show();
                        }
                        Err(e) => {
                            let error_dialog = gtk4::MessageDialog::new(
                                Some(&window),
                                gtk4::DialogFlags::MODAL,
                                gtk4::MessageType::Error,
                                gtk4::ButtonsType::Ok,
                                &format!("Enrollment failed: {}", e),
                            );
                            error_dialog.show();
                        }
                    }
                }
            }
            ControlFlow::Break // Stop the timeout after receiving the message
        });

        ControlFlow::Continue
    });

    Ok(())
}

fn create_page_content(title: &str, window: &ApplicationWindow, stack: &Stack) -> GBox {
    let page = GBox::new(Orientation::Vertical, 10);
    page.set_margin_start(10);
    page.set_margin_end(10);
    page.set_margin_top(10);
    page.set_margin_bottom(10);

    if title != "Main Menu" {
        let header = GBox::new(Orientation::Horizontal, 10);
        let back_button = Button::with_label("Back");
        let title_label = Label::new(Some(title));
        header.append(&back_button);
        header.append(&title_label);
        page.append(&header);

        let stack_weak = stack.downgrade();
        back_button.connect_clicked(move |_| {
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name("main");
            }
        });
    }

    if title != "Main Menu" {
        let finger_label = Label::new(Some("Select finger:"));
        let finger_selector = create_finger_selector();
        page.append(&finger_label);
        page.append(&finger_selector);

        match title {
            "Enroll Fingerprint" => {
                let enroll_button = Button::with_label("Enroll");
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
                                        &format!("Error: {}", e),
                                    );
                                    error_dialog.connect_response(|dialog, _| {
                                        dialog.destroy();
                                    });
                                    error_dialog.show();
                                }
                            });
                        }
                    }
                });
                page.append(&enroll_button);
            }
            "Verify Fingerprint" => {
                let verify_button = Button::with_label("Verify");
                let window_weak = window.downgrade();
                verify_button.connect_clicked(move |_| {
                    if let Some(window) = window_weak.upgrade() {
                        let Some(finger_name) = finger_selector.active_text().map(String::from)
                        else {
                            return;
                        };
                        glib::spawn_future_local(async move {
                            if let Err(e) = handle_verification(&window, finger_name).await {
                                let error_dialog = gtk4::MessageDialog::new(
                                    Some(&window),
                                    gtk4::DialogFlags::MODAL,
                                    gtk4::MessageType::Error,
                                    gtk4::ButtonsType::Ok,
                                    &format!("Error: {}", e),
                                );
                                error_dialog.connect_response(|dialog, _| {
                                    dialog.destroy();
                                });
                                error_dialog.show();
                            }
                        });
                    }
                });
                page.append(&verify_button);
            }
            "List Fingerprints" => {
                let list_button = Button::with_label("List");
                page.append(&list_button);
            }
            "Delete Fingerprint" => {
                let delete_button = Button::with_label("Delete");
                page.append(&delete_button);
            }
            _ => {}
        }
    }

    page
}

async fn get_enrolled_fingers() -> anyhow::Result<Vec<String>> {
    let conn = Connection::system().await?;
    let proxy = FPrintDeviceProxy::new(&conn).await?;
    Ok(proxy.list_enrolled_fingers(&whoami::username()).await?)
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Fingerprint Manager")
        .default_width(400)
        .default_height(300)
        .build();

    let stack = Stack::new();

    // Create main menu
    let main_page = create_page_content("Main Menu", &window, &stack);

    let enroll_button = Button::with_label("Enroll Fingerprint");
    let verify_button = Button::with_label("Verify Fingerprint");
    let delete_button = Button::with_label("Delete Fingerprint");

    // Add enrolled fingers list
    let enrolled_list = Label::new(Some("Loading enrolled fingerprints..."));
    enrolled_list.set_margin_top(20);

    let stack_weak = stack.downgrade();
    enroll_button.connect_clicked(move |_| {
        if let Some(stack) = stack_weak.upgrade() {
            stack.set_visible_child_name("enroll");
        }
    });

    let stack_weak = stack.downgrade();
    verify_button.connect_clicked(move |_| {
        if let Some(stack) = stack_weak.upgrade() {
            stack.set_visible_child_name("verify");
        }
    });

    let stack_weak = stack.downgrade();
    delete_button.connect_clicked(move |_| {
        if let Some(stack) = stack_weak.upgrade() {
            stack.set_visible_child_name("delete");
        }
    });

    main_page.append(&enroll_button);
    main_page.append(&verify_button);
    main_page.append(&delete_button);
    main_page.append(&enrolled_list);

    // Set up enrolled fingers list update
    let enrolled_list_weak = enrolled_list.downgrade();
    glib::spawn_future_local(async move {
        match get_enrolled_fingers().await {
            Ok(fingers) => {
                if let Some(label) = enrolled_list_weak.upgrade() {
                    if fingers.is_empty() {
                        label.set_text("No fingerprints enrolled");
                    } else {
                        label.set_text(&format!("Enrolled fingerprints:\n{}", fingers.join("\n")));
                    }
                }
            }
            Err(e) => {
                if let Some(label) = enrolled_list_weak.upgrade() {
                    label.set_text(&format!("Error loading fingerprints: {}", e));
                }
            }
        }
    });

    stack.add_named(&main_page, Some("main"));

    // Create other pages
    let enroll_page = create_page_content("Enroll Fingerprint", &window, &stack);
    let verify_page = create_page_content("Verify Fingerprint", &window, &stack);
    let delete_page = create_page_content("Delete Fingerprint", &window, &stack);

    stack.add_named(&enroll_page, Some("enroll"));
    stack.add_named(&verify_page, Some("verify"));
    stack.add_named(&delete_page, Some("delete"));

    stack.set_visible_child_name("main");

    window.set_child(Some(&stack));
    window.present();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    adw::init()?;

    let app = Application::builder().application_id(APP_ID).build();

    // let conn = Connection::system().await?;
    // let proxy = FprintDeviceProxy::new(&conn).await?;

    // proxy.release().await?;

    // let v = proxy.clone();

    app.connect_activate(move |app| {
        // let value = v.clone();
        build_ui(app);
        // glib::spawn_future_local(async move {
        //     // let _ = value.claim(&whoami::username()).await;
        // });
    });
    // app.connect_shutdown(move |_| {
    //     let value = proxy.clone();
    //     glib::spawn_future_local(async move {
    //         // let _ = value.release().await;
    //     });
    // });
    app.run();

    Ok(())
}
