//! Deserialisation tests against recorded-style JSON fixtures.

use audible::models::{CatalogPage, Category, ChapterInfo, LibraryItem, TitlePosition};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn library_items_tolerate_nulls_and_missing_groups() {
    let page = fixture("library_page1.json");
    let items: Vec<LibraryItem> = serde_json::from_value(page["items"].clone()).unwrap();
    assert_eq!(items.len(), 2);

    let hobbit = &items[0];
    assert_eq!(hobbit.asin.as_str(), "B002V5D7RU");
    assert_eq!(hobbit.author_names(), "J. R. R. Tolkien");
    assert!(hobbit.series.is_empty(), "null series becomes empty");
    assert_eq!(hobbit.available_codecs.len(), 2);
    assert_eq!(
        hobbit.cover_url(600),
        Some("https://m.media-amazon.com/images/I/51aqHgPpaQL._SL1215_.jpg")
    );
    assert_eq!(
        hobbit
            .listening_status
            .as_ref()
            .and_then(|s| s.time_remaining_seconds),
        Some(24825)
    );
    assert!(hobbit.is_downloadable());

    let hail_mary = &items[1];
    assert!(
        hail_mary.available_codecs.is_empty(),
        "null codecs become empty"
    );
    assert_eq!(hail_mary.is_finished, Some(true));
    assert_eq!(
        hail_mary.cover_url(2000),
        Some("https://m.media-amazon.com/images/I/51b6UNQX8ML._SL500_.jpg"),
        "falls back to the largest cover"
    );
}

#[test]
fn series_and_multiple_narrators() {
    let page = fixture("library_page2.json");
    let items: Vec<LibraryItem> = serde_json::from_value(page["items"].clone()).unwrap();
    let book = &items[0];
    assert_eq!(book.narrator_names(), "Michael Kramer, Kate Reading");
    assert_eq!(book.series[0].sequence.as_deref(), Some("1"));
}

#[test]
fn chapter_tree_flattens_depth_first() {
    let meta = fixture("content_metadata.json");
    let info: ChapterInfo =
        serde_json::from_value(meta["content_metadata"]["chapter_info"].clone()).unwrap();
    let titles: Vec<&str> = info.flatten().iter().map(|c| c.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Opening Credits",
            "Part One",
            "Chapter 1: An Unexpected Party",
            "Chapter 2: Roast Mutton",
            "End Credits"
        ]
    );
    assert_eq!(info.brand_intro_duration_ms, Some(2043));
}

#[test]
fn positions_distinguish_missing() {
    let body = fixture("lastpositions.json");
    let positions: Vec<TitlePosition> =
        serde_json::from_value(body["asin_last_position_heard_annots"].clone()).unwrap();
    assert_eq!(positions[0].last_position_heard.position(), Some(1_234_567));
    assert_eq!(positions[1].last_position_heard.position(), None);
}

#[test]
fn catalog_and_categories() {
    let page: CatalogPage = serde_json::from_value(fixture("catalog_search.json")).unwrap();
    assert_eq!(page.total_results, Some(4231));
    assert_eq!(page.products[0].is_ayce, Some(true));

    let body = fixture("categories.json");
    let cats: Vec<Category> = serde_json::from_value(body["categories"].clone()).unwrap();
    assert_eq!(cats[0].children[0].name, "Classics");
    assert!(cats[1].children.is_empty());
}
