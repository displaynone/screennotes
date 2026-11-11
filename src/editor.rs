use gtk::{cairo, gdk, gio, glib, prelude::*};
use screennotes::{model::*, render};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::{Rc, Weak},
};

struct View {
    window: gtk::ApplicationWindow,
    canvas: gtk::DrawingArea,
    status: gtk::Label,
    controls: Controls,
    toolbar: gtk::Box,
}

struct Controls {
    move_bar: gtk::Button,
    width: gtk::SpinButton,
    size: gtk::SpinButton,
    fonts: gtk::DropDown,
    text: gtk::Entry,
    color: gtk::Entry,
    tools: Vec<(Tool, gtk::Button)>,
}

struct State {
    doc: Document,
    tool: Tool,
    style: Style,
    text: String,
    anchor: Option<Point>,
    resize: Option<(Rect, CropHandle, Point)>,
    preview: Option<Annotation>,
}

pub struct Editor {
    app: gtk::Application,
    desktop: Desktop,
    background: cairo::ImageSurface,
    state: RefCell<State>,
    views: RefCell<Vec<View>>,
    busy: Rc<Cell<bool>>,
    owner: Weak<RefCell<Option<Rc<Editor>>>>,
    closed: Cell<bool>,
    saving: Cell<bool>,
    syncing: Cell<bool>,
    monitor_signal: RefCell<Option<(gio::ListModel, glib::SignalHandlerId)>>,
    geometry_signals: RefCell<Vec<(gdk::Monitor, glib::SignalHandlerId)>>,
}

