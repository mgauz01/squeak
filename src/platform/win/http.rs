//! Minimal HTTPS downloads for model provisioning and updates.

use std::io::{Read, Write};

const USER_AGENT: &str = concat!("Squeak/", env!("CARGO_PKG_VERSION"));
/// Moonshine / HF artifacts can be large; cap at 2 GiB per request.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn get_text(url: &str, max_bytes: u64) -> Result<String, String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|err| format_http_error(url, err))?;

    let (_, body) = response.into_parts();
    let mut reader = body.into_with_config().limit(max_bytes).reader();
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .map_err(|err| err.to_string())?;
    String::from_utf8(buffer).map_err(|err| err.to_string())
}

pub fn stream_url_to_writer(
    url: &str,
    writer: &mut impl Write,
    max_bytes: u64,
    mut on_chunk: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| format_http_error(url, err))?;

    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let (_, body) = response.into_parts();
    let mut reader = body.into_with_config().limit(max_bytes).reader();

    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|e| e.to_string())?;
        downloaded += read as u64;
        on_chunk(downloaded, total);
    }

    Ok(())
}

/// Stream a URL with the default large cap (model downloads).
pub fn stream_url_to_writer_unlimited(
    url: &str,
    writer: &mut impl Write,
    on_chunk: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    stream_url_to_writer(url, writer, MAX_BYTES, on_chunk)
}

fn format_http_error(url: &str, err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(code) => format!("HTTP {code} for {url}"),
        other => format!("{other} for {url}"),
    }
}
