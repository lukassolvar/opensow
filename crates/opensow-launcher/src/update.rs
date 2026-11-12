//! Updates from GitHub releases (docs/launcher.md, "Updates"). The launcher asks the public API for the latest release of
//! [`REPO`], compares its tag (`vX.Y.Z`) with its own version, downloads this platform's archive (`opensow-<platform>.zip` or
//! `.tar.gz`, see [`platform`]) and its `.sha256`, checks the hash and puts the new `opensow-launcher` and `opensow-game` next to
//! itself. Network and unpacking go through the system's `curl`, `tar` and `unzip` (macOS, Windows 10+ and Linux have them), so
//! no HTTP library is built in.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository the releases come from; `OPENSOW_UPDATE_REPO=owner/name` points elsewhere (forks, tests).
pub const REPO: &str = "lukassolvar/opensow";

/// This launcher's version (the release tag without its `v`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The programs a release archive brings (with `.exe` on Windows).
pub const PROGRAMS: [&str; 2] = ["opensow-launcher", "opensow-game"];

pub fn repo() -> String {
    std::env::var("OPENSOW_UPDATE_REPO").ok().filter(|r| r.contains('/')).unwrap_or_else(|| REPO.into())
}

/// The part of an archive's name that says which system it is for: `macos-aarch64`, `macos-x86_64`, `windows-x86_64`,
/// `linux-x86_64`, `linux-aarch64`, ...
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// `v1.2.3` / `1.2.3` -> (1, 2, 3); a pre-release (`1.2.3-beta`) or anything else -> `None`.
pub fn parse_version(tag: &str) -> Option<(u32, u32, u32)> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    let mut it = t.split('.').map(|p| p.parse::<u32>().ok());
    let v = (it.next()??, it.next()??, it.next().unwrap_or(Some(0))?);
    it.next().is_none().then_some(v)
}

/// A file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// The latest release as the launcher needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: (u32, u32, u32),
    /// The release's title (the tag when it has none).
    pub name: String,
    /// The release's page.
    pub url: String,
    /// This platform's archive.
    pub archive: Option<Asset>,
    /// The archive's `.sha256`.
    pub checksum: Option<Asset>,
}

impl Release {
    pub fn version_text(&self) -> String {
        let (a, b, c) = self.version;
        format!("{a}.{b}.{c}")
    }

    /// Is it newer than this launcher?
    pub fn is_newer(&self) -> bool {
        parse_version(VERSION).is_some_and(|v| self.version > v)
    }
}

/// Why a check, download or install did not work; the launcher words it in its language (`Tx::update_fail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The repository has no (public) release yet.
    NoReleases,
    /// No answer, or not one we understand.
    Network(String),
    /// The release has no archive for this system (the platform name).
    NoArchive(String),
    /// The release has no `.sha256` for the archive.
    NoChecksum,
    /// The download does not match its `.sha256`.
    Checksum,
    /// Unpacking or replacing the programs failed.
    Install(String),
}

/// Reads the API's answer for `/releases/latest` and picks the asset for `platform`.
pub fn parse_release(json: &str, platform: &str) -> Result<Release, Fail> {
    let r: ApiRelease = serde_json::from_str(json).map_err(|e| Fail::Network(e.to_string()))?;
    if r.draft || r.prerelease {
        return Err(Fail::NoReleases);
    }
    let version = parse_version(&r.tag_name).ok_or_else(|| Fail::Network(format!("tag {}", r.tag_name)))?;
    let is_archive = |n: &str| n.ends_with(".zip") || n.ends_with(".tar.gz") || n.ends_with(".tgz");
    let archive = r.assets.iter().find(|a| a.name.contains(platform) && is_archive(&a.name)).cloned();
    let checksum = archive.as_ref().and_then(|a| r.assets.iter().find(|c| c.name == format!("{}.sha256", a.name)).cloned());
    let name = r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone());
    Ok(Release { tag: r.tag_name, version, name, url: r.html_url, archive, checksum })
}

/// `curl` with the options every call uses: fail on HTTP errors, follow redirects (release files are served from another host),
/// a generic user agent (the API asks for one).
fn curl() -> Command {
    let mut c = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    c.args(["-fsSL", "--connect-timeout", "15", "-A", "OpenSOW-Launcher"]);
    c
}

