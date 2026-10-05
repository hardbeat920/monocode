//! The `latest.json` feed. Adapted from tauri-plugin-updater 2.10.1
//! (`src/updater.rs`, Apache-2.0 OR MIT), so the native app reads the feed the
//! release workflow already publishes.
//!
//! The static shape lists packages per platform:
//!
//! ```json
//! { "version": "0.7.0", "notes": "...", "pub_date": "2026-10-02T12:34:31Z",
//!   "platforms": { "darwin-aarch64": { "url": "...", "signature": "..." } } }
//! ```
//!
//! The dynamic shape has one `url` and `signature` at the top level.

use std::collections::HashMap;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};
use time::OffsetDateTime;
use url::Url;

use crate::error::{Error, Result};

/// One package: where to download it and its minisign signature.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ReleaseManifestPlatform {
    pub url: Url,
    /// Base64 of the `.sig` file's text.
    pub signature: String,
}

/// The two feed shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteReleaseInner {
    Dynamic(ReleaseManifestPlatform),
    Static {
        platforms: HashMap<String, ReleaseManifestPlatform>,
    },
}

/// A release the feed announced.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteRelease {
    pub version: Version,
    pub notes: Option<String>,
    pub pub_date: Option<OffsetDateTime>,
    pub data: RemoteReleaseInner,
}

impl RemoteRelease {
    /// The package URL for `target`.
    pub fn download_url(&self, target: &str) -> Result<&Url> {
        match &self.data {
            RemoteReleaseInner::Dynamic(platform) => Ok(&platform.url),
            RemoteReleaseInner::Static { platforms } => platforms
                .get(target)
                .map(|platform| &platform.url)
                .ok_or_else(|| Error::TargetNotFound(target.to_string())),
        }
    }

    /// The package signature for `target`.
    pub fn signature(&self, target: &str) -> Result<&String> {
        match &self.data {
            RemoteReleaseInner::Dynamic(platform) => Ok(&platform.signature),
            RemoteReleaseInner::Static { platforms } => platforms
                .get(target)
                .map(|platform| &platform.signature)
                .ok_or_else(|| Error::TargetNotFound(target.to_string())),
        }
    }
}

impl<'de> Deserialize<'de> for RemoteRelease {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct InnerRemoteRelease {
            #[serde(alias = "name", deserialize_with = "parse_version")]
            version: Version,
            notes: Option<String>,
            pub_date: Option<String>,
            platforms: Option<HashMap<String, ReleaseManifestPlatform>>,
            url: Option<Url>,
            signature: Option<String>,
        }

        let release = InnerRemoteRelease::deserialize(deserializer)?;

        let pub_date = match release.pub_date {
            Some(date) => Some(
                OffsetDateTime::parse(&date, &time::format_description::well_known::Rfc3339)
                    .map_err(|e| DeError::custom(format!("invalid value for `pub_date`: {e}")))?,
            ),
            None => None,
        };

        Ok(RemoteRelease {
            version: release.version,
            notes: release.notes,
            pub_date,
            data: match release.platforms {
                Some(platforms) => RemoteReleaseInner::Static { platforms },
                None => RemoteReleaseInner::Dynamic(ReleaseManifestPlatform {
                    url: release.url.ok_or_else(|| {
                        DeError::custom("the `url` field was not set on the updater response")
                    })?,
                    signature: release.signature.ok_or_else(|| {
                        DeError::custom("the `signature` field was not set on the updater response")
                    })?,
                }),
            },
        })
    }
}

fn parse_version<'de, D>(deserializer: D) -> std::result::Result<Version, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    Version::from_str(text.trim_start_matches('v')).map_err(DeError::custom)
}

