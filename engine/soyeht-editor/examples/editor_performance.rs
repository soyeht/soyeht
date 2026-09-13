use soyeht_editor::{ContentMatcher, SyntaxDocument};
use soyeht_protocol::EditorLanguage;
use std::hint::black_box;
use std::time::Instant;

const SEARCH_BYTES: usize = 1 << 30;
const SEARCH_CHUNK_BYTES: usize = 64 << 20;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = report_path()?;
    let open_ms = measure_open()?;
    let search_ms = measure_search()?;
    let body =
        format!("{{\n  \"open_100k_ms\": {open_ms:.3},\n  \"search_1gb_ms\": {search_ms:.3}\n}}\n");
    std::fs::write(report, body)?;
    Ok(())
}

fn report_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let flag = arguments.next().ok_or("missing --report")?;
    let value = arguments.next().ok_or("missing report path")?;
    if flag != "--report" || arguments.next().is_some() {
        return Err("usage: editor_performance --report PATH".into());
    }
    Ok(value.into())
}

fn measure_open() -> Result<f64, Box<dyn std::error::Error>> {
    let source = "// viewport line\n".repeat(100_000);
    let started = Instant::now();
    let document = SyntaxDocument::new(source, EditorLanguage::Rust)?;
    black_box(document);
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

fn measure_search() -> Result<f64, Box<dyn std::error::Error>> {
    let haystack = "a".repeat(SEARCH_CHUNK_BYTES);
    let matcher = ContentMatcher::new("a needle that is absent", true)?;
    let started = Instant::now();
    for _ in 0..(SEARCH_BYTES / SEARCH_CHUNK_BYTES) {
        let result = matcher.search(black_box(&haystack), None, 500)?;
        if !result.matches.is_empty() {
            return Err("the absent search needle matched".into());
        }
        black_box(result);
    }
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}