/// Asks GitHub for the latest release.
pub fn fetch_latest() -> Result<Release, Fail> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
    // -w appends the status code, so a 404 (no release, or a private repository) is told apart from a broken connection
    let out = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .args(["-sSL", "--connect-timeout", "15", "-A", "OpenSOW-Launcher", "-H", "Accept: application/vnd.github+json", "-w", "\n%{http_code}", &url])
        .output()
        .map_err(|e| Fail::Network(e.to_string()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    match code.trim() {
        "200" => parse_release(body, &platform()),
        "404" => Err(Fail::NoReleases),
        "000" | "" => Err(Fail::Network(String::from_utf8_lossy(&out.stderr).trim().to_owned())),
        c => Err(Fail::Network(format!("HTTP {c}"))),
    }
}

/// Starts downloading `url` to `dest` (a child process; the caller watches the file grow for the progress bar).
pub fn start_download(url: &str, dest: &Path) -> std::io::Result<std::process::Child> {
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d)?;
    }
    curl().arg("-o").arg(dest).arg(url).spawn()
}

/// Downloads a small file (the `.sha256`) into memory.
pub fn fetch_text(url: &str) -> Result<String, Fail> {
    let out = curl().arg(url).output().map_err(|e| Fail::Network(e.to_string()))?;
    if !out.status.success() {
        return Err(Fail::Network(String::from_utf8_lossy(&out.stderr).trim().to_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// SHA-256 of `data` (FIPS 180-4).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Is `data` what the `.sha256` file says (its first word, the `sha256sum` / `shasum -a 256` format)?
pub fn matches_checksum(data: &[u8], sha_file: &str) -> bool {
    sha_file.split_whitespace().next().is_some_and(|want| want.eq_ignore_ascii_case(&hex(&sha256(data))))
}

/// A program's file name on this system.
fn exe_name(program: &str) -> String {
    if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_owned()
    }
}

/// Unpacks `archive` into `into` (`tar` reads zip archives too on macOS and Windows; Linux uses `unzip` for them).
fn unpack(archive: &Path, into: &Path) -> Result<(), Fail> {
    std::fs::create_dir_all(into).map_err(|e| Fail::Install(e.to_string()))?;
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let status = if name.ends_with(".zip") && cfg!(target_os = "linux") {
        Command::new("unzip").arg("-oq").arg(archive).arg("-d").arg(into).status()
    } else {
        Command::new(if cfg!(windows) { "tar.exe" } else { "tar" }).arg("-xf").arg(archive).arg("-C").arg(into).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(Fail::Install(format!("unpacking failed ({s})"))),
        Err(e) => Err(Fail::Install(e.to_string())),
    }
}

/// The first file named `name` under `dir` (an archive may put the programs in a folder).
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = vec![];
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            dirs.push(p);
        } else if p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)) {
            return Some(p);
        }
    }
    dirs.iter().find_map(|d| find_file(d, name))
}

/// Puts `new` in place of `dest`. A running program's file cannot be overwritten on Windows but can be renamed, so the old one
/// becomes `<name>.old` (removed at the next start, [`remove_leftovers`]); elsewhere the new file is renamed over the old.
fn replace(new: &Path, dest: &Path) -> std::io::Result<()> {
    let staged = dest.with_extension("new");
    std::fs::copy(new, &staged)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(windows) && dest.exists() {
        let old = dest.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(dest, &old)?;
    }
    std::fs::rename(&staged, dest)
}

/// Checks the download, unpacks it in `work` and replaces the programs in `target` (the launcher's folder). Returns the
/// programs it replaced.
pub fn install(archive: &Path, sha_file: &str, work: &Path, target: &Path) -> Result<Vec<String>, Fail> {
    let data = std::fs::read(archive).map_err(|e| Fail::Install(e.to_string()))?;
    if !matches_checksum(&data, sha_file) {
        return Err(Fail::Checksum);
    }
    let unpacked = work.join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked);
    unpack(archive, &unpacked)?;
    let found: Vec<(String, PathBuf)> = PROGRAMS.iter().filter_map(|p| find_file(&unpacked, &exe_name(p)).map(|f| (exe_name(p), f))).collect();
    if !found.iter().any(|(n, _)| *n == exe_name(PROGRAMS[0])) {
        return Err(Fail::Install("the archive has no opensow-launcher".into()));
    }
    for (name, file) in &found {
        replace(file, &target.join(name)).map_err(|e| Fail::Install(format!("{name}: {e}")))?;
    }
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(found.into_iter().map(|(n, _)| n).collect())
}

