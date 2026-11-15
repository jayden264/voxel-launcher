//! Forge / NeoForge version lists, parsed from real metadata snapshots.

use launcher_core::download::Downloader;
use launcher_core::forge;
use launcher_core::loader::{self, LoaderKind};
use launcher_core::mirror::Source;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FORGE_XML: &str = include_str!("fixtures/forge-maven-metadata.xml");
const FORGE_PROMOS: &str = include_str!("fixtures/forge-promotions_slim.json");
const NEO_XML: &str = include_str!("fixtures/neoforge-maven-metadata.xml");

fn names(v: &[loader::LoaderVersion]) -> Vec<&str> {
    v.iter().map(|x| x.version.as_str()).collect()
}

#[test]
fn forge_1_21_1_lists_newest_first_with_recommended_in_front() {
    let list = forge::parse_forge_versions(FORGE_XML, FORGE_PROMOS, "1.21.1");
    let n = names(&list);
    assert_eq!(n[0], "52.1.20", "the recommended build leads");
    assert!(n.contains(&"52.1.0") && n.contains(&"52.1.21"));
    assert_eq!(n[1], "52.1.21", "then newest first");
    assert!(list.iter().all(|v| v.stable));
    assert!(n.iter().all(|v| v.starts_with("52.")), "{n:?}");
}

#[test]
fn forge_1_7_10_strips_both_minecraft_parts() {
    let list = forge::parse_forge_versions(FORGE_XML, FORGE_PROMOS, "1.7.10");
    let n = names(&list);
    assert_eq!(n[0], "10.13.4.1614");
    assert!(n.contains(&"10.13.0.1152"));
    assert!(n.iter().all(|v| v.starts_with("10.")), "{n:?}");
}

#[test]
fn forge_unknown_minecraft_version_is_empty() {
    assert!(forge::parse_forge_versions(FORGE_XML, FORGE_PROMOS, "9.9.9").is_empty());
}

#[test]
fn forge_does_not_confuse_prefixes() {
    // `1.21` must not pick up `1.21.1-…` versions.
    let list = forge::parse_forge_versions(FORGE_XML, FORGE_PROMOS, "1.21");
    assert!(list.iter().all(|v| v.version.starts_with("51.")), "{:?}", names(&list));
}

#[test]
fn neoforge_prefix_rules() {
    assert_eq!(forge::neoforge_prefix("1.21.1").as_deref(), Some("21.1."));
    assert_eq!(forge::neoforge_prefix("1.21").as_deref(), Some("21.0."));
    assert_eq!(forge::neoforge_prefix("1.20.2").as_deref(), Some("20.2."));
    assert_eq!(forge::neoforge_prefix("26.3").as_deref(), Some("26.3.0."));
    assert_eq!(forge::neoforge_prefix("26.1.2").as_deref(), Some("26.1.2."));
    assert_eq!(forge::neoforge_prefix("1.20.1"), None, "NeoForge starts at 1.20.2");
    assert_eq!(forge::neoforge_prefix("1.19.4"), None);
    assert_eq!(forge::neoforge_prefix("1.7.10"), None);
    assert_eq!(forge::neoforge_prefix("1.21.1-pre1"), None);
}

#[test]
fn neoforge_1_21_1_lists_newest_first_and_marks_betas() {
    let list = forge::parse_neoforge_versions(NEO_XML, "1.21.1");
    let n = names(&list);
    assert_eq!(n[0], "21.1.266");
    assert!(n.contains(&"21.1.172"));
    assert!(n.iter().all(|v| v.starts_with("21.1.")), "{n:?}");
    assert!(list.iter().all(|v| v.stable == !v.version.contains('-')));

    let betas = forge::parse_neoforge_versions(NEO_XML, "1.20.2");
    assert!(betas.iter().any(|v| !v.stable), "1.20.2 has -beta builds");
    assert!(forge::parse_neoforge_versions(NEO_XML, "1.19.4").is_empty());
    assert!(forge::parse_neoforge_versions(NEO_XML, "1.21").iter().all(|v| v.version.starts_with("21.0.")));
}

#[test]
fn neoforge_26_snapshots_are_not_mixed_into_release_lists() {
    let list = forge::parse_neoforge_versions(NEO_XML, "26.1.2");
    assert!(list.iter().all(|v| v.version.starts_with("26.1.2.")), "{:?}", names(&list));
    assert_eq!(names(&list)[0], "26.1.2.126");
}

#[test]
fn installer_urls_match_known_artifacts() {
    assert_eq!(
        forge::forge_installer_candidates("1.21.1", "52.1.0"),
        vec![
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-installer.jar",
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.21.1-52.1.0-1.21.1/forge-1.21.1-52.1.0-1.21.1-installer.jar",
        ]
    );
    assert_eq!(
        forge::neoforge_installer_url("21.1.172"),
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.172/neoforge-21.1.172-installer.jar"
    );
}

#[tokio::test]
async fn list_versions_fetches_through_the_downloader_and_dispatches_from_loader() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/net/minecraftforge/forge/maven-metadata.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FORGE_XML))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/net/minecraftforge/forge/promotions_slim.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FORGE_PROMOS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/net/neoforged/neoforge/maven-metadata.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(NEO_XML))
        .mount(&server)
        .await;
    let base = server.uri();
    let dl = Downloader::new(Source::Official).with_retry(0, Duration::from_millis(1)).with_resolver(Arc::new(move |u| {
        vec![u
            .replace("https://maven.minecraftforge.net", &base)
            .replace("https://files.minecraftforge.net", &base)
            .replace("https://maven.neoforged.net/releases", &base)]
    }));
    let f = loader::list_loader_versions(&dl, LoaderKind::Forge, "1.21.1").await.unwrap();
    assert_eq!(f[0].version, "52.1.20");
    let n = loader::list_loader_versions(&dl, LoaderKind::NeoForge, "1.21.1").await.unwrap();
    assert_eq!(n[0].version, "21.1.266");
    assert_eq!(loader::default_version(&n).unwrap().version, "21.1.266");
    // Unsupported combination: empty, not an error.
    assert!(loader::list_loader_versions(&dl, LoaderKind::NeoForge, "1.12.2").await.unwrap().is_empty());
}

/// Hits the real sites; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn real_forge_and_neoforge_versions() {
    let dl = Downloader::new(Source::Mirror);
    assert!(!loader::list_loader_versions(&dl, LoaderKind::Forge, "1.21.1").await.unwrap().is_empty());
    assert!(!loader::list_loader_versions(&dl, LoaderKind::NeoForge, "1.21.1").await.unwrap().is_empty());
    assert!(loader::list_loader_versions(&dl, LoaderKind::NeoForge, "1.19.4").await.unwrap().is_empty());
}
