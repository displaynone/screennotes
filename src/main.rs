mod capture;
mod diagnostics;
mod editor;
mod tray;

use gtk::{gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

const APP_ID: &str = "io.github.screennotes.ScreenNotes";

#[derive(Debug)]
enum Event {
    Capture,
    Captured(Result<Option<PathBuf>, capture::CaptureError>),
    Recovered(Result<Option<PathBuf>, capture::CaptureError>),
    Demo,
    Help,
    Quit,
    Error(String),
}

fn message(app: &gtk::Application, heading: &str, detail: &str) {
    let parent = app.active_window();
    let dialog = gtk::AlertDialog::builder()
        .message(heading)
        .detail(detail)
        .build();
    dialog.show(parent.as_ref());
}

fn main() -> glib::ExitCode {
    if std::env::args().any(|s| s == "--diagnose") {
        diagnostics::print_report();
        return glib::ExitCode::SUCCESS;
    }
    if std::env::args().any(|s| s == "--help" || s == "-h") {
        println!(
            "ScreenNotes\n  --diagnose    Print desktop portal diagnostics\n  --background  Keep the indicator in the top bar\n  --capture     Take a screenshot (default)\n  --demo        Try the editor with a synthetic image\n  --quit        Quit the running instance\n\nCtrl+S / Ctrl+C: copy · Ctrl+Shift+S: save · Esc: cancel"
        );
        return glib::ExitCode::SUCCESS;
    }
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let (tx, rx) = async_channel::unbounded();
    let commands = tx.clone();
    app.connect_command_line(move |_, command| {
        let args = command.arguments();
        let known = ["--capture", "--background", "--demo", "--quit"];
        if args
            .iter()
            .skip(1)
            .any(|a| !known.contains(&a.to_string_lossy().as_ref()))
        {
            return glib::ExitCode::FAILURE;
        }
        let event = if args.iter().any(|a| a == "--quit") {
            Some(Event::Quit)
        } else if args.iter().any(|a| a == "--demo") {
            Some(Event::Demo)
        } else if args.iter().any(|a| a == "--background") {
            None
        } else {
            Some(Event::Capture)
        };
        if let Some(event) = event {
            let _ = commands.try_send(event);
        }
        glib::ExitCode::SUCCESS
    });
    app.connect_startup(move |app| {
        editor::install_css();
        let hold=app.hold();
        tray::start(tx.clone());
        let app=app.clone();
        let rx=rx.clone();
        let tx=tx.clone();
        let busy=Rc::new(Cell::new(false));
        let active:Rc<RefCell<Option<Rc<editor::Editor>>>>=Rc::new(RefCell::new(None));
        glib::spawn_future_local(async move {
            let _hold=hold;
            while let Ok(event)=rx.recv().await {
                match event {
                    Event::Capture=> {
                        if busy.replace(true) {continue;}
                        let tx=tx.clone();
                        // Let the indicator menu disappear before asking for a screenshot.
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(250));
                            let result=capture::screenshot();
                            let _=tx.send_blocking(Event::Captured(result));
                        });
                    }
                    Event::Captured(result)=> {
                        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
                        if result.as_ref().err().is_some_and(|error| capture::needs_focus_recovery(error, &desktop)) {
                            let app = app.clone();
                            let tx = tx.clone();
                            glib::spawn_future_local(async move {
                                let result = capture::recover_with_focus(&app).await;
                                let _ = tx.send(Event::Recovered(result)).await;
                            });
                        } else {
                            let _ = tx.try_send(Event::Recovered(result));
                        }
                    }
                    Event::Recovered(result)=> {
                        let result = result.map_err(|error| error.to_string());
                        if matches!(&result,Ok(None)) {busy.set(false);continue;}
                        let result=result.and_then(|path| editor::Editor::open(&app,path,busy.clone(),active.clone()));
                        match result { Ok(editor)=>*active.borrow_mut()=Some(editor),Err(e)=> {busy.set(false);message(&app,"Screenshot failed",&e);} }
                    }
                    Event::Demo=> {
                        if busy.replace(true) {continue;}
                        match editor::Editor::open(&app,None,busy.clone(),active.clone()) {
                            Ok(editor)=>*active.borrow_mut()=Some(editor),Err(e)=>{busy.set(false);message(&app,"Could not open the editor",&e);}
                        }
                    }
                    Event::Help=>message(&app,"ScreenNotes", "Drag to select. To select across monitors, click each corner separately. Use the floating toolbar to annotate.\n\nCtrl+S / Ctrl+C: copy\nCtrl+Shift+S: save PNG\nCtrl+Z / Ctrl+Shift+Z: undo / redo\nEsc: cancel\n\nThe app stays running to preserve the clipboard. Set up an Ubuntu keyboard shortcut for: screennotes --capture"),
                    Event::Error(e)=>message(&app,"ScreenNotes",&e),
                    Event::Quit=> {if let Some(editor)=active.borrow_mut().take(){editor.close();} app.quit();break;}
                }
            }
        });
    });
    app.run()
}
