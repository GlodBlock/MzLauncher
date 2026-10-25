use std::sync::Arc;
use std::time::Duration;

use mzlauncher_core::config::{DownloadConfig, EndpointConfig};
use mzlauncher_core::download::Downloader;
use mzlauncher_core::instance::InstanceService;
use mzlauncher_core::job::JobManager;
use mzlauncher_core::model::{CreateInstanceParams, JobKind, JobStatus, LoaderKind};
use mzlauncher_core::paths::AppPaths;
use mzlauncher_core::store::Store;

#[tokio::test]
#[ignore = "requires live Mojang and Fabric services"]
async fn fabric_profile_can_be_read_and_merged_for_an_instance() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path().join("data"));
    paths.initialize().unwrap();
    let store = Arc::new(Store::open(&paths).unwrap());
    let downloader = Downloader::new(DownloadConfig::default()).unwrap();
    let runtime_root = Arc::new(parking_lot::RwLock::new(paths.runtimes.clone()));
    let instances = InstanceService::new(
        paths,
        store.clone(),
        downloader,
        EndpointConfig::default(),
        runtime_root,
    );
    let instance = instances
        .create(CreateInstanceParams {
            name: "Fabric live".into(),
            description: None,
            game_version: "1.21.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.10".into()),
            java_path: None,
            min_memory_mb: 512,
            max_memory_mb: 4096,
            jvm_args: vec![],
            game_args: vec![],
            account_id: None,
        })
        .unwrap();
    let jobs = JobManager::new(store, 1).unwrap();
    let loader = instances.loaders().clone();
    let version_service = instances.versions().clone();
    let job = jobs
        .submit(JobKind::Install, Some(instance.id), None, move |context| {
            let instance = instance.clone();
            async move {
                let profile = loader.prepare(&instance, &context).await?;
                let merged = version_service.merged_for_instance(&instance).await?;
                let arguments = merged.arguments.as_ref().unwrap();
                assert!(arguments.game.len() > 2);
                assert!(arguments.jvm.len() > 2);
                Ok(serde_json::json!({
                    "profile": profile,
                    "gameArguments": arguments.game.len(),
                    "jvmArguments": arguments.jvm.len(),
                }))
            }
        })
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let snapshot = jobs.get(job.id).unwrap();
        if snapshot.status.is_terminal() {
            assert_eq!(
                snapshot.status,
                JobStatus::Succeeded,
                "{:?}",
                snapshot.error
            );
            println!("Fabric profile merge succeeded: {:?}", snapshot.result);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "live loader test timed out"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
