use crate::Event;
use async_channel::Sender;
use ksni::blocking::TrayMethods;

struct Tray {
    tx: Sender<Event>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "screennotes".into()
    }
    fn title(&self) -> String {
        "ScreenNotes".into()
    }
    fn icon_name(&self) -> String {
        "screennotes-symbolic".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Native panel sizes avoid downsampling a single oversized bitmap.
        [16, 22, 24, 32, 48]
            .into_iter()
            .filter_map(symbolic_pixmap)
            .collect()
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.try_send(Event::Capture);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "Capture region…".into(),
                icon_name: "camera-photo".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(Event::Capture);
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: "Help and shortcuts".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(Event::Help);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            ksni::menu::StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.try_send(Event::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn start(tx: Sender<Event>) {
    std::thread::spawn(move || match (Tray { tx: tx.clone() }).spawn() {
        Ok(_handle) => loop {
            std::thread::park();
        },
        Err(e) => {
            let _=tx.send_blocking(Event::Error(format!("Could not show the indicator: {e}. Enable Ubuntu AppIndicators in Extensions. You can still capture with screennotes --capture.")));
        }
    });
}

/// Keep the geometry in sync with assets/screennotes-symbolic.svg.
fn symbolic_pixmap(size: i32) -> Option<ksni::Icon> {
    let mut surface =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, size, size).ok()?;
    {
        let cr = gtk::cairo::Context::new(&surface).ok()?;
        cr.scale(f64::from(size) / 16., f64::from(size) / 16.);
        cr.set_source_rgb(222. / 255., 221. / 255., 218. / 255.);
        cr.set_line_width(2.);
        cr.set_line_cap(gtk::cairo::LineCap::Square);
        cr.set_line_join(gtk::cairo::LineJoin::Miter);
        for points in [
            [(5., 2.), (2., 2.), (2., 5.)],
            [(11., 2.), (14., 2.), (14., 5.)],
            [(14., 11.), (14., 14.), (11., 14.)],
            [(5., 14.), (2., 14.), (2., 11.)],
        ] {
            cr.move_to(points[0].0, points[0].1);
            cr.line_to(points[1].0, points[1].1);
            cr.line_to(points[2].0, points[2].1);
        }
        cr.move_to(6.5, 9.5);
        cr.line_to(9.5, 6.5);
        cr.stroke().ok()?;
    }
    let stride = surface.stride() as usize;
    let pixels = surface.data().ok()?;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size as usize {
        for x in 0..size as usize {
            let offset = y * stride + x * 4;
            let pixel = u32::from_ne_bytes(pixels[offset..offset + 4].try_into().ok()?);
            data.extend_from_slice(&[(pixel >> 24) as u8, 222, 221, 218]);
        }
    }
    Some(ksni::Icon {
        width: size,
        height: size,
        data,
    })
}