/// Removes the `.old` programs a Windows update left behind.
pub fn remove_leftovers(dir: &Path) {
    for p in PROGRAMS {
        let _ = std::fs::remove_file(dir.join(exe_name(p)).with_extension("old"));
    }
}

/// Opens a web page in the system's browser.
pub fn open_url(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10"), Some((0, 10, 0)));
        assert_eq!(parse_version("v1.2.3-beta"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert!(parse_version(VERSION).is_some(), "the crate's own version is a plain x.y.z");
    }

    const API: &str = r#"{"tag_name":"v9.1.0","name":"Spring update","html_url":"https://github.com/o/r/releases/tag/v9.1.0","draft":false,"prerelease":false,
        "assets":[{"name":"opensow-macos-aarch64.zip","browser_download_url":"https://x/a.zip","size":1234},
                  {"name":"opensow-macos-aarch64.zip.sha256","browser_download_url":"https://x/a.zip.sha256","size":90},
                  {"name":"opensow-windows-x86_64.zip","browser_download_url":"https://x/w.zip","size":99}]}"#;

    #[test]
    fn the_release_brings_this_platforms_archive() {
        let r = parse_release(API, "macos-aarch64").unwrap();
        assert_eq!((r.version, r.name.as_str()), ((9, 1, 0), "Spring update"));
        assert!(r.is_newer());
        assert_eq!(r.archive.unwrap().size, 1234);
        assert_eq!(r.checksum.unwrap().browser_download_url, "https://x/a.zip.sha256");
        let w = parse_release(API, "windows-x86_64").unwrap();
        assert!(w.archive.is_some() && w.checksum.is_none(), "an archive without its .sha256 is found but cannot be installed");
        assert!(parse_release(API, "linux-x86_64").unwrap().archive.is_none());
        assert_eq!(parse_release(&API.replace("\"prerelease\":false", "\"prerelease\":true"), "macos-aarch64"), Err(Fail::NoReleases));
        assert!(matches!(parse_release("<html>", "x"), Err(Fail::Network(_))));
    }

    #[test]
    fn sha256_known_values() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = vec![b'a'; 1000];
        assert_eq!(hex(&sha256(&long)), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
        assert!(matches_checksum(b"abc", "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD  opensow.zip\n"));
        assert!(!matches_checksum(b"abd", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    }

    /// A whole install from a real `.tar.gz` in a temp folder: the hash is checked, both programs replaced.
    #[cfg(unix)]
    #[test]
    fn install_replaces_the_programs() {
        let root = std::env::temp_dir().join(format!("opensow-update-test-{}", std::process::id()));
        let (src, target, work) = (root.join("src/opensow"), root.join("bin"), root.join("work"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(src.join("opensow-launcher"), b"new launcher").unwrap();
        std::fs::write(src.join("opensow-game"), b"new game").unwrap();
        std::fs::write(target.join("opensow-launcher"), b"old").unwrap();
        let archive = root.join("opensow-test.tar.gz");
        assert!(Command::new("tar").arg("-czf").arg(&archive).arg("-C").arg(root.join("src")).arg("opensow").status().unwrap().success());
        let sha = format!("{}  opensow-test.tar.gz\n", hex(&sha256(&std::fs::read(&archive).unwrap())));
        assert_eq!(install(&archive, "0000", &work, &target), Err(Fail::Checksum), "a wrong hash installs nothing");
        assert_eq!(std::fs::read(target.join("opensow-launcher")).unwrap(), b"old");
        assert_eq!(install(&archive, &sha, &work, &target).unwrap(), vec!["opensow-launcher", "opensow-game"]);
        assert_eq!(std::fs::read(target.join("opensow-launcher")).unwrap(), b"new launcher");
        assert_eq!(std::fs::read(target.join("opensow-game")).unwrap(), b"new game");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