impl Editor {
    pub fn open(
        app: &gtk::Application,
        path: Option<PathBuf>,
        busy: Rc<Cell<bool>>,
        owner: Rc<RefCell<Option<Rc<Self>>>>,
    ) -> Result<Rc<Self>, String> {
        let display = gdk::Display::default().ok_or("No graphical session was found.")?;
        let list = display.monitors();
        let monitors: Vec<gdk::Monitor> = (0..list.n_items())
            .filter_map(|i| list.item(i)?.downcast().ok())
            .collect();
        let geometry: Vec<Rect> = monitors
            .iter()
            .map(|m| {
                let r = m.geometry();
                Rect {
                    x: r.x() as f64,
                    y: r.y() as f64,
                    width: r.width() as f64,
                    height: r.height() as f64,
                }
            })
            .collect();
        let background = if let Some(path) = path {
            let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file(path).map_err(|e| e.to_string())?;
            let surface =
                cairo::ImageSurface::create(cairo::Format::ARgb32, pixbuf.width(), pixbuf.height())
                    .map_err(|e| e.to_string())?;
            {
                let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
                cr.set_source_pixbuf(&pixbuf, 0., 0.);
                cr.paint().map_err(|e| e.to_string())?;
            }
            surface
        } else {
            let bounds = geometry
                .iter()
                .copied()
                .reduce(Rect::union)
                .ok_or("No monitors were found.")?;
            let surface = cairo::ImageSurface::create(
                cairo::Format::ARgb32,
                bounds.width as i32,
                bounds.height as i32,
            )
            .map_err(|e| e.to_string())?;
            {
                let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
                cr.set_source_rgb(0.08, 0.11, 0.17);
                let _ = cr.paint();
                for (i, r) in geometry.iter().enumerate() {
                    cr.set_source_rgb(0.12 + i as f64 * 0.04, 0.2, 0.28);
                    cr.rectangle(
                        r.x - bounds.x + 30.,
                        r.y - bounds.y + 30.,
                        r.width - 60.,
                        r.height - 60.,
                    );
                    let _ = cr.fill();
                    let a = Annotation {
                        shape: Shape::Text(
                            Point {
                                x: r.x - bounds.x + 70.,
                                y: r.y - bounds.y + 150.,
                            },
                            format!(
                                "ScreenNotes · Monitor {}\nTest image · {} × {}",
                                i + 1,
                                r.width,
                                r.height
                            ),
                        ),
                        style: Style {
                            color: (0.85, 0.95, 1., 1.),
                            font: "Sans 26".into(),
                            ..Default::default()
                        },
                    };
                    let _ = render::annotation(&cr, &a);
                }
            }
            surface
        };
        let desktop = Desktop::new(geometry, (background.width(), background.height()))?;
        let editor = Rc::new(Self {
            app: app.clone(),
            desktop,
            background,
            state: RefCell::new(State {
                doc: Document::default(),
                tool: Tool::Select,
                style: Style::default(),
                text: String::new(),
                anchor: None,
                resize: None,
                preview: None,
            }),
            views: RefCell::new(vec![]),
            busy,
            owner: Rc::downgrade(&owner),
            closed: Cell::new(false),
            saving: Cell::new(false),
            syncing: Cell::new(false),
            monitor_signal: RefCell::new(None),
            geometry_signals: RefCell::new(Vec::new()),
        });
        for (i, monitor) in monitors.iter().enumerate() {
            editor.add_view(monitor, i);
            for property in ["geometry", "scale-factor"] {
                let weak = Rc::downgrade(&editor);
                let signal = monitor.connect_notify_local(Some(property), move |_, _| {
                    if let Some(e) = weak.upgrade() {
                        e.close();
                        crate::message(
                            &e.app,
                            "Monitor configuration changed",
                            "Take another screenshot to use the new screen geometry or scale.",
                        );
                    }
                });
                editor
                    .geometry_signals
                    .borrow_mut()
                    .push((monitor.clone(), signal));
            }
        }
        editor.refresh();
        let weak = Rc::downgrade(&editor);
        let signal = list.connect_items_changed(move |_, _, _, _| {
            if let Some(e) = weak.upgrade() {
                e.close();
                crate::message(
                    &e.app,
                    "Monitor configuration changed",
                    "Take another screenshot to use the new monitor layout.",
                );
            }
        });
        *editor.monitor_signal.borrow_mut() = Some((list, signal));
        let weak = Rc::downgrade(&editor);
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            if let Some(e) = weak.upgrade()
                && !e.closed.get()
                && e.views.borrow().iter().any(|v| !v.window.is_fullscreen())
            {
                e.close();
                crate::message(
                    &e.app,
                    "The desktop did not allow the overlay",
                    "Could not open a fullscreen surface on every monitor. The capture was canceled to avoid incorrect coordinates.",
                );
            }
        });
        Ok(editor)
    }

    fn add_view(self: &Rc<Self>, monitor: &gdk::Monitor, index: usize) {
        let bounds = self.desktop.monitors[index];
        let window = gtk::ApplicationWindow::builder()
            .application(&self.app)
            .title("ScreenNotes · Capture")
            .decorated(false)
            .build();
        window.set_default_size(bounds.width as i32, bounds.height as i32);
        let overlay = gtk::Overlay::new();
        let canvas = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .build();
        canvas.set_cursor_from_name(Some("crosshair"));
        overlay.set_child(Some(&canvas));
        let weak = Rc::downgrade(self);
        canvas.set_draw_func(move |_, cr, _, _| {
            if let Some(e) = weak.upgrade() {
                e.draw(cr, bounds);
            }
        });
        let status = gtk::Label::new(Some(
            "Drag to select a region · or click each corner, even on different monitors · Esc cancels",
        ));
        status.add_css_class("status");
        status.set_wrap(true);
        status.set_halign(gtk::Align::Center);
        status.set_valign(gtk::Align::Start);
        status.set_margin_top(18);
        overlay.add_overlay(&status);
        let (toolbar, controls) = self.toolbar();
        overlay.add_overlay(&toolbar);
        window.set_child(Some(&overlay));

        // GTK's drag has an implicit pointer grab. Two-click selection is also supported
        // because some Wayland compositors constrain cross-output drag coordinates.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let weak = Rc::downgrade(self);
        drag.connect_drag_begin(move |_, x, y| {
            if let Some(e) = weak.upgrade() {
                e.begin(Point {
                    x: x + bounds.x,
                    y: y + bounds.y,
                });
            }
        });
        let weak = Rc::downgrade(self);
        drag.connect_drag_update(move |gesture, dx, dy| {
            if let (Some(e), Some((x, y))) = (weak.upgrade(), gesture.start_point()) {
                e.update(Point {
                    x: x + bounds.x + dx,
                    y: y + bounds.y + dy,
                });
            }
        });
        let weak = Rc::downgrade(self);
        drag.connect_drag_end(move |gesture, dx, dy| {
            if let (Some(e), Some((x, y))) = (weak.upgrade(), gesture.start_point()) {
                e.end(
                    Point {
                        x: x + bounds.x + dx,
                        y: y + bounds.y + dy,
                    },
                    dx.hypot(dy) > 3.,
                );
            }
        });
        let weak = Rc::downgrade(self);
        drag.connect_cancel(move |_, _| {
            if let Some(e) = weak.upgrade() {
                let original = e.state.borrow_mut().resize.take();
                if let Some((original, _, _)) = original {
                    e.state.borrow_mut().doc.selection = Some(original);
                    e.refresh();
                }
            }
        });
        canvas.add_controller(drag);
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_motion(move |controller, x, y| {
            if let Some(e) = weak.upgrade() {
                let p = Point {
                    x: x + bounds.x,
                    y: y + bounds.y,
                };
                let moving = {
                    let s = e.state.borrow();
                    s.anchor.is_some() || s.resize.is_some()
                };
                if moving {
                    e.update(p);
                }
                let s = e.state.borrow();
                let handle = s
                    .resize
                    .map(|(_, handle, _)| handle)
                    .or_else(|| s.doc.selection.and_then(|r| CropHandle::hit(r, p)));
                if let Some(widget) = controller.widget() {
                    widget
                        .set_cursor_from_name(Some(handle.map_or("crosshair", CropHandle::cursor)));
                }
            }
        });
        canvas.add_controller(motion);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(e) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            if key == gdk::Key::F6 {
                if let Some(view) = e.views.borrow().iter().find(|v| v.window.is_active()) {
                    toggle_toolbar(&view.toolbar, &view.controls.move_bar);
                }
            } else if key == gdk::Key::Escape {
                e.close();
            } else if ctrl && (key == gdk::Key::s || key == gdk::Key::S) {
                if shift {
                    e.save();
                } else {
                    e.copy();
                }
            } else if ctrl && key == gdk::Key::c {
                // Preserve ordinary copying while editing the text/colour input.
                let editing = e
                    .views
                    .borrow()
                    .iter()
                    .filter_map(|v| gtk::prelude::GtkWindowExt::focus(&v.window))
                    .any(|w| w.is::<gtk::Text>() || w.is::<gtk::Entry>());
                if editing {
                    return glib::Propagation::Proceed;
                }
                e.copy();
            } else if ctrl && (key == gdk::Key::z || key == gdk::Key::Z) {
                e.history(shift);
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        window.add_controller(keys);
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| {
            if let Some(e) = weak.upgrade() {
                e.close();
            }
            glib::Propagation::Stop
        });
        self.views.borrow_mut().push(View {
            window: window.clone(),
            canvas: canvas.clone(),
            status,
            controls,
            toolbar,
        });
        window.fullscreen_on_monitor(monitor);
        window.present();
        canvas.grab_focus();
    }

    fn toolbar(self: &Rc<Self>) -> (gtk::Box, Controls) {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
        panel.add_css_class("toolbar");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::End);
        panel.set_margin_bottom(24);
        panel.set_margin_top(60);
        panel.set_margin_start(16);
        panel.set_margin_end(16);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        let mut tools = Vec::new();
        for (label, tool) in [
            ("↖ Region", Tool::Select),
            ("╱ Line", Tool::Line),
            ("□ Rectangle", Tool::Rectangle),
            ("T Text", Tool::Text),
        ] {
            let weak = Rc::downgrade(self);
            let button = gtk::Button::with_label(label);
            button.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    let mut s = e.state.borrow_mut();
                    s.tool = tool;
                    s.anchor = None;
                    s.resize = None;
                    s.preview = None;
                    drop(s);
                    e.refresh();
                }
            });
            row.append(&button);
            tools.push((tool, button));
        }
        for (label, action) in [("↶", 0), ("↷", 1), ("Copy", 2), ("Save…", 3), ("✕", 4)] {
            let weak = Rc::downgrade(self);
            let button = gtk::Button::with_label(label);
            button.set_tooltip_text(Some(match action {
                0 => "Undo (Ctrl+Z)",
                1 => "Redo (Ctrl+Shift+Z)",
                2 => "Copy (Ctrl+S)",
                3 => "Save PNG (Ctrl+Shift+S)",
                _ => "Cancel (Esc)",
            }));
            button.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    match action {
                        0 => e.history(false),
                        1 => e.history(true),
                        2 => e.copy(),
                        3 => e.save(),
                        _ => e.close(),
                    }
                }
            });
            row.append(&button);
        }
        let move_bar = gtk::Button::with_label("Move up");
        move_bar.set_tooltip_text(Some("Move toolbar to top (F6)"));
        move_bar.set_focus_on_click(false);
        let weak_panel = panel.downgrade();
        move_bar.connect_clicked(move |button| {
            if let Some(panel) = weak_panel.upgrade() {
                toggle_toolbar(&panel, button);
            }
        });
        row.append(&move_bar);
        panel.append(&row);
        let style = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        style.append(&gtk::Label::new(Some("Color")));
        for (label, color) in [
            ("🔴", (1., 0.25, 0.25, 1.)),
            ("🟡", (1., 0.8, 0.15, 1.)),
            ("🟢", (0.2, 0.9, 0.5, 1.)),
            ("🔵", (0.2, 0.55, 1., 1.)),
            ("⚪", (1., 1., 1., 1.)),
            ("⚫", (0., 0., 0., 1.)),
        ] {
            let b = gtk::Button::with_label(label);
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    e.state.borrow_mut().style.color = color;
                    e.refresh();
                }
            });
            style.append(&b);
        }
        let color = gtk::Entry::builder()
            .placeholder_text("#RRGGBB")
            .width_chars(9)
            .max_width_chars(9)
            .build();
        color.set_tooltip_text(Some("Custom color: enter #RRGGBB and press Enter"));
        let weak = Rc::downgrade(self);
        color.connect_activate(move |entry| {
            if let Some(e) = weak.upgrade() {
                if let Ok(rgba) = gdk::RGBA::parse(entry.text()) {
                    e.state.borrow_mut().style.color = (
                        rgba.red() as f64,
                        rgba.green() as f64,
                        rgba.blue() as f64,
                        rgba.alpha() as f64,
                    );
                    entry.remove_css_class("error");
                    e.refresh();
                } else {
                    entry.add_css_class("error");
                }
            }
        });
        style.append(&color);
        style.append(&gtk::Label::new(Some("Width")));
        let width = gtk::SpinButton::with_range(1., 40., 1.);
        width.set_value(3.);
        let weak = Rc::downgrade(self);
        width.connect_value_changed(move |v| {
            if let Some(e) = weak.upgrade() {
                if e.syncing.get() {
                    return;
                }
                e.state.borrow_mut().style.width = v.value();
                e.refresh();
            }
        });
        style.append(&width);
        panel.append(&style);
        let textrow = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let text = gtk::Entry::builder()
            .placeholder_text("Enter text here, then click where you want to place it")
            .hexpand(true)
            .build();
        let weak = Rc::downgrade(self);
        text.connect_changed(move |entry| {
            if let Some(e) = weak.upgrade() {
                if e.syncing.get() {
                    return;
                }
                let mut s = e.state.borrow_mut();
                s.text = entry.text().to_string();
                s.tool = Tool::Text;
                s.anchor = None;
                s.resize = None;
                drop(s);
                e.refresh();
            }
        });
        textrow.append(&text);
        let mut names: Vec<String> = panel
            .pango_context()
            .list_families()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        names.sort();
        names.dedup();
        if !names.iter().any(|n| n == "Sans") {
            names.insert(0, "Sans".into());
        }
        let fonts =
            gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
        fonts.set_selected(names.iter().position(|n| n == "Sans").unwrap_or(0) as u32);
        fonts.set_enable_search(true);
        fonts.set_size_request(150, -1);
        let weak = Rc::downgrade(self);
        fonts.connect_selected_notify(move |f| {
            if let Some(e) = weak.upgrade() {
                if e.syncing.get() {
                    return;
                }
                if let Some(name) = names.get(f.selected() as usize) {
                    let mut s = e.state.borrow_mut();
                    let mut d = gtk::pango::FontDescription::from_string(&s.style.font);
                    d.set_family(name);
                    s.style.font = d.to_string().to_string();
                    drop(s);
                    e.refresh();
                }
            }
        });
        textrow.append(&fonts);
        let size = gtk::SpinButton::with_range(8., 160., 1.);
        size.set_value(20.);
        let weak = Rc::downgrade(self);
        size.connect_value_changed(move |v| {
            if let Some(e) = weak.upgrade() {
                if e.syncing.get() {
                    return;
                }
                let mut s = e.state.borrow_mut();
                let mut d = gtk::pango::FontDescription::from_string(&s.style.font);
                d.set_size((v.value() * gtk::pango::SCALE as f64) as i32);
                s.style.font = d.to_string().to_string();
                drop(s);
                e.refresh();
            }
        });
        textrow.append(&size);
        panel.append(&textrow);
        (
            panel,
            Controls {
                move_bar,
                width,
                size,
                fonts,
                text,
                color,
                tools,
            },
        )
    }

    fn begin(&self, p: Point) {
        let mut s = self.state.borrow_mut();
        if s.anchor.is_none()
            && let Some(r) = s.doc.selection
            && let Some(handle) = CropHandle::hit(r, p)
        {
            let corner = handle.position(r);
            s.resize = Some((
                r,
                handle,
                Point {
                    x: corner.x - p.x,
                    y: corner.y - p.y,
                },
            ));
            s.preview = None;
            return;
        }
        if s.tool == Tool::Text {
            if s.doc.selection.is_some_and(|r| r.contains(p)) && !s.text.trim().is_empty() {
                let a = Annotation {
                    shape: Shape::Text(p, s.text.clone()),
                    style: s.style.clone(),
                };
                s.doc.add(a);
            }
        } else if s.anchor.is_none() {
            if s.tool != Tool::Select && !s.doc.selection.is_some_and(|r| r.contains(p)) {
                return;
            }
            s.anchor = Some(p);
        }
        drop(s);
        self.refresh();
    }

    fn update(&self, p: Point) {
        let mut s = self.state.borrow_mut();
        if let Some((original, handle, offset)) = s.resize {
            s.doc.selection = Some(handle.resize(
                original,
                Point {
                    x: p.x + offset.x,
                    y: p.y + offset.y,
                },
                self.desktop.bounds,
            ));
            drop(s);
            self.refresh();
            return;
        }
        let Some(a) = s.anchor else {
            return;
        };
        match s.tool {
            Tool::Select => s.doc.selection = Rect::between(a, p).intersect(self.desktop.bounds),
            Tool::Line => {
                s.preview = Some(Annotation {
                    shape: Shape::Line(a, p),
                    style: s.style.clone(),
                })
            }
            Tool::Rectangle => {
                s.preview = Some(Annotation {
                    shape: Shape::Rectangle(Rect::between(a, p)),
                    style: s.style.clone(),
                })
            }
            Tool::Text => {}
        }
        drop(s);
        self.refresh();
    }

    fn end(&self, p: Point, dragged: bool) {
        if self.state.borrow().resize.is_some() {
            self.update(p);
            self.state.borrow_mut().resize = None;
            self.refresh();
            return;
        }
        let anchor = self.state.borrow().anchor;
        let Some(a) = anchor else {
            return;
        };
        if !dragged && (p.x - a.x).hypot(p.y - a.y) < 3. {
            return;
        }
        self.update(p);
        let mut s = self.state.borrow_mut();
        if let Some(a) = s.preview.take() {
            s.doc.add(a);
        }
        s.anchor = None;
        drop(s);
        self.refresh();
    }

    fn draw(&self, cr: &cairo::Context, monitor: Rect) {
        let _ = cr.save();
        // Image pixels -> desktop logical coordinates -> this output's local coordinates.
        cr.translate(
            self.desktop.bounds.x - monitor.x,
            self.desktop.bounds.y - monitor.y,
        );
        cr.scale(1. / self.desktop.scale, 1. / self.desktop.scale);
        let _ = cr.set_source_surface(&self.background, 0., 0.);
        let _ = cr.paint();
        let _ = cr.restore();
        let s = self.state.borrow();
        let _ = cr.save();
        cr.translate(-monitor.x, -monitor.y);
        if let Some(r) = s.doc.selection {
            let _ = cr.save();
            cr.rectangle(r.x, r.y, r.width, r.height);
            cr.clip();
            for a in &s.doc.annotations {
                let _ = render::annotation(cr, a);
            }
            if let Some(a) = &s.preview {
                let _ = render::annotation(cr, a);
            }
            let _ = cr.restore();
        }
        cr.set_fill_rule(cairo::FillRule::EvenOdd);
        cr.rectangle(monitor.x, monitor.y, monitor.width, monitor.height);
        if let Some(r) = s.doc.selection {
            cr.rectangle(r.x, r.y, r.width, r.height);
        }
        cr.set_source_rgba(0., 0., 0., 0.48);
        let _ = cr.fill();
        if let Some(r) = s.doc.selection {
            cr.set_source_rgb(0.4, 0.8, 1.);
            cr.set_line_width(1.);
            cr.rectangle(r.x, r.y, r.width, r.height);
            let _ = cr.stroke();
            for handle in CropHandle::ALL {
                let point = handle.position(r);
                cr.rectangle(point.x - 4., point.y - 4., 8., 8.);
                cr.set_source_rgb(1., 1., 1.);
                let _ = cr.fill_preserve();
                cr.set_source_rgb(0.1, 0.35, 0.55);
                cr.set_line_width(2.);
                let _ = cr.stroke();
            }
        }
        let _ = cr.restore();
    }

    fn refresh(&self) {
        let s = self.state.borrow();
        let tool = match s.tool {
            Tool::Select => "Region",
            Tool::Line => "Line",
            Tool::Rectangle => "Rectangle",
            Tool::Text => "Text",
        };
        let (r, g, b, _) = s.style.color;
        let selection = s
            .doc
            .selection
            .and_then(|r| self.desktop.pixel_crop(r))
            .map(|(_, _, w, h)| format!("{w} × {h} px"))
            .unwrap_or_else(|| "Select a region".into());
        let status = format!(
            "{selection}  ·  {tool}  ·  #{:02X}{:02X}{:02X} · {} px · {}  ·  Ctrl+S copies · Esc cancels",
            (r * 255.) as u8,
            (g * 255.) as u8,
            (b * 255.) as u8,
            s.style.width,
            s.style.font
        );
        self.syncing.set(true);
        let font = gtk::pango::FontDescription::from_string(&s.style.font);
        for v in self.views.borrow().iter() {
            v.canvas.queue_draw();
            v.status.set_text(&status);
            v.controls.width.set_value(s.style.width);
            v.controls
                .size
                .set_value(font.size() as f64 / gtk::pango::SCALE as f64);
            if v.controls.text.text().as_str() != s.text {
                v.controls.text.set_text(&s.text);
            }
            // Do not overwrite a partially typed colour while its entry has focus.
            if !v
                .controls
                .color
                .state_flags()
                .contains(gtk::StateFlags::FOCUS_WITHIN)
            {
                v.controls.color.set_text(&format!(
                    "#{:02X}{:02X}{:02X}",
                    (r * 255.) as u8,
                    (g * 255.) as u8,
                    (b * 255.) as u8
                ));
            }
            if let (Some(model), Some(family)) = (v.controls.fonts.model(), font.family()) {
                for i in 0..model.n_items() {
                    if model
                        .item(i)
                        .and_then(|item| item.downcast::<gtk::StringObject>().ok())
                        .is_some_and(|item| item.string() == family)
                    {
                        if v.controls.fonts.selected() != i {
                            v.controls.fonts.set_selected(i);
                        }
                        break;
                    }
                }
            }
            for (tool, button) in &v.controls.tools {
                if *tool == s.tool {
                    button.add_css_class("suggested-action");
                } else {
                    button.remove_css_class("suggested-action");
                }
            }
        }
        self.syncing.set(false);
    }

    fn history(&self, redo: bool) {
        let mut s = self.state.borrow_mut();
        s.anchor = None;
        s.resize = None;
        s.preview = None;
        if redo {
            s.doc.redo();
        } else {
            s.doc.undo();
        }
        drop(s);
        self.refresh();
    }

    fn output(&self) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
        let mut surface =
            render::export(&self.background, &self.desktop, &self.state.borrow().doc)?;
        render::pixbuf(&mut surface)
    }

    fn copy(&self) {
        match self.output() {
            Ok(pixbuf) => {
                if let Some(display) = gdk::Display::default() {
                    display
                        .clipboard()
                        .set_texture(&gdk::Texture::for_pixbuf(&pixbuf));
                    self.close();
                }
            }
            Err(e) => crate::message(&self.app, "Could not copy the screenshot", &e),
        }
    }

    fn save(self: &Rc<Self>) {
        if self.saving.replace(true) {
            return;
        }
        let pixbuf = match self.output() {
            Ok(p) => p,
            Err(e) => {
                self.saving.set(false);
                crate::message(&self.app, "Could not save the screenshot", &e);
                return;
            }
        };
        let dialog = gtk::FileDialog::builder()
            .title("Save screenshot")
            .initial_name(format!(
                "ScreenNotes-{}.png",
                glib::DateTime::now_local()
                    .and_then(|d| d.format("%Y%m%d-%H%M%S"))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|_| "screenshot".into())
            ))
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PNG image"));
        filter.add_mime_type("image/png");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        dialog.set_default_filter(Some(&filter));
        let parent = self.app.active_window();
        let weak = Rc::downgrade(self);
        dialog.save(parent.as_ref(), None::<&gio::Cancellable>, move |result| {
            let Some(e) = weak.upgrade() else {
                return;
            };
            e.saving.set(false);
            match result {
                Ok(file) => {
                    let result = pixbuf.save_to_bufferv("png", &[]).and_then(|bytes| {
                        file.replace_contents(
                            &bytes,
                            None,
                            false,
                            gio::FileCreateFlags::REPLACE_DESTINATION,
                            None::<&gio::Cancellable>,
                        )
                        .map(|_| ())
                    });
                    match result {
                        Ok(()) => e.close(),
                        Err(err) => crate::message(
                            &e.app,
                            "Could not save the screenshot",
                            &err.to_string(),
                        ),
                    }
                }
                Err(err) => {
                    if !err.matches(gtk::DialogError::Dismissed)
                        && !err.matches(gtk::DialogError::Cancelled)
                    {
                        crate::message(&e.app, "Could not save the screenshot", &err.to_string());
                    }
                }
            }
        });
    }

    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some((list, signal)) = self.monitor_signal.borrow_mut().take() {
            list.disconnect(signal);
        }
        for (monitor, signal) in self.geometry_signals.take() {
            monitor.disconnect(signal);
        }
        for v in self.views.take() {
            v.window.destroy();
        }
        self.busy.set(false);
        // Defer dropping the owner's strong reference until signal dispatch finishes.
        let owner = self.owner.clone();
        glib::idle_add_local_once(move || {
            if let Some(owner) = owner.upgrade()
                && owner.borrow().as_ref().is_some_and(|e| e.closed.get())
            {
                owner.borrow_mut().take();
            }
        });
    }
}

