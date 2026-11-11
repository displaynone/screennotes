use std::process::Command;

/// Read-only diagnostics: never grant permissions or restart desktop services.
pub fn print_report() {
    println!(
        "ScreenNotes {} — desktop portal diagnostics",
        env!("CARGO_PKG_VERSION")
    );
    for name in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "DESKTOP_SESSION",
        "WAYLAND_DISPLAY",
    ] {
        println!(
            "{name}={}",
            std::env::var(name).unwrap_or_else(|_| "(unset)".into())
        );
    }
    if let Ok(os) = std::fs::read_to_string("/etc/os-release") {
        println!("{os}");
    }
    let checks: &[(&str, &str, &[&str])] = &[
        (
            "Portal package versions",
            "dpkg-query",
            &[
                "-W",
                "xdg-desktop-portal",
                "xdg-desktop-portal-gnome",
                "xdg-desktop-portal-gtk",
                "xdg-desktop-portal-kde",
            ],
        ),
        (
            "Screenshot permissions (an absent entry means no saved decision)",
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.freedesktop.impl.portal.PermissionStore",
                "--object-path",
                "/org/freedesktop/impl/portal/PermissionStore",
                "--method",
                "org.freedesktop.impl.portal.PermissionStore.Lookup",
                "screenshot",
                "screenshot",
            ],
        ),
        (
            "Screenshot portal version",
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                "org.freedesktop.DBus.Properties.Get",
                "org.freedesktop.portal.Screenshot",
                "version",
            ],
        ),
        (
            "Recent portal logs (review before sharing)",
            "journalctl",
            &[
                "--user",
                "--no-pager",
                "--since",
                "10 minutes ago",
                "-n",
                "60",
                "-u",
                "xdg-desktop-portal.service",
                "-u",
                "xdg-desktop-portal-gnome.service",
                "-u",
                "xdg-desktop-portal-gtk.service",
            ],
        ),
    ];
    for (label, command, args) in checks {
        println!("\n--- {label} ---");
        match Command::new("timeout")
            .args(["10s", command])
            .args(*args)
            .output()
        {
            Ok(output) => {
                print!("{}", String::from_utf8_lossy(&output.stdout));
                print!("{}", String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    println!("Check exited with {}", output.status);
                }
            }
            Err(error) => println!("Could not run diagnostic: {error}"),
        }
    }
}
