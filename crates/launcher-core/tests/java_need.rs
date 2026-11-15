//! Knowing which Java a version needs, and whether it is already there, before anything is downloaded.

use launcher_core::config::Dirs;
use launcher_core::download::Downloader;
use launcher_core::install::java_need;
use launcher_core::mirror::Source;

fn dl() -> Downloader {
    Downloader::new(Source::Official)
}

fn version(dirs: &Dirs, id: &str, body: &str) {
    let path = dirs.versions().join(id).join(format!("{id}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn runtime(dirs: &Dirs, component: &str) {
    let java = launcher_core::java::runtime_java_path(dirs, component).unwrap();
    std::fs::create_dir_all(java.parent().unwrap()).unwrap();
    std::fs::write(java, b"exe").unwrap();
}

#[tokio::test]
async fn a_version_names_its_component_and_major_and_is_not_installed_yet() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    version(&dirs, "1.21.1", r#"{"id":"1.21.1","mainClass":"m","javaVersion":{"component":"java-runtime-delta","majorVersion":21}}"#);

    let need = java_need(&dl(), &dirs, "1.21.1").await.unwrap();

    assert_eq!((need.component.as_str(), need.major, need.installed), ("java-runtime-delta", 21, false));
}

#[tokio::test]
async fn an_installed_runtime_is_recognised() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    version(&dirs, "1.21.1", r#"{"id":"1.21.1","mainClass":"m","javaVersion":{"component":"java-runtime-delta","majorVersion":21}}"#);
    runtime(&dirs, "java-runtime-delta");

    assert!(java_need(&dl(), &dirs, "1.21.1").await.unwrap().installed);
}

#[tokio::test]
async fn a_version_without_java_info_needs_the_legacy_java_8() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    version(&dirs, "1.12.2", r#"{"id":"1.12.2","mainClass":"m"}"#);

    let need = java_need(&dl(), &dirs, "1.12.2").await.unwrap();

    assert_eq!((need.component.as_str(), need.major), ("jre-legacy", 8));
}

#[tokio::test]
async fn a_loader_profile_inherits_the_java_of_the_game_underneath() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    version(&dirs, "1.20.1", r#"{"id":"1.20.1","mainClass":"m","javaVersion":{"component":"java-runtime-gamma","majorVersion":17}}"#);
    version(&dirs, "fabric-loader-0.15-1.20.1", r#"{"id":"fabric-loader-0.15-1.20.1","inheritsFrom":"1.20.1","mainClass":"k"}"#);

    let need = java_need(&dl(), &dirs, "fabric-loader-0.15-1.20.1").await.unwrap();

    assert_eq!((need.component.as_str(), need.major), ("java-runtime-gamma", 17));
}
