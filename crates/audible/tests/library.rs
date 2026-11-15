//! Library pagination and position sync against a mock server.

mod support;

use audible::Asin;
use audible::api::{library, position};
use support::{client, fixture};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn pages_until_total_count_reached() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.0/library"))
        .and(query_param("page", "1"))
        .and(query_param("num_results", "2"))
        .and(query_param("sort_by", "-PurchaseDate"))
        .and(query_param("image_sizes", "500,1215"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Total-Count", "3")
                .set_body_string(fixture("library_page1.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/1.0/library"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Total-Count", "3")
                .set_body_string(fixture("library_page2.json")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let query = library::LibraryQuery {
        page_size: 2,
        ..Default::default()
    };
    let mut pages = Vec::new();
    let items = library::all(&client(&server), &query, |p| {
        pages.push((p.page, p.items.len()))
    })
    .await
    .unwrap();
    assert_eq!(pages, [(1, 2), (2, 1)]);
    let asins: Vec<&str> = items.iter().map(|i| i.asin.as_str()).collect();
    assert_eq!(asins, ["B002V5D7RU", "B07PWF7VKB", "B08G9PRS1K"]);
}

#[tokio::test]
async fn pages_until_short_page_without_total_count() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/library"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("library_page1.json")))
        .mount(&server)
        .await;
    Mock::given(path("/1.0/library"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"items":[]}"#))
        .mount(&server)
        .await;
    let query = library::LibraryQuery {
        page_size: 2,
        ..Default::default()
    };
    let items = library::all(&client(&server), &query, |_| {})
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn transient_failure_mid_pagination_is_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/library"))
        .and(query_param("page", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Total-Count", "3")
                .set_body_string(fixture("library_page1.json")),
        )
        .mount(&server)
        .await;
    Mock::given(path("/1.0/library"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path("/1.0/library"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Total-Count", "3")
                .set_body_string(fixture("library_page2.json")),
        )
        .mount(&server)
        .await;
    let query = library::LibraryQuery {
        page_size: 2,
        ..Default::default()
    };
    let items = library::all(&client(&server), &query, |_| {})
        .await
        .unwrap();
    assert_eq!(items.len(), 3);
}

#[tokio::test]
async fn fetches_single_item() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/library/B002V5D7RU"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("library_item.json")))
        .mount(&server)
        .await;
    let item = library::item(&client(&server), &Asin::new("B002V5D7RU").unwrap(), &[])
        .await
        .unwrap();
    assert!(
        item.publisher_summary
            .as_deref()
            .unwrap_or_default()
            .contains("prelude")
    );
}

#[tokio::test]
async fn reads_and_writes_positions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.0/annotations/lastpositions"))
        .and(query_param("asins", "B002V5D7RU,B07PWF7VKB"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("lastpositions.json")))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/1.0/lastpositions/B002V5D7RU"))
        .and(body_json(
            serde_json::json!({"acr": "CR!ACR", "asin": "B002V5D7RU", "position_ms": 42_000}),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server);
    let asins = [
        Asin::new("B002V5D7RU").unwrap(),
        Asin::new("B07PWF7VKB").unwrap(),
    ];
    let positions = position::get(&c, &asins).await.unwrap();
    assert_eq!(positions[0].last_position_heard.position(), Some(1_234_567));
    position::set(&c, &asins[0], "CR!ACR", 42_000)
        .await
        .unwrap();
}

#[tokio::test]
async fn position_lookups_are_chunked() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/annotations/lastpositions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"asin_last_position_heard_annots":[]}"#),
        )
        .expect(2)
        .mount(&server)
        .await;
    let asins: Vec<Asin> = (0..30)
        .map(|i| Asin::new(format!("B0{i:08}")).unwrap())
        .collect();
    position::get(&client(&server), &asins).await.unwrap();
}
