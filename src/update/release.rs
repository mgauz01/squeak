//! Parse GitHub Releases API responses and compare versions.

use semver::Version;
use serde::Deserialize;
use thiserror::Error;

pub const MSI_ASSET_SUFFIX: &str = "-x64.msi";
pub const MSI_ASSET_PREFIX: &str = "Squeak-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: Version,
    pub msi_url: String,
}

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("failed to parse release JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid release tag: {0}")]
    InvalidTag(String),

    #[error("no Squeak-*-x64.msi asset in release")]
    MissingMsiAsset,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// Strip a leading `v` and parse semver (`v0.2.0` → `0.2.0`).
pub fn parse_release_tag(tag: &str) -> Result<Version, ReleaseError> {
    let trimmed = tag.trim().strip_prefix('v').unwrap_or(tag.trim());
    Version::parse(trimmed).map_err(|_| ReleaseError::InvalidTag(tag.to_string()))
}

pub fn is_msi_asset_name(name: &str) -> bool {
    name.starts_with(MSI_ASSET_PREFIX) && name.ends_with(MSI_ASSET_SUFFIX)
}

pub fn is_newer_than(latest: &Version, current: &str) -> Result<bool, ReleaseError> {
    let current = parse_release_tag(current)?;
    Ok(latest > &current)
}

pub fn parse_latest_release(json: &str) -> Result<AvailableUpdate, ReleaseError> {
    let release: GithubRelease = serde_json::from_str(json)?;
    let version = parse_release_tag(&release.tag_name)?;
    let msi_url = release
        .assets
        .iter()
        .find(|asset| is_msi_asset_name(&asset.name))
        .map(|asset| asset.browser_download_url.clone())
        .ok_or(ReleaseError::MissingMsiAsset)?;
    Ok(AvailableUpdate { version, msi_url })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "tag_name": "v0.2.0",
        "assets": [
            {
                "name": "Squeak-0.2.0-x64.msi",
                "browser_download_url": "https://github.com/mgauz01/squeak/releases/download/v0.2.0/Squeak-0.2.0-x64.msi"
            }
        ]
    }"#;

    #[test]
    fn parses_fixture_and_detects_upgrade() {
        let update = parse_latest_release(FIXTURE).expect("fixture parses");
        assert_eq!(update.version, Version::new(0, 2, 0));
        assert!(update.msi_url.contains("Squeak-0.2.0-x64.msi"));
        assert!(is_newer_than(&update.version, "0.1.0").unwrap());
    }

    #[test]
    fn same_version_is_not_newer() {
        let update = parse_latest_release(FIXTURE).unwrap();
        assert!(!is_newer_than(&update.version, "0.2.0").unwrap());
    }

    #[test]
    fn prerelease_orders_below_stable() {
        let beta = parse_release_tag("v0.2.0-beta").unwrap();
        let stable = parse_release_tag("v0.2.0").unwrap();
        assert!(stable > beta);
        assert!(!is_newer_than(&beta, "0.2.0").unwrap());
    }

    #[test]
    fn missing_msi_asset_errors() {
        let json = r#"{"tag_name":"v0.2.0","assets":[{"name":"notes.txt","browser_download_url":"https://x"}]}"#;
        assert!(matches!(
            parse_latest_release(json),
            Err(ReleaseError::MissingMsiAsset)
        ));
    }

    #[test]
    fn malformed_tag_errors() {
        let json = r#"{"tag_name":"not-a-version","assets":[]}"#;
        assert!(matches!(
            parse_latest_release(json),
            Err(ReleaseError::InvalidTag(_))
        ));
    }
}
