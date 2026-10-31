use chrono::Utc;
use mzlauncher_core::model::{
    InstallStatus, Instance, LoaderKind, ModFile, ModProvider, ReleaseChannel,
};
use mzlauncher_core::mods::compatible;
use uuid::Uuid;

#[test]
fn mod_candidates_require_game_version_and_loader_matches() {
    let instance = Instance {
        id: Uuid::new_v4(),
        name: "test".into(),
        description: None,
        game_version: "1.21.1".into(),
        loader: LoaderKind::Fabric,
        loader_version: Some("0.16.10".into()),
        install_status: InstallStatus::Installed,
        java_path: None,
        min_memory_mb: 512,
        max_memory_mb: 4096,
        jvm_args: vec![],
        game_args: vec![],
        account_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_launched_at: None,
        total_play_time_ms: 0,
    };
    let mut file = ModFile {
        provider: ModProvider::Modrinth,
        project_id: "sodium".into(),
        file_id: "version".into(),
        project_name: "Sodium".into(),
        version_name: "0.6.0".into(),
        file_name: "sodium.jar".into(),
        url: Some("https://example.com/sodium.jar".into()),
        size: None,
        sha1: None,
        sha256: None,
        game_versions: vec!["1.21.1".into()],
        loaders: vec![LoaderKind::Fabric],
        channel: ReleaseChannel::Release,
        dependencies: vec![],
        published_at: None,
        distribution_allowed: true,
    };
    assert!(compatible(&file, &instance));
    file.game_versions = vec!["1.20.1".into()];
    assert!(!compatible(&file, &instance));
    file.game_versions = vec!["1.21.1".into()];
    file.loaders = vec![LoaderKind::Forge];
    assert!(!compatible(&file, &instance));

    file.provider = ModProvider::CurseForge;
    file.loaders.clear();
    assert!(!compatible(&file, &instance));
    let forge_instance = Instance {
        loader: LoaderKind::Forge,
        ..instance
    };
    assert!(compatible(&file, &forge_instance));
    file.provider = ModProvider::Modrinth;
    assert!(!compatible(&file, &forge_instance));
    file.provider = ModProvider::CurseForge;
    file.loaders = vec![LoaderKind::Fabric];
    assert!(!compatible(&file, &forge_instance));
}
