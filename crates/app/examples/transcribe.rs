//! Run with `cargo run -p better-audible --example transcribe -- FILE FROM TO LOCALE`.
#[path = "../src/paths.rs"]
#[allow(dead_code)]
mod paths;
#[path = "../src/whisper.rs"]
#[allow(dead_code)]
mod whisper;

mod transcript {
    #[derive(Clone, Debug)]
    /// One streamed recognition segment.
    pub struct Line {
        /// Absolute start in seconds.
        pub start: f64,
        /// Absolute end in seconds.
        pub end: f64,
        /// Recognised text.
        pub text: String,
    }
}

fn main() -> anyhow::Result<()> {
    let result = run();
    // Free the model before exit; ggml's Metal backend asserts otherwise.
    whisper::shutdown();
    result
}

fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 4, "Usage: transcribe FILE FROM TO LOCALE");
    let from: f64 = args[1].parse()?;
    let to: f64 = args[2].parse()?;
    let mut last = -1;
    whisper::download(|progress| {
        let percent = (progress * 100.) as i32;
        if percent / 10 != last {
            eprintln!("Downloading speech model: {percent}%");
            last = percent / 10;
        }
    })?;
    let start = std::time::Instant::now();
    let lines = whisper::transcribe(
        std::path::Path::new(&args[0]),
        from,
        to,
        &args[3],
        &|| false,
        |line| {
            println!("[{:.2} - {:.2}] {}", line.start, line.end, line.text);
        },
    )?;
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(
        "{} lines; {:.2}s audio in {elapsed:.2}s; realtime factor {:.3}",
        lines.len(),
        to - from,
        elapsed / (to - from)
    );
    Ok(())
}
