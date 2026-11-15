//! Listening position sync: `GET /1.0/annotations/lastpositions` and
//! `PUT /1.0/lastpositions/{asin}`.

use serde_json::json;

use crate::error::Result;
use crate::http::HttpClient;
use crate::models::{Asin, LastPositionsResponse, TitlePosition};

/// ASINs per lookup request. The API takes a comma-separated list; the
/// limit is undocumented, so lookups are chunked conservatively.
pub const ASINS_PER_REQUEST: usize = 25;

/// Fetch the last position heard for each ASIN.
pub async fn get(http: &HttpClient, asins: &[Asin]) -> Result<Vec<TitlePosition>> {
    let mut out = Vec::with_capacity(asins.len());
    for chunk in asins.chunks(ASINS_PER_REQUEST) {
        let list = chunk.iter().map(Asin::as_str).collect::<Vec<_>>().join(",");
        let body: LastPositionsResponse = http
            .get_json("/1.0/annotations/lastpositions", &[("asins", list)])
            .await?;
        out.extend(body.asin_last_position_heard_annots);
    }
    Ok(out)
}

/// Write the last position heard. `acr` comes from the title's
/// [`crate::models::ContentReference`] (content metadata or license).
pub async fn set(http: &HttpClient, asin: &Asin, acr: &str, position_ms: u64) -> Result<()> {
    http.put_json(
        &format!("/1.0/lastpositions/{asin}"),
        &json!({ "acr": acr, "asin": asin.as_str(), "position_ms": position_ms }),
    )
    .await
}
