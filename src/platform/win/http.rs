//! Minimal HTTPS downloads for model provisioning.

use std::io::{Read, Write};

const USER_AGENT: &str = "Squeak/0.1";
/// Moonshine / HF artifacts can be large; cap at 2 GiB per request.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn stream_url_to_writer(
    url: &str,
    writer: &mut impl Write,
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
    let mut reader = body.into_with_config().limit(MAX_BYTES).reader();

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

fn format_http_error(url: &str, err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(code) => format!("HTTP {code} for {url}"),
        other => format!("{other} for {url}"),
    }
}
