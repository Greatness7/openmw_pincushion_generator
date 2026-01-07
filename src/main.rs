use std::collections::HashMap;
use std::path::Path;

use openmw_config::OpenMWConfiguration;
use vfstool_lib::VFS;

use tes3::esp::*;
use tes3::nif::*;

/// For arrows we offset translation and reduce scale
fn process_arrow(object: &mut NiAVObject) {
    object.translation.y += 8.0;
    object.scale *= 0.5;
}

/// For bolts we just shift them forward slightly
fn process_bolt(object: &mut NiAVObject) {
    object.translation.y += 4.0;
}

/// For throwables we just flip them. (-1 scale)
fn process_throwable(object: &mut NiAVObject) {
    object.scale *= -1.0;
}

/// Insert a new parent node above the previous root node.
///
/// The engine ignores transformations on root nodes, so we must
/// do this before we can transform the original file root node.
///
fn insert_root_parent(stream: &mut NiStream) -> &mut NiNode {
    let mut node = NiNode::default();

    // Make all previous roots children of the new node.
    for root in &stream.roots {
        node.children.push(root.cast());
    }

    // Insert the new node and assign it as the scene root.
    let link = stream.insert(node);
    stream.roots.clear();
    stream.roots.push(link.cast());

    stream.get_mut(link).unwrap()
}

fn process_plugin(vfs: &VFS, plugin_path: &Path) {
    let filter = |tag| tag == *Weapon::TAG;

    let Ok(plugin) = Plugin::from_path_filtered(&plugin_path, filter) else {
        eprintln!("Failed to parse plugin: {plugin_path:?}");
        return;
    };

    // Gather all projectile meshes in the plugin.

    let projectiles: HashMap<_, _> = plugin
        .objects_of_type::<Weapon>()
        .filter_map(|weapon| {
            // Skip spell projectile VFX types.
            if weapon.id.starts_with("VFX_") {
                return None;
            }
            // Skip non-projectile weapon types.
            match weapon.data.weapon_type {
                WeaponType::MarksmanThrown | WeaponType::Arrow | WeaponType::Bolt => {}
                _ => return None,
            }
            // Mesh path as key for de-duplication.
            Some((weapon.mesh.to_lowercase(), weapon))
        })
        .collect();

    // Process each projectile mesh.

    let output_path = Path::new("output");

    for (mesh_path, weapon) in projectiles {
        let with_prefix = format!("meshes/{}", mesh_path);

        let Some(vfs_path) = vfs.get_file(&with_prefix) else {
            eprintln!("File not found in VFS: {mesh_path}");
            continue;
        };

        let abs_path = vfs_path.path();

        let Ok(mut stream) = NiStream::from_path(abs_path) else {
            eprintln!("Failed to open NIF file at path: {abs_path:?}");
            continue;
        };

        if stream.roots.len() != 1 {
            eprintln!("Invalid root node count: {abs_path:?}",);
            continue;
        }

        let root = match stream.objects.get(stream.roots[0].key) {
            Some(NiType::NiNode(node)) => node,
            _ => insert_root_parent(&mut stream),
        };

        for child in root.children.clone() {
            let Some(object) = stream.get_mut(child) else {
                continue;
            };
            match weapon.data.weapon_type {
                WeaponType::MarksmanThrown => {
                    process_throwable(object);
                }
                WeaponType::Arrow => {
                    process_arrow(object);
                }
                WeaponType::Bolt => {
                    process_bolt(object);
                }
                _ => {}
            }
        }

        let output_path = output_path.join(mesh_path);
        std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();
        stream.save_path(&output_path).unwrap();
    }
}

fn main() {
    let config = OpenMWConfiguration::new(None).unwrap();

    let vfs = VFS::from_directories(config.data_directories(), None);

    for file in config.content_files() {
        let path = Path::new(&file);
        if let Some(extension) = path.extension()
            && let bytes = extension.as_encoded_bytes()
            && (bytes.eq_ignore_ascii_case(b"esp")
                || bytes.eq_ignore_ascii_case(b"esm")
                || bytes.eq_ignore_ascii_case(b"omwaddon")
                || bytes.eq_ignore_ascii_case(b"omwgam"))
        {
            if let Some(vfs_file) = vfs.get_file(file) {
                process_plugin(&vfs, vfs_file.path());
            }
        }
    }
}
