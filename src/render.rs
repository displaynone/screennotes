use crate::model::{Annotation, Desktop, Document, Shape};
use gtk::{cairo, pango};

pub fn annotation(cr: &cairo::Context, a: &Annotation) -> Result<(), cairo::Error> {
    cr.save()?;
    let (r, g, b, alpha) = a.style.color;
    cr.set_source_rgba(r, g, b, alpha);
    cr.set_line_width(a.style.width);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    match &a.shape {
        Shape::Line(a, b) => {
            cr.move_to(a.x, a.y);
            cr.line_to(b.x, b.y);
            cr.stroke()?;
        }
        Shape::Arrow(start, tip) => {
            let dx = tip.x - start.x;
            let dy = tip.y - start.y;
            let length = dx.hypot(dy);
            if length > 0. {
                let (ux, uy) = (dx / length, dy / length);
                // Keep the head proportional to the stroke, but within short arrows.
                let head = (a.style.width * 4.).max(12.).min(length * 0.6);
                let half_width = head * 0.45;
                let (bx, by) = (tip.x - ux * head, tip.y - uy * head);
                cr.move_to(start.x, start.y);
                cr.line_to(bx, by);
                cr.stroke()?;
                cr.move_to(tip.x, tip.y);
                cr.line_to(bx - uy * half_width, by + ux * half_width);
                cr.line_to(bx + uy * half_width, by - ux * half_width);
                cr.close_path();
                cr.fill()?;
            }
        }
        Shape::Rectangle(r) => {
            cr.rectangle(r.x, r.y, r.width, r.height);
            cr.stroke()?;
        }
        Shape::Text(p, text) => {
            let layout = pangocairo::functions::create_layout(cr);
            layout.set_font_description(Some(&pango::FontDescription::from_string(&a.style.font)));
            layout.set_text(text);
            cr.move_to(p.x, p.y);
            pangocairo::functions::show_layout(cr, &layout);
        }
    }
    cr.restore()
}

pub fn export(
    background: &cairo::ImageSurface,
    desktop: &Desktop,
    doc: &Document,
) -> Result<cairo::ImageSurface, String> {
    let selection = doc.selection.ok_or("Select a region before exporting.")?;
    let (x, y, w, h) = desktop
        .pixel_crop(selection)
        .ok_or("The selection is outside the desktop.")?;
    let w = w.min(background.width() - x);
    let h = h.min(background.height() - y);
    if w <= 0 || h <= 0 {
        return Err("The selection is empty.".into());
    }
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    cr.set_source_surface(background, -(x as f64), -(y as f64))
        .map_err(|e| e.to_string())?;
    cr.paint().map_err(|e| e.to_string())?;
    cr.translate(-(x as f64), -(y as f64));
    cr.scale(desktop.scale, desktop.scale);
    cr.translate(-desktop.bounds.x, -desktop.bounds.y);
    for a in &doc.annotations {
        annotation(&cr, a).map_err(|e| e.to_string())?;
    }
    surface.flush();
    Ok(surface)
}

/// Convert native-endian premultiplied Cairo pixels to a GDK texture through a Pixbuf.
pub fn pixbuf(surface: &mut cairo::ImageSurface) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    let (width, height, stride) = (surface.width(), surface.height(), surface.stride() as usize);
    let data = surface.data().map_err(|e| e.to_string())?;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = y * stride + x * 4;
            let pixel = u32::from_ne_bytes(data[i..i + 4].try_into().unwrap());
            let a = (pixel >> 24) as u8;
            let unpremultiply = |c: u32| -> u8 {
                if a == 0 {
                    0
                } else {
                    ((c * 255 + a as u32 / 2) / a as u32).min(255) as u8
                }
            };
            rgba.extend_from_slice(&[
                unpremultiply((pixel >> 16) & 255),
                unpremultiply((pixel >> 8) & 255),
                unpremultiply(pixel & 255),
                a,
            ]);
        }
    }
    Ok(gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &gtk::glib::Bytes::from_owned(rgba),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        width,
        height,
        width * 4,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    #[test]
    fn export_arrow_has_colored_shaft_and_head_in_both_directions() {
        for reverse in [false, true] {
            let bg = cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 100).unwrap();
            let desktop = Desktop::new(
                vec![Rect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 100.,
                }],
                (100, 100),
            )
            .unwrap();
            let mut doc = Document {
                selection: Some(desktop.bounds),
                ..Default::default()
            };
            let (start, tip) = if reverse { (80., 20.) } else { (20., 80.) };
            doc.add(Annotation {
                shape: Shape::Arrow(Point { x: start, y: 50. }, Point { x: tip, y: 50. }),
                style: Style {
                    color: (0., 1., 0., 1.),
                    width: 4.,
                    ..Default::default()
                },
            });
            let mut out = export(&bg, &desktop, &doc).unwrap();
            let stride = out.stride() as usize;
            let pixels = out.data().unwrap();
            let pixel = |x: usize, y: usize| {
                let at = y * stride + x * 4;
                u32::from_ne_bytes(pixels[at..at + 4].try_into().unwrap())
            };
            assert_eq!(pixel(40, 50), 0xff00ff00);
            assert_eq!(pixel(if reverse { 33 } else { 66 }, 54), 0xff00ff00);
            assert_eq!(pixel(40, 54), 0);
            assert_eq!(pixel(if reverse { 16 } else { 84 }, 50), 0);
        }
    }

    #[test]
    fn export_crops_and_renders_in_desktop_coordinates() {
        let bg = cairo::ImageSurface::create(cairo::Format::ARgb32, 300, 100).unwrap();
        {
            let cr = cairo::Context::new(&bg).unwrap();
            cr.set_source_rgb(1., 1., 1.);
            cr.paint().unwrap();
        }
        let desktop = Desktop::new(
            vec![Rect {
                x: -100.,
                y: 0.,
                width: 300.,
                height: 100.,
            }],
            (300, 100),
        )
        .unwrap();
        let mut doc = Document {
            selection: Some(Rect {
                x: 0.,
                y: 0.,
                width: 20.,
                height: 20.,
            }),
            ..Default::default()
        };
        doc.add(Annotation {
            shape: Shape::Line(Point { x: 0., y: 10. }, Point { x: 20., y: 10. }),
            style: Style {
                color: (1., 0., 0., 1.),
                width: 4.,
                ..Default::default()
            },
        });
        let mut out = export(&bg, &desktop, &doc).unwrap();
        assert_eq!((out.width(), out.height()), (20, 20));
        let stride = out.stride() as usize;
        let pixels = out.data().unwrap();
        let at = 10 * stride + 10 * 4;
        assert_eq!(
            u32::from_ne_bytes(pixels[at..at + 4].try_into().unwrap()),
            0xffff0000
        );
    }
}
