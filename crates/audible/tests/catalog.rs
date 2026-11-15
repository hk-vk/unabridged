//! Catalog search and browse against a mock server.

mod support;

use audible::Asin;
use audible::api::catalog::{self, CatalogQuery, CatalogSort};
use audible::models::CategoryId;
use support::{client, fixture};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn keyword_search_sends_expected_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.0/catalog/products"))
        .and(query_param("keywords", "dune herbert"))
        .and(query_param("products_sort_by", "Relevance"))
        .and(query_param("num_results", "50"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("catalog_search.json")))
        .expect(1)
        .mount(&server)
        .await;
    let query = CatalogQuery {
        page: 2,
        page_size: 500,
        ..CatalogQuery::search("dune herbert")
    };
    let page = catalog::search(&client(&server), &query).await.unwrap();
    assert_eq!(page.products[0].title.as_deref(), Some("Dune"));
    assert_eq!(page.total_results, Some(4231));
}

#[tokio::test]
async fn genre_browse_omits_empty_keywords() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/catalog/products"))
        .and(query_param("category_id", "18580606011"))
        .and(query_param("products_sort_by", "BestSellers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("catalog_search.json")))
        .mount(&server)
        .await;
    let query = CatalogQuery {
        keywords: Some("  ".into()),
        ..CatalogQuery::category(CategoryId("18580606011".into()))
    };
    catalog::search(&client(&server), &query).await.unwrap();
    let req = &server.received_requests().await.unwrap()[0];
    assert!(!req.url.query().unwrap().contains("keywords"));
    assert_eq!(query.sort, CatalogSort::BestSellers);
}

#[tokio::test]
async fn categories_and_product_details() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/catalog/categories"))
        .and(query_param("root", "Genres"))
        .and(query_param("categories_num_levels", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("categories.json")))
        .mount(&server)
        .await;
    Mock::given(path("/1.0/catalog/products/B0CW1QFHBN"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"product":{"asin":"B0CW1QFHBN","title":"Dune","authors":[{"name":"Frank Herbert"}]}}"#,
        ))
        .mount(&server)
        .await;
    let c = client(&server);
    let cats = catalog::categories(&c, "Genres", 2).await.unwrap();
    assert_eq!(cats.len(), 3);
    let p = catalog::product(&c, &Asin::new("B0CW1QFHBN").unwrap())
        .await
        .unwrap();
    assert_eq!(p.author_names(), "Frank Herbert");
}
