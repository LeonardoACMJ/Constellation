//! Site-icon discovery and portable disk cache. No network or GUI dependencies.
//! Only a site's origin is used: saved URL paths, queries and fragments are not sent.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const IMAGE_LIMIT: usize = 2 * 1024 * 1024;
pub const HTML_LIMIT: usize = 256 * 1024;
const DAY: u64 = 86400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebUrl {
    pub origin: String,
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub resource: String,
}
impl WebUrl {
    pub fn parse(url: &str) -> Option<Self> {
        if url.len() > 8192 || url.chars().any(|c| c.is_control() || c == '\\') {
            return None;
        }
        let (scheme, rest) = url.split_once("://")?;
        let secure = if scheme.eq_ignore_ascii_case("https") {
            true
        } else if scheme.eq_ignore_ascii_case("http") {
            false
        } else {
            return None;
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        if authority.is_empty() || !authority.is_ascii() || authority.contains(['@', '%']) {
            return None;
        }
        let (host, port) = if let Some(ipv6) = authority.strip_prefix('[') {
            let close = ipv6.find(']')?;
            let host = &ipv6[..close];
            host.parse::<std::net::Ipv6Addr>().ok()?;
            (host, &ipv6[close + 1..])
        } else {
            let end = authority.find(':').unwrap_or(authority.len());
            let host = &authority[..end];
            if host.is_empty()
                || !host
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
            {
                return None;
            }
            (host, &authority[end..])
        };
        let port = if port.is_empty() {
            if secure { 443 } else { 80 }
        } else {
            port.strip_prefix(':')?
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)?
        };
        let host = host.to_ascii_lowercase();
        let display_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let suffix = if port == if secure { 443 } else { 80 } {
            String::new()
        } else {
            format!(":{port}")
        };
        let origin = format!(
            "{}://{display_host}{suffix}",
            if secure { "https" } else { "http" }
        );
        let path = rest[end..].split('#').next().unwrap_or("");
        let resource = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Some(Self {
            origin,
            host,
            port,
            secure,
            resource,
        })
    }
}
pub fn origin(url: &str) -> Option<String> {
    Some(WebUrl::parse(url)?.origin)
}

/// Restricted HTTP URL resolution; never resolves file/data/javascript schemes.
pub fn resolve(base: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty()
        || reference.starts_with('#')
        || reference.contains(['\0', '\r', '\n', '\\'])
    {
        return None;
    }
    let base = WebUrl::parse(base)?;
    let result = if reference.starts_with("//") {
        format!("{}:{reference}", if base.secure { "https" } else { "http" })
    } else if reference.split(['/', '?', '#']).next()?.contains(':') {
        reference.to_owned()
    } else {
        let relative = if reference.starts_with('/') {
            reference.to_owned()
        } else if reference.starts_with('?') {
            format!("{}{reference}", base.resource.split('?').next()?)
        } else {
            let directory = base.resource.split('?').next()?.rsplit_once('/')?.0;
            format!("{directory}/{reference}")
        };
        let (path, tail) = relative
            .split_once(['?', '#'])
            .map_or((relative.as_str(), ""), |(p, _)| (p, &relative[p.len()..]));
        let mut parts = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        let trailing = if !parts.is_empty()
            && (path.ends_with('/') || path.ends_with("/.") || path.ends_with("/.."))
        {
            "/"
        } else {
            ""
        };
        format!("{}/{}{trailing}{tail}", base.origin, parts.join("/"))
    };
    let parsed = WebUrl::parse(&result)?;
    if base.secure && !parsed.secure {
        return None;
    }
    Some(format!("{}{}", parsed.origin, parsed.resource))
}

