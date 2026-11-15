//! Sorting and category filters in content search.

use launcher_core::loader::LoaderKind;
use launcher_core::mods::{target_for, ContentKind, ModrinthClient, SearchOpts};
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn empty() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "hits": [], "total_hits": 0 }))
}

fn opts(sort: &str, category: &str) -> SearchOpts {
    SearchOpts { sort: (!sort.is_empty()).then(|| sort.to_string()), category: (!category.is_empty()).then(|| category.to_string()) }
}

#[tokio::test]
async fn sort_order_and_category_reach_modrinth_for_one_instance() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("index", "downloads"))
        .and(query_param("facets", r#"[["project_type:mod"],["versions:1.21.1"],["categories:fabric"],["categories:optimization"]]"#))
        .respond_with(empty())
        .expect(1)
        .mount(&server)
        .await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    ModrinthClient::new(server.uri()).search_with(&t, "", 0, &opts("downloads", "optimization")).await.unwrap();
}

#[tokio::test]
async fn searching_everything_can_be_sorted_and_filtered_too() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("index", "updated"))
        .and(query_param("facets", r#"[["project_type:shader"],["categories:realistic"]]"#))
        .respond_with(empty())
        .expect(1)
        .mount(&server)
        .await;
    ModrinthClient::new(server.uri()).search_any_with(ContentKind::Shader, "x", 0, &opts("updated", "realistic")).await.unwrap();
}

#[tokio::test]
async fn defaults_change_nothing_and_unknown_values_are_rejected_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/search")).respond_with(empty()).mount(&server).await;
    let client = ModrinthClient::new(server.uri());
    let t = target_for("1.21.1", LoaderKind::Fabric);

    client.search_with(&t, "", 0, &SearchOpts::default()).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert!(!requests[0].url.query().unwrap_or("").contains("index="), "no sort asked for, none sent");

    for bad in [opts("bogus", ""), opts("", "a b"), opts("", "x\"]"), opts("", "../x")] {
        assert!(client.search_with(&t, "", 0, &bad).await.is_err());
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1, "bad values never leave the launcher");
}
