//! Manual verification CLI. Run against your own account:
//!
//! ```sh
//! cargo run --example cli -- login us      # sign in, store credentials in the Keychain
//! cargo run --example cli -- list          # list the library
//! cargo run --example cli -- download B002V5D7RU ~/Audiobooks
//! cargo run --example cli -- position B002V5D7RU
//! cargo run --example cli -- logout        # deregister the device and delete credentials
//! ```
//!
//! Set `RUST_LOG=audible=debug` for request logs (tokens are never logged).

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("this example uses the macOS Keychain");
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "audible=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Err(e) = cli::run(std::env::args().skip(1).collect()).await {
        eprintln!("error: {e}");
        if e.requires_sign_in() {
            eprintln!("hint: run `cargo run --example cli -- login <marketplace>`");
        }
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
mod cli {
    use std::io::{BufRead, Write};
    use std::path::PathBuf;
    use std::sync::Arc;

    use audible::api::library::LibraryQuery;
    use audible::auth::LoginSession;
    use audible::credentials::{CredentialStore, KeychainStore};
    use audible::{
        Asin, Audible, AudibleClient, DownloadEvent, DownloadRequest, Error, Marketplace, Result,
    };

    const PROFILE: &str = "default";

    fn store() -> Arc<dyn CredentialStore> {
        Arc::new(KeychainStore::new())
    }

    fn client() -> Result<Audible> {
        Audible::from_store(store(), PROFILE)?.ok_or(Error::MissingCredential("stored sign-in"))
    }

    fn arg(args: &[String], i: usize, what: &str) -> Result<String> {
        args.get(i)
            .cloned()
            .ok_or_else(|| Error::InvalidArgument(format!("missing {what}")))
    }

    pub async fn run(args: Vec<String>) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("login") => login(args.get(1).map_or("us", String::as_str)).await,
            Some("list") => list().await,
            Some("download") => {
                let asin = Asin::new(arg(&args, 1, "ASIN")?)?;
                let dir = args
                    .get(2)
                    .map_or_else(|| PathBuf::from("."), PathBuf::from);
                download(asin, dir).await
            }
            Some("position") => {
                let asin = Asin::new(arg(&args, 1, "ASIN")?)?;
                let positions = client()?.last_positions(&[asin]).await?;
                for p in positions {
                    println!("{}: {:?} ms", p.asin, p.last_position_heard.position());
                }
                Ok(())
            }
            Some("logout") => {
                client()?.sign_out(true).await?;
                println!("Signed out and deregistered this device.");
                Ok(())
            }
            _ => {
                eprintln!(
                    "usage: cli <login [us|uk|de|...]|list|download ASIN [DIR]|position ASIN|logout>"
                );
                Ok(())
            }
        }
    }

    async fn login(marketplace: &str) -> Result<()> {
        let marketplace: Marketplace = marketplace.parse()?;
        let session = LoginSession::new(marketplace);
        let url = session.authorization_url()?;
        println!("Sign in to Amazon ({marketplace}) in the browser window that opens.");
        println!("When the browser shows a 'page not found' error, copy the full URL");
        println!("from the address bar (it contains openid.oa2.authorization_code)");
        println!("and paste it here.\n\nIf no browser opens, visit:\n{url}\n");
        std::process::Command::new("open")
            .arg(url.as_str())
            .status()
            .ok();

        print!("Redirect URL: ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        let client = Audible::sign_in(&session, line.trim(), store(), PROFILE).await?;
        let creds = client.credentials().await;
        println!(
            "Signed in as {} (device: {}). Credentials saved to the Keychain.",
            creds.customer_name.as_deref().unwrap_or("?"),
            creds.device_name.as_deref().unwrap_or("?")
        );
        Ok(())
    }

    async fn list() -> Result<()> {
        let items = client()?.library(&LibraryQuery::default()).await?;
        for item in &items {
            println!(
                "{}  {:<50}  {:<30}  {:>4} min  {}",
                item.asin,
                truncate(item.title.as_deref().unwrap_or("?"), 50),
                truncate(&item.author_names(), 30),
                item.runtime_length_min.unwrap_or(0),
                item.percent_complete
                    .map(|p| format!("{p:.0}%"))
                    .unwrap_or_default()
            );
        }
        println!("{} titles", items.len());
        Ok(())
    }

    async fn download(asin: Asin, dir: PathBuf) -> Result<()> {
        let mut request = DownloadRequest::new(asin, dir);
        request.progress = Some(Arc::new(|event| match event {
            DownloadEvent::Licensing => eprintln!("requesting license…"),
            DownloadEvent::Downloading(p) | DownloadEvent::Decrypting(p) => {
                let stage = if matches!(event, DownloadEvent::Downloading(_)) {
                    "download"
                } else {
                    "decrypt"
                };
                let pct = p
                    .fraction()
                    .map(|f| format!("{:5.1}%", f * 100.0))
                    .unwrap_or_default();
                eprint!("\r{stage}: {pct} ({} MiB)   ", p.downloaded / (1024 * 1024));
            }
        }));
        let book = client()?.download_book(request).await?;
        eprintln!();
        println!("Saved {}", book.path.display());
        if let Some(chapters) = book.chapters {
            println!("{} chapters", chapters.flatten().len());
        }
        Ok(())
    }

    fn truncate(s: &str, n: usize) -> String {
        if s.chars().count() <= n {
            s.to_owned()
        } else {
            s.chars().take(n - 1).chain(std::iter::once('…')).collect()
        }
    }
}