fn entity(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|n| *n <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let code = &rest[1..end];
        let decoded = match code {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            _ => code
                .strip_prefix("#x")
                .or_else(|| code.strip_prefix("#X"))
                .and_then(|n| u32::from_str_radix(n, 16).ok())
                .or_else(|| code.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Small bounded HTML-head scanner. Ignores comments/raw-text elements and
/// handles quoted '>' characters, attribute order, case and HTML entities.
pub fn links(html: &str) -> (Option<String>, Vec<String>) {
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut base = None;
    let mut result = Vec::new();
    let lower = html.to_ascii_lowercase();
    while i < bytes.len() && result.len() < 8 {
        let Some(start) = lower[i..].find('<') else {
            break;
        };
        i += start + 1;
        if lower[i..].starts_with("!--") {
            let Some(end) = lower[i + 3..].find("-->") else {
                break;
            };
            i += 3 + end + 3;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'/') {
            i += 1;
        }
        let tag = &lower[start..i];
        if matches!(tag, "/head" | "body") {
            break;
        }
        if matches!(tag, "script" | "style" | "title" | "textarea") {
            let Some(end) = lower[i..].find(&format!("</{tag}")) else {
                break;
            };
            i += end;
            continue;
        }
        let mut attrs = Vec::new();
        while i < bytes.len() && bytes[i] != b'>' {
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
                i += 1;
            }
            let start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !matches!(bytes[i], b'=' | b'>' | b'/')
            {
                i += 1;
            }
            if start == i {
                if i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                continue;
            }
            let key = &lower[start..i];
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i == bytes.len() || bytes[i] != b'=' {
                continue;
            }
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let quote = bytes.get(i).copied().filter(|c| matches!(c, b'\'' | b'"'));
            if quote.is_some() {
                i += 1;
            }
            let start = i;
            while i < bytes.len()
                && if let Some(q) = quote {
                    bytes[i] != q
                } else {
                    !bytes[i].is_ascii_whitespace() && bytes[i] != b'>'
                }
            {
                i += 1;
            }
            // UTF-8 boundaries are safe: delimiters are ASCII.
            attrs.push((key, entity(&html[start..i])));
            if quote.is_some() && i < bytes.len() {
                i += 1;
            }
        }
        if i < bytes.len() {
            i += 1;
        }
        let get = |name| {
            attrs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.clone())
        };
        if tag == "base" && base.is_none() {
            base = get("href");
        }
        if tag == "link"
            && get("rel").is_some_and(|rel| {
                rel.split_ascii_whitespace().any(|r| {
                    r.eq_ignore_ascii_case("icon") || r.eq_ignore_ascii_case("apple-touch-icon")
                })
            })
        {
            if let Some(href) = get("href").filter(|v| v.len() <= 8192 && !result.contains(v)) {
                result.push(href);
            }
        }
    }
    (base, result)
}

#[derive(Serialize, Deserialize)]
struct Record {
    origin: String,
    checked: u64,
    file: Option<String>,
    success: bool,
}
pub struct DiskCache {
    directory: PathBuf,
    key: String,
    origin: String,
}
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    })
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn read_bounded(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit as u64 + 1)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() <= limit).then_some(data)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let temp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let written = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = written.and_then(|_| crate::storage::replace(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
impl DiskCache {
    pub fn new(root: &Path, origin: &str) -> Self {
        Self {
            directory: root.join("data/cache/favicons"),
            key: format!("{:016x}", hash(origin.as_bytes())),
            origin: origin.to_owned(),
        }
    }
    fn record(&self) -> Option<Record> {
        let record: Record = serde_json::from_slice(&read_bounded(
            &self.directory.join(format!("{}.json", self.key)),
            16384,
        )?)
        .ok()?;
        (record.origin == self.origin).then_some(record)
    }
    pub fn image(&self) -> Option<PathBuf> {
        let file = self.record()?.file?;
        // Metadata is untrusted too; never permit a path outside the cache.
        let (stem, ext) = file.rsplit_once('.')?;
        let digest = stem.strip_prefix(&format!("{}-", self.key))?;
        if digest.len() != 16
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || !["ico", "png", "jpg", "bmp"].contains(&ext)
        {
            return None;
        }
        let path = self.directory.join(file);
        (fs::metadata(&path).ok()?.len() <= IMAGE_LIMIT as u64).then_some(path)
    }
    pub fn fresh(&self) -> bool {
        let now = now();
        self.record().is_some_and(|r| {
            r.checked <= now
                && now - r.checked < if r.success { DAY * 7 } else { 3600 }
                && (!r.success || self.image().is_some())
        })
    }
    fn record_result(&self, file: Option<String>, success: bool) -> bool {
        let record = Record {
            origin: self.origin.clone(),
            checked: now(),
            file,
            success,
        };
        serde_json::to_vec(&record).ok().is_some_and(|bytes| {
            atomic_write(&self.directory.join(format!("{}.json", self.key)), &bytes).is_ok()
        })
    }
    fn store(&self, bytes: &[u8], accept: &mut impl FnMut(&Path) -> bool) -> bool {
        let Some(ext) = image_extension(bytes) else {
            return false;
        };
        let file = format!("{}-{:016x}.{ext}", self.key, hash(bytes));
        let path = self.directory.join(&file);
        let previous = self.image();
        if atomic_write(&path, bytes).is_err() {
            return false;
        }
        if !accept(&path) {
            let _ = fs::remove_file(&path);
            return false;
        }
        if !self.record_result(Some(file), true) {
            return false;
        }
        if let Some(previous) = previous.filter(|p| *p != path) {
            // Only our validated, generated cache filename; never user icons.
            let _ = fs::remove_file(previous);
        }
        true
    }
    /// Called only on the URL worker. Cached pixels are published before this.
    /// Fetch is injectable so tests never access the network.
    pub fn refresh(
        &self,
        mut fetch: impl FnMut(&str, usize) -> Option<Response>,
        mut accept: impl FnMut(&Path) -> bool,
    ) {
        if self.fresh() || fs::create_dir_all(&self.directory).is_err() {
            return;
        }
        if fetch(&format!("{}/favicon.ico", self.origin), IMAGE_LIMIT)
            .is_some_and(|r| self.store(&r.bytes, &mut accept))
        {
            return;
        }
        if let Some(page) = fetch(&format!("{}/", self.origin), HTML_LIMIT) {
            let (base, links) = links(&String::from_utf8_lossy(&page.bytes));
            let base = base
                .and_then(|b| resolve(&page.url, &b))
                .unwrap_or(page.url);
            for link in links.into_iter().take(6) {
                if let Some(url) = resolve(&base, &link) {
                    if fetch(&url, IMAGE_LIMIT).is_some_and(|r| self.store(&r.bytes, &mut accept)) {
                        return;
                    }
                }
            }
        }
        // Keep an earlier good icon offline, but back off after failures.
        self.record_result(self.record().and_then(|r| r.file), false);
    }
}
pub struct Response {
    pub url: String,
    pub bytes: Vec<u8>,
}
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > IMAGE_LIMIT {
        return None;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("ico")
    } else if bytes.starts_with(&[255, 216, 255]) {
        Some("jpg")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_share_cache_without_sending_private_path_or_credentials() {
        assert_eq!(
            origin("HTTPS://Web.WhatsApp.com:443/private?q=secret#x").as_deref(),
            Some("https://web.whatsapp.com")
        );
        assert_eq!(
            origin("http://[::1]:8080/x").as_deref(),
            Some("http://[::1]:8080")
        );
        for value in [
            "file:///secret",
            "https://u:p@host/",
            "https://host\\@other/",
            "https://host:0/",
            "https://%65xample.org/",
            "https://host/\n",
        ] {
            assert!(origin(value).is_none(), "{value}");
        }
    }
    #[test]
    fn resolve_relative_icons_redirects_and_base_without_unsafe_schemes() {
        let base = "https://example.org/sub/page.html";
        assert_eq!(
            resolve(base, "../img/icon.png?a=1&b=2").unwrap(),
            "https://example.org/img/icon.png?a=1&b=2"
        );
        assert_eq!(
            resolve(base, "//cdn.example.org/icon.ico").unwrap(),
            "https://cdn.example.org/icon.ico"
        );
        assert_eq!(
            resolve(base, "?a=2").unwrap(),
            "https://example.org/sub/page.html?a=2"
        );
        let directory = resolve(base, "/assets/").unwrap();
        assert_eq!(
            resolve(&directory, "icon.png").unwrap(),
            "https://example.org/assets/icon.png"
        );
        for r in [
            "data:image/png;base64,x",
            "javascript:alert(1)",
            "file:///x",
            "http://example.org/",
            "https://a:b@host/icon",
            "#x",
        ] {
            assert!(resolve(base, r).is_none());
        }
    }
    #[test]
    fn parses_realistic_head_and_ignores_fake_icons_in_text() {
        let (base, icons) = links(
            r#"<head><!-- <link rel=icon href=bad> --><script>const a='<link rel=icon href=bad>';</script><BASE href='/assets/'><LINK HREF="logo.png?a=1&amp;b=2" data-x='>' REL='SHORTCUT ICON'><link rel=apple-touch-icon href=touch.png><link rel=stylesheet href=style.css></head><link rel=icon href=body.png>"#,
        );
        assert_eq!(base.as_deref(), Some("/assets/"));
        assert_eq!(icons, ["logo.png?a=1&b=2", "touch.png"]);
        assert_eq!(
            links("<link rel=icon href='caf\u{e9}.png'>").1,
            ["caf\u{e9}.png"]
        );
    }
    fn temp() -> PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "constellation-favicon-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn cache_discovery_offline_reuse_and_failed_refresh_keep_good_image() {
        let root = temp();
        let cache = DiskCache::new(&root, "https://example.org");
        let png = b"\x89PNG\r\n\x1a\nfixture";
        let mut requests = Vec::new();
        cache.refresh(
            |url, _| {
                requests.push(url.to_owned());
                Some(Response {
                    url: url.to_owned(),
                    bytes: if url.ends_with("favicon.ico") {
                        b"<html>not an icon</html>".to_vec()
                    } else if url.ends_with("logo.png") {
                        png.to_vec()
                    } else {
                        b"<link href='/logo.png' rel='icon'>".to_vec()
                    },
                })
            },
            |_| true,
        );
        assert_eq!(requests.len(), 3);
        assert!(cache.fresh());
        assert_eq!(fs::read(cache.image().unwrap()).unwrap(), png);
        cache.refresh(|_, _| panic!("Fresh cache must not use network"), |_| true);
        let mut record = cache.record().unwrap();
        record.checked = 0;
        atomic_write(
            &cache.directory.join(format!("{}.json", cache.key)),
            &serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        cache.refresh(|_, _| None, |_| false);
        assert!(cache.fresh());
        assert_eq!(fs::read(cache.image().unwrap()).unwrap(), png);
    }
    #[test]
    fn rejects_oversize_or_undecodable_downloads_and_untrusted_metadata_paths() {
        let cache = DiskCache::new(&temp(), "https://example.org");
        cache.refresh(
            |url, _| {
                Some(Response {
                    url: url.into(),
                    bytes: b"\x89PNG\r\n\x1a\ninvalid".to_vec(),
                })
            },
            |_| false,
        );
        assert!(cache.image().is_none());
        assert!(cache.fresh());
        assert!(image_extension(&vec![0; IMAGE_LIMIT + 1]).is_none());
        cache.record_result(Some("../../config.json".into()), true);
        assert!(cache.image().is_none());
        assert!(!cache.fresh());
    }
}