fn toggle_toolbar(panel: &gtk::Box, button: &gtk::Button) {
    let at_top = panel.valign() != gtk::Align::Start;
    panel.set_valign(if at_top {
        gtk::Align::Start
    } else {
        gtk::Align::End
    });
    button.set_label(if at_top { "Move down" } else { "Move up" });
    button.set_tooltip_text(Some(if at_top {
        "Move toolbar to bottom (F6)"
    } else {
        "Move toolbar to top (F6)"
    }));
    // Overlay owns the child's position. Invalidate its layout too so painting
    // and pointer hit testing use the new position on the next frame.
    panel.queue_resize();
    if let Some(overlay) = panel.parent() {
        overlay.queue_resize();
        overlay.queue_allocate();
    }
}

pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(".toolbar { background: #20242e; color: #f4f6fa; padding: 12px; border: 1px solid #536078; border-radius: 14px; box-shadow: 0 6px 24px #0008; } .toolbar button { padding: 6px 9px; color: #f4f6fa; background: #343d4d; } .toolbar button:hover { background: #49566c; } .toolbar button.suggested-action { background: #236cb0; color: white; } .toolbar entry { color: #172030; background: #f7f9fc; } .toolbar spinbutton { color: #172030; background: #f7f9fc; } .status { background: #161a23; color: #e6f1ff; border-radius: 8px; padding: 8px 14px; } .error { outline: 2px solid #ff6666; }");
    if let Some(d) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &d,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run explicitly on a real desktop. Uses synthetic pixels, never a screenshot.
    #[test]
    #[ignore = "requires a graphical session and briefly opens fullscreen windows"]
    fn desktop_editor_smoke() {
        gtk::init().expect("GTK display");
        install_css();
        let app = gtk::Application::builder()
            .application_id("io.github.screennotes.Test")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let owner = Rc::new(RefCell::new(None));
        let busy = Rc::new(Cell::new(true));
        let editor = Editor::open(&app, None, busy.clone(), owner.clone()).unwrap();
        *owner.borrow_mut() = Some(editor.clone());
        let context = glib::MainContext::default();
        let until = std::time::Instant::now() + std::time::Duration::from_millis(1200);
        while std::time::Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!editor.views.borrow().is_empty());
        if let Ok(expected) = std::env::var("SCREENNOTES_EXPECT_MONITORS") {
            assert_eq!(
                editor.views.borrow().len(),
                expected.parse::<usize>().unwrap()
            );
        }
        for (i, view) in editor.views.borrow().iter().enumerate() {
            let monitor = editor.desktop.monitors[i];
            eprintln!(
                "Monitor {}: {:?}; canvas {}×{}; fullscreen={}",
                i + 1,
                monitor,
                view.canvas.width(),
                view.canvas.height(),
                view.window.is_fullscreen()
            );
            assert!(view.window.is_fullscreen());
            assert_eq!(view.canvas.width(), monitor.width as i32);
            assert_eq!(view.canvas.height(), monitor.height as i32);
        }
        // Repeated moves must update both visible geometry and pointer targeting.
        for _ in 0..3 {
            for at_top in [true, false] {
                for view in editor.views.borrow().iter() {
                    view.controls.move_bar.emit_clicked();
                }
                let until = std::time::Instant::now() + std::time::Duration::from_millis(100);
                while std::time::Instant::now() < until {
                    while context.pending() {
                        context.iteration(false);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                for view in editor.views.borrow().iter() {
                    let bounds = view.toolbar.compute_bounds(&view.window).unwrap();
                    assert_eq!(bounds.y() < view.window.height() as f32 / 2., at_top);
                    let button = &view.controls.move_bar;
                    let rect = button.compute_bounds(&view.window).unwrap();
                    let picked = view
                        .window
                        .pick(
                            f64::from(rect.x() + rect.width() / 2.),
                            f64::from(rect.y() + rect.height() / 2.),
                            gtk::PickFlags::DEFAULT,
                        )
                        .expect("Move button must remain clickable at its new position");
                    assert!(picked == *button || picked.is_ancestor(button));
                }
            }
        }
        let m = editor.desktop.monitors[0];
        editor.begin(Point {
            x: m.x + 50.,
            y: m.y + 100.,
        });
        editor.end(
            Point {
                x: m.x + 500.,
                y: m.y + 400.,
            },
            true,
        );
        {
            let views = editor.views.borrow();
            views[0].controls.width.set_value(7.);
            views[0].controls.text.set_text("Hello, three screens!");
        }
        editor.begin(Point {
            x: m.x + 80.,
            y: m.y + 150.,
        });
        editor.state.borrow_mut().tool = Tool::Rectangle;
        editor.begin(Point {
            x: m.x + 70.,
            y: m.y + 130.,
        });
        editor.end(
            Point {
                x: m.x + 480.,
                y: m.y + 300.,
            },
            true,
        );
        for view in editor.views.borrow().iter() {
            assert_eq!(view.controls.width.value(), 7.);
            assert_eq!(view.controls.text.text(), "Hello, three screens!");
        }
        editor.history(false);
        editor.history(true);
        assert_eq!(editor.state.borrow().doc.annotations.len(), 2);
        let image = editor.output().unwrap();
        assert_eq!((image.width(), image.height()), (450, 300));
        image
            .savev("/tmp/screennotes-smoke-export.png", "png", &[])
            .unwrap();
        while context.pending() {
            context.iteration(false);
        }
        {
            let views = editor.views.borrow();
            let window = &views[0].window;
            let paintable = gtk::WidgetPaintable::new(Some(window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            if let Some(node) = snapshot.to_node() {
                let surface = window.surface().unwrap();
                let renderer = gtk::gsk::Renderer::for_surface(&surface).unwrap();
                let texture = renderer.render_texture(&node, None);
                texture
                    .save_to_png("/tmp/screennotes-smoke-ui.png")
                    .unwrap();
                renderer.unrealize();
            }
        }
        let original = editor.state.borrow().doc.selection.unwrap();
        // Handles work even with an annotation tool selected; a click alone must
        // not leave a resize active, and resizing must not alter annotations.
        let corner = CropHandle::BottomRight.position(original);
        editor.begin(corner);
        editor.end(corner, false);
        assert!(editor.state.borrow().resize.is_none());
        assert_eq!(editor.state.borrow().doc.selection, Some(original));
        for handle in CropHandle::ALL {
            editor.state.borrow_mut().doc.selection = Some(original);
            let start = handle.position(original);
            let press = Point {
                x: start.x + 2.,
                y: start.y + 2.,
            };
            editor.begin(press);
            editor.update(press);
            assert_eq!(editor.state.borrow().doc.selection, Some(original));
            let end = Point {
                x: press.x + 20.,
                y: press.y + 20.,
            };
            editor.end(end, true);
            assert!(editor.state.borrow().resize.is_none());
            assert_eq!(
                editor.state.borrow().doc.selection,
                Some(handle.resize(
                    original,
                    Point {
                        x: start.x + 20.,
                        y: start.y + 20.
                    },
                    editor.desktop.bounds
                ))
            );
            assert_eq!(editor.state.borrow().doc.annotations.len(), 2);
        }
        editor.close();
        while context.pending() {
            context.iteration(false);
        }
        assert!(!busy.get());
        assert!(owner.borrow().is_none());
    }
}
