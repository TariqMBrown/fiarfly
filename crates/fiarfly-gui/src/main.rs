//! FIARfly desktop GUI entry point.

use std::panic;

fn main() -> eframe::Result {
    // Enable full backtraces so the crash handler captures useful stack frames
    // even when the user hasn't set RUST_BACKTRACE in their shell.
    if std::env::var("RUST_BACKTRACE").is_err() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "full"); }
    }
    env_logger::init();
    install_crash_handler();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FIARfly")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "FIARfly",
        options,
        Box::new(|cc| Ok(Box::new(app::FiarflyApp::new(cc)))),
    )
}

/// Install a panic hook that writes a timestamped crash report to
/// ~/Documents/FIARfly Crash Reports/ before the process exits.
fn install_crash_handler() {
    let default_hook = panic::take_hook();

    panic::set_hook(Box::new(move |info| {
        // Build the report text.
        let timestamp = {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| {
                    let s = d.as_secs();
                    format!(
                        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
                        1970 + s / 31_557_600,
                        (s % 31_557_600) / 2_629_800 + 1,
                        (s % 2_629_800)  / 86400     + 1,
                        (s % 86400)      / 3600,
                        (s % 3600)       / 60,
                        s % 60,
                    )
                })
                .unwrap_or_else(|_| "unknown_time".into())
        };

        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".into());

        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".into()
        };

        // Capture backtrace.
        let backtrace = std::backtrace::Backtrace::capture();

        let report = format!(
            "FIARfly crash report — {timestamp}\n\
             ═══════════════════════════════════════════════════\n\
             Message  : {message}\n\
             Location : {location}\n\
             Version  : {} {}\n\
             OS       : {}\n\
             ───────────────────────────────────────────────────\n\
             Backtrace:\n{backtrace}\n",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
        );

        // Write to ~/Documents/FIARfly Crash Reports/<timestamp>.log
        let crash_dir = dirs_next::document_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("FIARfly Crash Reports");

        let wrote_file = std::fs::create_dir_all(&crash_dir)
            .ok()
            .and_then(|_| {
                let path = crash_dir.join(format!("crash_{timestamp}.log"));
                std::fs::write(&path, &report).ok().map(|_| path)
            });

        // Also print to stderr so it appears in terminal / system console.
        eprintln!("\n{report}");

        if let Some(path) = wrote_file {
            eprintln!("Crash report written to: {}", path.display());
        } else {
            eprintln!("(Could not write crash report to disk — check permissions on ~/Documents)");
        }

        // Run the default hook last so the OS still prints the normal
        // "thread 'main' panicked" line and any existing RUST_BACKTRACE output.
        default_hook(info);
    }));
}

mod app;
mod colormap;
mod panels;
mod state;
