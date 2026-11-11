use std::path::PathBuf;

// ashpd caches its D-Bus connection globally. Its I/O tasks must outlive each
// individual capture, including the initial failure and permission retry.
fn portal_runtime() -> Result<&'static tokio::runtime::Runtime, CaptureError> {
    static RUNTIME: std::sync::OnceLock<Result<tokio::runtime::Runtime, String>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| CaptureError::Other(error.clone()))
}

/// Cancellation is an ordinary outcome, not a failure dialog.
pub fn screenshot() -> Result<Option<PathBuf>, CaptureError> {
    let runtime = portal_runtime()?;
    runtime.block_on(async {
        let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            ashpd::desktop::screenshot::Screenshot::request()
                .interactive(false)
                .modal(false)
                .send()
                .await?
                .response()
        })
        .await
        .map_err(|_| CaptureError::Other("The desktop portal did not respond within 60 seconds. Close any pending permission dialog and try again.".into()))?;
        match result {
            Ok(response) => {
                response.uri().to_file_path().map(Some).map_err(|_| {
                    CaptureError::Other("The portal did not return a local file.".into())
                })
            }
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
            | Err(ashpd::Error::Portal(ashpd::PortalError::Cancelled(_))) => Ok(None),
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)) => {
                Err(CaptureError::PortalOther)
            }
            Err(error) => Err(CaptureError::Other(error.to_string())),
        }
    })
}

#[derive(Debug)]
pub enum CaptureError {
    PortalOther,
    Other(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = match self {
            Self::PortalOther => "The desktop portal returned Other.",
            Self::Other(detail) => detail,
        };
        write!(
            f,
            "The desktop portal could not take a screenshot.\n\nCheck screenshot access for ScreenNotes in Settings → Apps. A denied permission or a desktop portal failure can cause this error.\n\nRun screennotes --diagnose for troubleshooting details.\n\nPortal error: {detail}"
        )
    }
}

pub fn needs_focus_recovery(error: &CaptureError, desktop: &str) -> bool {
    matches!(error, CaptureError::PortalOther)
        && desktop
            .split(':')
            .any(|part| part.eq_ignore_ascii_case("gnome"))
}

async fn screenshot_async() -> Result<Option<PathBuf>, CaptureError> {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(screenshot());
    });
    rx.recv()
        .await
        .map_err(|e| CaptureError::Other(e.to_string()))?
}

/// GNOME 50 AccessDialog checks focus_app.id before displaying permission UI.
/// A tray-only app has no focused window. Ask the user to focus a real app window
/// and discard the authorization screenshot before capturing the clean desktop.
pub async fn recover_with_focus(app: &gtk::Application) -> Result<Option<PathBuf>, CaptureError> {
    recover_with_focus_using(app, screenshot_async).await
}

async fn recover_with_focus_using<F, Fut>(
    app: &gtk::Application,
    capture: F,
) -> Result<Option<PathBuf>, CaptureError>
where
    F: Fn() -> Fut + Clone + 'static,
    Fut: std::future::Future<Output = Result<Option<PathBuf>, CaptureError>> + 'static,
{
    use gtk::prelude::*;
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("ScreenNotes — Screenshot access")
        .default_width(420)
        .resizable(false)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    let label = gtk::Label::new(Some(
        "GNOME needs ScreenNotes to be focused before it can ask for screenshot access.\n\nClick Continue, then allow access in the system dialog. This window will close before your screenshot is taken.",
    ));
    label.set_wrap(true);
    label.set_max_width_chars(48);
    content.append(&label);
    let button = gtk::Button::with_label("Continue");
    button.add_css_class("suggested-action");
    content.append(&button);
    window.set_child(Some(&content));
    enum Action {
        Continue,
        Cancel,
        Finished(Result<Option<PathBuf>, CaptureError>),
    }
    let (tx, rx) = async_channel::unbounded();
    let close_tx = tx.clone();
    window.connect_close_request(move |_| {
        let _ = close_tx.try_send(Action::Cancel);
        gtk::glib::Propagation::Proceed
    });
    let button_tx = tx.clone();
    let weak = window.downgrade();
    button.connect_clicked(move |button| {
        if let Some(window) = weak.upgrade() {
            if !window.is_active() {
                window.present();
                return;
            }
            button.set_sensitive(false);
            let _ = button_tx.try_send(Action::Continue);
        }
    });
    window.present();
    let mut started = false;
    let authorization = loop {
        match rx.recv().await {
            Ok(Action::Continue) if !started => {
                started = true;
                label.set_text("Waiting for the desktop permission dialog…\n\nChoose Allow to continue. You can close this window to cancel.");
                let tx = tx.clone();
                let capture = capture.clone();
                gtk::glib::spawn_future_local(async move {
                    let result = capture().await;
                    let _ = tx.try_send(Action::Finished(result));
                });
            }
            Ok(Action::Continue) => {}
            Ok(Action::Finished(result)) => break result,
            Ok(Action::Cancel) | Err(_) => {
                window.destroy();
                return Ok(None);
            }
        }
    };
    window.destroy();
    match authorization? {
        None => Ok(None),
        Some(_authorization_image) => {
            // Never show the frame containing the permission helper in the editor.
            gtk::glib::timeout_future(std::time::Duration::from_millis(350)).await;
            capture().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a session desktop portal; only reads appearance settings"]
    fn portal_connection_survives_successive_requests() {
        for _ in 0..3 {
            portal_runtime().unwrap().block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    let settings = ashpd::desktop::settings::Settings::new().await.unwrap();
                    settings.color_scheme().await.unwrap();
                })
                .await
                .expect("Cached D-Bus connection must stay responsive");
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    #[ignore = "requires a graphical session; opens and cancels a permission helper"]
    fn permission_helper_cancels_while_portal_is_pending() {
        use gtk::prelude::*;
        use std::{cell::Cell, rc::Rc};
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.screennotes.PermissionTest")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let started = Rc::new(Cell::new(false));
        let probe = started.clone();
        let app_probe = app.clone();
        gtk::glib::spawn_future_local(async move {
            for _ in 0..100 {
                gtk::glib::timeout_future(std::time::Duration::from_millis(30)).await;
                if let Some(window) = app_probe.active_window()
                    && window.is_active()
                {
                    let content = window.child().unwrap().downcast::<gtk::Box>().unwrap();
                    let button = content
                        .last_child()
                        .unwrap()
                        .downcast::<gtk::Button>()
                        .unwrap();
                    if !probe.get() {
                        button.emit_clicked();
                    } else {
                        window.close();
                        return;
                    }
                }
            }
            panic!("Permission helper never became active");
        });
        let capture_started = started.clone();
        let result = gtk::glib::MainContext::default()
            .block_on(async {
                gtk::glib::future_with_timeout(
                    std::time::Duration::from_secs(5),
                    recover_with_focus_using(&app, move || {
                        capture_started.set(true);
                        std::future::pending::<Result<Option<PathBuf>, CaptureError>>()
                    }),
                )
                .await
            })
            .expect("Cancellation must not wait for the portal")
            .unwrap();
        assert!(started.get());
        assert!(result.is_none());
        assert!(app.windows().is_empty());
    }

    #[test]
    fn focus_recovery_is_limited_to_gnome_generic_portal_failure() {
        assert!(needs_focus_recovery(
            &CaptureError::PortalOther,
            "ubuntu:GNOME"
        ));
        assert!(!needs_focus_recovery(&CaptureError::PortalOther, "KDE"));
        assert!(!needs_focus_recovery(
            &CaptureError::Other("D-Bus unavailable".into()),
            "GNOME"
        ));
    }
}
