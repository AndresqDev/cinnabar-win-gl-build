use std::path::Path;

use json_ui::Context;

use super::{JAVA_HUD_IMAGES, ScreenRegistry, register};

/// Pack-relative paths of every non-hidden file under `dir`.
fn files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_str().unwrap().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            files(root, &path, out);
        } else {
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            out.push(relative.replace('\\', "/"));
        }
    }
}

/// A texture added to the pack directory without shipping it would draw nothing.
#[test]
fn shipped_images_match_the_java_hud_textures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/java-hud");
    let mut on_disk = Vec::new();
    files(&root, &root.join("textures"), &mut on_disk);
    on_disk.sort();
    let mut shipped: Vec<_> = JAVA_HUD_IMAGES
        .iter()
        .map(|(path, _)| format!("{path}.png"))
        .collect();
    shipped.sort();
    assert_eq!(shipped, on_disk);
}

/// Both offhand frames span the hotbar's height plus the gap beside it.
#[test]
fn offhand_frames_register_at_their_full_size() {
    let mut registry = ScreenRegistry::new(Context::desktop());
    register(&mut registry);
    for side in ["left", "right"] {
        let key = format!("textures/ui/hotbar_offhand_{side}");
        let (_, bytes) = registry
            .builtin_images()
            .iter()
            .find(|(path, _)| *path == key)
            .expect("registered offhand frame");
        let image = image::load_from_memory(bytes).unwrap();
        assert_eq!([image.width(), image.height()], [29, 24]);
    }
}
