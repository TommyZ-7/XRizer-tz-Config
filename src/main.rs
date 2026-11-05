mod app;
mod core;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "XRizer-tz Config",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
