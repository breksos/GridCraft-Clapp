use super::Dialog;

/// Help ▸ About GridCraft: the app, its contributors and the AI models that helped. The credits
/// are compiled in (`crate::credits`, docs/contributors.md).
pub(super) fn about(ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in ["About", "Contributors", "Models"] {
            if ui.selectable_label(d.tab == tab, tab).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    match d.tab.as_str() {
        "Contributors" => crate::credits::contributors_ui(ui),
        "Models" => crate::credits::models_ui(ui),
        _ => {
            ui.heading("GridCraft");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(4.0);
            // clapp: a modified version may say, in plain text, that it is based on GridCraft by the
            // ArtCraft team, and must not present itself as an ArtCraft product or send its users to
            // ArtCraft for support (docs/brand/LICENSE-brand.txt).
            ui.label("A clean-room, open-source, Rust-native spreadsheet, as a Clatch app.");
            ui.label("Based on GridCraft by the ArtCraft team.");
            ui.hyperlink_to("github.com/breksos/GridCraft-Clapp", "https://github.com/breksos/GridCraft-Clapp");
            ui.hyperlink_to("Upstream: github.com/storytold/gridcraft", "https://github.com/storytold/gridcraft");
        }
    }
}
