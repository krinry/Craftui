use craftui::prelude::*;
use craftui_example_craftpad::mount_craftpad_app;

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "Craftpad — CraftUI Studio & Playground".to_string(),
            width: 1100.0,
            height: 750.0,
            ..Default::default()
        },
        |cx, tree, _app_context| mount_craftpad_app(cx, tree).0,
    )
}