/// Fills the endpoint placeholders. `url::Url` percent-encodes braces in the
/// path but not in the query, so both spellings are replaced.
pub fn expand_endpoint(
    endpoint: &Url,
    current_version: &Version,
    target: &str,
    arch: &str,
    bundle_type: &str,
) -> Result<Url> {
    let encoded_version = encode_version(&current_version.to_string());
    let url = endpoint
        .to_string()
        .replace("%7B%7Bcurrent_version%7D%7D", &encoded_version)
        .replace("%7B%7Btarget%7D%7D", target)
        .replace("%7B%7Barch%7D%7D", arch)
        .replace("%7B%7Bbundle_type%7D%7D", bundle_type)
        .replace("{{current_version}}", &encoded_version)
        .replace("{{target}}", target)
        .replace("{{arch}}", arch)
        .replace("{{bundle_type}}", bundle_type);
    Ok(url.parse()?)
}

/// Percent-encodes control characters and `+`, the set the plugin used, so a
/// build suffix like `1.0.0+abc` survives the query string.
fn encode_version(version: &str) -> String {
    let mut out = String::with_capacity(version.len());
    for byte in version.bytes() {
        if byte.is_ascii_control() || byte == b'+' || !byte.is_ascii() {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push(byte as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATIC_FEED: &str = r#"{
      "version": "0.7.0",
      "notes": "MonoCode 0.7.0",
      "pub_date": "2026-10-02T12:34:31.123Z",
      "platforms": {
        "darwin-aarch64": { "signature": "c2ln", "url": "https://cdn.example/releases/0.7.0/MonoCode.app.tar.gz" },
        "darwin-x86_64": { "signature": "c2ln", "url": "https://cdn.example/releases/0.7.0/MonoCode_x64.app.tar.gz" },
        "windows-x86_64": { "signature": "c2ln", "url": "https://cdn.example/releases/0.7.0/MonoCode_0.7.0_x64-setup.exe" }
      }
    }"#;

    #[test]
    fn parses_the_published_static_feed() {
        let release: RemoteRelease = serde_json::from_str(STATIC_FEED).unwrap();
        assert_eq!(release.version, Version::new(0, 7, 0));
        assert_eq!(release.notes.as_deref(), Some("MonoCode 0.7.0"));
        assert!(release.pub_date.is_some());
        assert_eq!(
            release.download_url("darwin-x86_64").unwrap().as_str(),
            "https://cdn.example/releases/0.7.0/MonoCode_x64.app.tar.gz"
        );
        assert!(matches!(
            release.download_url("linux-x86_64"),
            Err(Error::TargetNotFound(target)) if target == "linux-x86_64"
        ));
    }

    #[test]
    fn parses_the_dynamic_shape_and_a_v_prefix() {
        let release: RemoteRelease = serde_json::from_str(
            r#"{ "name": "v1.2.3", "url": "https://cdn.example/a.tar.gz", "signature": "c2ln" }"#,
        )
        .unwrap();
        assert_eq!(release.version, Version::new(1, 2, 3));
        assert_eq!(release.signature("anything").unwrap(), "c2ln");
    }

    #[test]
    fn rejects_a_dynamic_release_without_url() {
        let err = serde_json::from_str::<RemoteRelease>(r#"{ "version": "1.0.0" }"#).unwrap_err();
        assert!(err.to_string().contains("the `url` field was not set"));
    }

    #[test]
    fn rejects_a_bad_pub_date() {
        let err = serde_json::from_str::<RemoteRelease>(
            r#"{ "version": "1.0.0", "pub_date": "yesterday", "platforms": {} }"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid value for `pub_date`"));
    }

    #[test]
    fn expands_placeholders_in_path_and_query() {
        let endpoint: Url =
            "https://cdn.example/{{target}}/{{arch}}/latest.json?v={{current_version}}&b={{bundle_type}}"
                .parse()
                .unwrap();
        let version = Version::parse("1.0.0+build.5").unwrap();
        let url = expand_endpoint(&endpoint, &version, "darwin", "aarch64", "app").unwrap();
        assert_eq!(
            url.as_str(),
            "https://cdn.example/darwin/aarch64/latest.json?v=1.0.0%2Bbuild.5&b=app"
        );
    }
}
