use super::resource_packs::{self, PackApplication};
use client_ui::ui_runtime::UiRuntime;

/// Installs session UI and audio after world and equipment publication have resolved.
pub(super) fn install(
    packs: &mut PackApplication,
    ui_runtime: &mut UiRuntime,
    session_generation: u64,
    session_succeeded: bool,
) {
    resource_packs::install_server_language(
        ui_runtime,
        session_generation,
        packs.server_lang.take(),
        session_succeeded,
    );
    resource_packs::install_session_icons(
        ui_runtime,
        session_generation,
        packs.item_icons.take(),
        packs.item_components.take(),
        session_succeeded,
    );
    resource_packs::install_server_ui(
        ui_runtime,
        session_generation,
        packs.server_ui.take(),
        session_succeeded,
    );
    resource_packs::install_session_glyphs(
        ui_runtime,
        session_generation,
        packs.glyph_sheets.take(),
        session_succeeded,
    );
    client_audio::audio::publish_server_sounds(packs.server_sounds.take());
}
