//! Resumable downloads against a mock server.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use audible::download::{Downloader, Progress};
use audible::http::RetryPolicy;
use reqwest::header::HeaderMap;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn data() -> Vec<u8> {
    (0..200u32).map(|i| (i * 7 % 251) as u8).collect()
}

fn downloader() -> Downloader {
    Downloader::new(reqwest::Client::new()).with_retry(RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(20),
    })
}

fn no_range(req: &Request) -> bool {
    !req.headers.contains_key("range")
}

#[tokio::test]
async fn fresh_download_reports_progress_and_renames() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/book.aaxc"))
        .and(header("user-agent", audible::download::DOWNLOAD_USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("nested/book.aaxc");
    let seen: Arc<Mutex<Vec<Progress>>> = Arc::default();
    let sink = seen.clone();
    let size = downloader()
        .download(
            &format!("{}/book.aaxc", server.uri()),
            &HeaderMap::new(),
            &dest,
            Some(Arc::new(move |p| sink.lock().unwrap().push(p))),
        )
        .await
        .unwrap();
    assert_eq!(size, 200);
    assert_eq!(std::fs::read(&dest).unwrap(), data());
    assert!(!Downloader::part_path(&dest).exists());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.first().unwrap().downloaded, 0);
    assert_eq!(
        *seen.last().unwrap(),
        Progress {
            downloaded: 200,
            total: Some(200)
        }
    );
}

#[tokio::test]
async fn resumes_from_partial_file() {
    let server = MockServer::start().await;
    Mock::given(path("/book.aaxc"))
        .and(header("range", "bytes=60-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 60-199/200")
                .set_body_bytes(data()[60..].to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("book.aaxc");
    std::fs::write(Downloader::part_path(&dest), &data()[..60]).unwrap();
    downloader()
        .download(
            &format!("{}/book.aaxc", server.uri()),
            &HeaderMap::new(),
            &dest,
            None,
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data());
}

#[tokio::test]
async fn restarts_when_server_ignores_range() {
    let server = MockServer::start().await;
    Mock::given(path("/book.aaxc"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("book.aaxc");
    std::fs::write(
        Downloader::part_path(&dest),
        b"stale bytes from another file",
    )
    .unwrap();
    downloader()
        .download(
            &format!("{}/book.aaxc", server.uri()),
            &HeaderMap::new(),
            &dest,
            None,
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data());
}

#[tokio::test]
async fn already_complete_part_file_finishes_on_416() {
    let server = MockServer::start().await;
    Mock::given(path("/book.aaxc"))
        .and(header("range", "bytes=200-"))
        .respond_with(ResponseTemplate::new(416).insert_header("content-range", "bytes */200"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("book.aaxc");
    std::fs::write(Downloader::part_path(&dest), data()).unwrap();
    let size = downloader()
        .download(
            &format!("{}/book.aaxc", server.uri()),
            &HeaderMap::new(),
            &dest,
            None,
        )
        .await
        .unwrap();
    assert_eq!(size, 200);
    assert!(dest.exists());
}

#[tokio::test]
async fn truncated_transfer_is_resumed() {
    let server = MockServer::start().await;
    // First response promises 200 bytes but delivers 100.
    Mock::given(path("/book.aaxc"))
        .and(no_range)
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 0-199/200")
                .set_body_bytes(data()[..100].to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(path("/book.aaxc"))
        .and(header("range", "bytes=100-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 100-199/200")
                .set_body_bytes(data()[100..].to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("book.aaxc");
    downloader()
        .download(
            &format!("{}/book.aaxc", server.uri()),
            &HeaderMap::new(),
            &dest,
            None,
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data());
}

#[tokio::test]
async fn retries_server_errors_then_gives_up() {
    let server = MockServer::start().await;
    Mock::given(path("/flaky"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path("/flaky"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data()))
        .mount(&server)
        .await;
    Mock::given(path("/gone"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let d = downloader();
    d.download(
        &format!("{}/flaky", server.uri()),
        &HeaderMap::new(),
        &dir.path().join("a"),
        None,
    )
    .await
    .unwrap();
    let err = d
        .download(
            &format!("{}/gone", server.uri()),
            &HeaderMap::new(),
            &dir.path().join("b"),
            None,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, audible::Error::Unauthorized { status: 403, .. }),
        "{err:?}"
    );
}
