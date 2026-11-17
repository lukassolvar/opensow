//! Updates from GitHub releases (docs/launcher.md, "Updates"). The launcher asks the public API for the latest release of
//! [`REPO`], compares its tag (`vX.Y.Z`) with its own version, downloads this platform's archive (`opensow-<platform>.zip` or
//! `.tar.gz`, see [`platform`]; on macOS the universal `opensow-<version>-macos-universal.dmg`) and its `.sha256`, checks the hash
//! and puts the new `opensow-launcher` and `opensow-game` next to itself (on macOS inside an app bundle: the whole new
//! `OpenSOW.app` replaces the old one). Network and unpacking go through the system's `curl`, `tar` and `unzip` (macOS, Windows 10+ and Linux have them), so
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
    /// The release notes (Markdown).
    #[serde(default)]
    body: Option<String>,
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
    /// The short lines of its notes (`parse_notes`) in each launcher language (`NOTE_LANGS`): English from the release's text,
    /// the others from its `<tag>.<lang>.md` files once fetched (`fetch_translations`); empty where there is none.
    pub notes: [Vec<Note>; 5],
    /// The release's translated notes files: (language index, file).
    pub translations: Vec<(usize, Asset)>,
}

impl Release {
    pub fn version_text(&self) -> String {
        let (a, b, c) = self.version;
        format!("{a}.{b}.{c}")
    }

    /// The notes in language `i` (`NOTE_LANGS`), English where they are not translated.
    pub fn notes_in(&self, i: usize) -> &[Note] {
        match self.notes.get(i) {
            Some(n) if !n.is_empty() => n,
            _ => &self.notes[0],
        }
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
    let is_archive = |n: &str| n.ends_with(".zip") || n.ends_with(".tar.gz") || n.ends_with(".tgz") || (n.ends_with(".dmg") && platform.starts_with("macos-"));
    // this system's own archive first; on macOS the universal disk image serves both processors
    let archive = r
        .assets
        .iter()
        .find(|a| a.name.contains(platform) && is_archive(&a.name))
        .or_else(|| r.assets.iter().find(|a| platform.starts_with("macos-") && a.name.contains("macos-universal") && is_archive(&a.name)))
        .cloned();
    let checksum = archive.as_ref().and_then(|a| r.assets.iter().find(|c| c.name == format!("{}.sha256", a.name)).cloned());
    let name = r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone());
    let mut notes: [Vec<Note>; 5] = Default::default();
    notes[0] = parse_notes(r.body.as_deref().unwrap_or(""));
    let translations = (1..NOTE_LANGS.len()).filter_map(|i| r.assets.iter().find(|a| a.name == notes_file(&r.tag_name, i)).map(|a| (i, a.clone()))).collect();
    Ok(Release { tag: r.tag_name, version, name, url: r.html_url, archive, checksum, notes, translations })
}

/// One line of the home page's "What's new" card: the bold lead of a bullet in a release's notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Under a "Fixed" heading (else a new feature).
    pub fixed: bool,
    pub title: String,
}

/// The notes files' language part, in `Tx::index` order (English has none): `v0.2.0.md`, `v0.2.0.sk.md`, `v0.2.0.cs.md`,
/// `v0.2.0.zh-Hans.md`, `v0.2.0.zh-Hant.md`. The same list is in build.rs.
pub const NOTE_LANGS: [&str; 5] = ["", "sk", "cs", "zh-Hans", "zh-Hant"];

/// A release's notes file in language `i`: `docs/releases/<name>`, and a file of that name on the release.
pub fn notes_file(tag: &str, i: usize) -> String {
    match NOTE_LANGS[i] {
        "" => format!("{tag}.md"),
        l => format!("{tag}.{l}.md"),
    }
}

/// The notes of this launcher's own version in each language: `docs/releases/vX.Y.Z[.lang].md`, built in by build.rs (empty
/// without the file).
pub const OWN_NOTES: [&str; 5] = [
    include_str!(concat!(env!("OUT_DIR"), "/notes.md")),
    include_str!(concat!(env!("OUT_DIR"), "/notes.sk.md")),
    include_str!(concat!(env!("OUT_DIR"), "/notes.cs.md")),
    include_str!(concat!(env!("OUT_DIR"), "/notes.zh-Hans.md")),
    include_str!(concat!(env!("OUT_DIR"), "/notes.zh-Hant.md")),
];

/// Downloads the release's translated notes (a few small files; only for a newer release). One that fails stays English.
pub fn fetch_translations(r: &mut Release) {
    for (i, a) in r.translations.clone() {
        if let Ok(md) = fetch_text(&a.browser_download_url) {
            r.notes[i] = parse_notes(&md);
        }
    }
}

/// The repository's page (the sidebar's GitHub link).
pub fn repo_url() -> String {
    format!("https://github.com/{}", repo())
}

/// The releases page of the repository.
pub fn releases_url() -> String {
    format!("https://github.com/{}/releases", repo())
}

/// The short lines of release notes in the form `docs/releases/*.md` uses: bullets under `## New` (or `## What's in it`,
/// `## Added`, `## Changed`, `## Improved`; translated: `## Novinky`, `## Nové`, `## 新功能`, `## 新內容`) and `## Fixed` (`## Opravy`,
/// `## Opravené`, `## 修复`, `## 修復`), each line the bullet's bold lead (`- **Controls menu.** ...`) without
/// its closing full stop or colon (also `。` / `：`); a bullet without a bold lead gives its first sentence. Other sections (Download, Known
/// limitations, GitHub's "What's Changed") are left out.
pub fn parse_notes(md: &str) -> Vec<Note> {
    let mut out = vec![];
    let mut section: Option<bool> = None;
    for line in md.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("## ") {
            let h = h.trim().to_lowercase();
            section = if h.starts_with("fix") || h.starts_with("oprav") || h.starts_with("修复") || h.starts_with("修復") {
                Some(true)
            } else if ["new", "what's new", "what\u{2019}s new", "what's in it", "added", "changed", "improved", "features", "novinky", "nové"]
                .contains(&h.as_str())
                || h.starts_with('新')
            {
                Some(false)
            } else {
                None
            };
            continue;
        }
        if t.starts_with('#') {
            section = None;
            continue;
        }
        let (Some(fixed), Some(item)) = (section, t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))) else { continue };
        let title = match item.strip_prefix("**").and_then(|r| r.split_once("**")) {
            Some((bold, _)) => bold.to_string(),
            None => item.split(". ").next().unwrap_or(item).replace(['*', '`'], ""),
        };
        let title = title.trim().trim_end_matches(['.', ':', '。', '：']).trim().to_string();
        if !title.is_empty() {
            out.push(Note { fixed, title });
        }
    }
    out
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

/// The first file or folder named `name` under `dir` (an archive may put the programs in a folder). Symbolic links are not
/// followed: a disk image's `Applications` link would lead into the installed programs.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = vec![];
    for e in entries.flatten() {
        let p = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)) && !kind.is_symlink() {
            return Some(p);
        }
        if kind.is_dir() {
            dirs.push(p);
        }
    }
    dirs.iter().find_map(|d| find_file(d, name))
}

/// The app bundle `dir` (a launcher's folder) lies in: `X.app` for `X.app/Contents/MacOS`.
fn bundle_of(dir: &Path) -> Option<PathBuf> {
    let contents = dir.parent()?;
    let app = contents.parent()?;
    (dir.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()?.eq_ignore_ascii_case("app")).then(|| app.to_path_buf())
}

/// Mounts a disk image read-only at `at` (macOS `hdiutil`); [`detach`] unmounts it.
fn attach(dmg: &Path, at: &Path) -> Result<(), Fail> {
    std::fs::create_dir_all(at).map_err(|e| Fail::Install(e.to_string()))?;
    let status = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-quiet", "-mountpoint"])
        .arg(at)
        .arg(dmg)
        .status()
        .map_err(|e| Fail::Install(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(Fail::Install(format!("mounting the disk image failed ({status})")))
    }
}

fn detach(at: &Path) {
    let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(at).status();
}

/// Replaces the app bundle `old` with a copy of `new` (`ditto` keeps its links, modes and signature): the copy is made beside
/// it first, so a failed copy leaves the old bundle as it was.
fn replace_bundle(new: &Path, old: &Path) -> std::io::Result<()> {
    let staged = old.with_extension("app.new");
    let backup = old.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&backup);
    let status = Command::new("ditto").arg(new).arg(&staged).status()?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(std::io::Error::other(format!("copying the app failed ({status})")));
    }
    std::fs::rename(old, &backup)?;
    if let Err(e) = std::fs::rename(&staged, old) {
        let _ = std::fs::rename(&backup, old);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
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
    let is_dmg = archive.extension().is_some_and(|e| e.eq_ignore_ascii_case("dmg"));
    if is_dmg {
        let mount = work.join("mount");
        detach(&mount);
        attach(archive, &mount)?;
        let result = install_from_image(&mount, target);
        detach(&mount);
        let _ = std::fs::remove_dir(&mount);
        return result;
    }
    let unpacked = work.join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked);
    unpack(archive, &unpacked)?;
    install_from(&unpacked, target)
}

/// Installs from a mounted disk image: the launcher inside an app bundle gets the image's whole `OpenSOW.app`, a launcher outside
/// one (a cargo build) only the programs from the bundle's `Contents/MacOS`.
fn install_from_image(mount: &Path, target: &Path) -> Result<Vec<String>, Fail> {
    let app = std::fs::read_dir(mount)
        .map_err(|e| Fail::Install(e.to_string()))?
        .flatten()
        .find(|e| e.file_type().is_ok_and(|k| k.is_dir()) && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("app")))
        .map(|e| e.path())
        .ok_or_else(|| Fail::Install("the disk image has no app".into()))?;
    let programs = app.join("Contents/MacOS");
    if !programs.join(PROGRAMS[0]).is_file() {
        return Err(Fail::Install("the disk image has no opensow-launcher".into()));
    }
    match bundle_of(target) {
        Some(bundle) => {
            replace_bundle(&app, &bundle).map_err(|e| Fail::Install(format!("{}: {e}", bundle.display())))?;
            Ok(PROGRAMS.iter().filter(|p| programs.join(p).is_file()).map(|p| p.to_string()).collect())
        }
        None => install_from(&programs, target),
    }
}

/// Copies the programs found under `unpacked` over the ones in `target`.
fn install_from(unpacked: &Path, target: &Path) -> Result<Vec<String>, Fail> {
    let found: Vec<(String, PathBuf)> = PROGRAMS.iter().filter_map(|p| find_file(unpacked, &exe_name(p)).map(|f| (exe_name(p), f))).collect();
    if !found.iter().any(|(n, _)| *n == exe_name(PROGRAMS[0])) {
        return Err(Fail::Install("the archive has no opensow-launcher".into()));
    }
    for (name, file) in &found {
        replace(file, &target.join(name)).map_err(|e| Fail::Install(format!("{name}: {e}")))?;
    }
    let _ = std::fs::remove_dir_all(unpacked);
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
    fn notes_give_the_bold_lead_of_each_new_and_fixed_bullet() {
        let md = "Intro line.\n\n## New\n\n- **Controls menu.** Three pages of hotkeys.\n* Plain bullet. With more text\n\n## Fixed\n\n\
                  - **Slovak and Czech texts:** typos.\n  - **Mission hints** are drawn with a shadow.\n\n## Download\n\n- **Not a note.**\n\n\
                  ## What's Changed\n* CI on every pull request by @someone\n";
        let n = parse_notes(md);
        let got: Vec<(bool, &str)> = n.iter().map(|n| (n.fixed, n.title.as_str())).collect();
        assert_eq!(got, [(false, "Controls menu"), (false, "Plain bullet"), (true, "Slovak and Czech texts"), (true, "Mission hints")]);
        assert!(parse_notes("").is_empty());
        assert_eq!(parse_notes("## What's in it\n- **Both campaigns in one game.** x")[0].title, "Both campaigns in one game");
        // the API's body becomes the release's notes
        let r = parse_release(&API.replace("\"draft\"", "\"body\":\"## Fixed\\r\\n- **Faster.** x\",\"draft\""), "macos-aarch64").unwrap();
        assert_eq!(r.notes[0], [Note { fixed: true, title: "Faster".into() }]);
        assert!(r.translations.is_empty());
        // translated notes files on the release; the card falls back to English for the others
        let api = API.replace("\"assets\":[", "\"assets\":[{\"name\":\"v9.1.0.sk.md\",\"browser_download_url\":\"https://x/sk\"},{\"name\":\"v9.0.0.cs.md\",\"browser_download_url\":\"https://x/old\"},");
        let mut r = parse_release(&api, "macos-aarch64").unwrap();
        assert_eq!(r.translations.iter().map(|(i, a)| (*i, a.browser_download_url.as_str())).collect::<Vec<_>>(), [(1, "https://x/sk")]);
        r.notes[0] = parse_notes("## New\n- **Faster.**");
        r.notes[1] = parse_notes("## Novinky\n- **Rýchlejšie.** x\n## Opravy\n- **Hudba.** y");
        assert_eq!(r.notes_in(1)[1], Note { fixed: true, title: "Hudba".into() });
        assert_eq!(r.notes_in(2)[0].title, "Faster");
        assert_eq!(parse_notes("## 修复\n- **更快。** x\n## 新功能\n- **地图编辑器。**").iter().map(|n| n.fixed).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn every_written_release_has_notes_for_the_card() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/releases");
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let n = parse_notes(&std::fs::read_to_string(e.path()).unwrap());
            assert!(n.len() >= 3, "{}: {n:?}", e.path().display());
            assert!(n.iter().all(|n| n.title.chars().count() < 60 && !n.title.contains('*')), "{}: {n:?}", e.path().display());
            seen += 1;
        }
        assert!(seen >= 2);
        // this version's notes are built in when written
        for (i, own) in OWN_NOTES.iter().enumerate() {
            if dir.join(notes_file(&format!("v{VERSION}"), i)).exists() {
                assert!(!parse_notes(own).is_empty(), "{}", NOTE_LANGS[i]);
            }
        }
    }

    #[test]
    fn macs_take_the_universal_disk_image() {
        let api = r#"{"tag_name":"v9.1.0","name":"","html_url":"u","draft":false,"prerelease":false,
            "assets":[{"name":"opensow-9.1.0-macos-universal.dmg","browser_download_url":"https://x/m.dmg","size":5},
                      {"name":"opensow-9.1.0-macos-universal.dmg.sha256","browser_download_url":"https://x/m.dmg.sha256","size":90},
                      {"name":"opensow-9.1.0-linux-x86_64.tar.gz","browser_download_url":"https://x/l.tgz","size":7}]}"#;
        for mac in ["macos-aarch64", "macos-x86_64"] {
            let r = parse_release(api, mac).unwrap();
            assert_eq!(r.archive.unwrap().name, "opensow-9.1.0-macos-universal.dmg");
            assert_eq!(r.checksum.unwrap().name, "opensow-9.1.0-macos-universal.dmg.sha256");
        }
        assert_eq!(parse_release(api, "linux-x86_64").unwrap().archive.unwrap().size, 7);
        assert!(parse_release(api, "windows-x86_64").unwrap().archive.is_none(), "a disk image is for Macs only");
        assert_eq!(bundle_of(Path::new("/Applications/OpenSOW.app/Contents/MacOS")), Some(PathBuf::from("/Applications/OpenSOW.app")));
        assert_eq!(bundle_of(Path::new("/x/target/release")), None);
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

    /// A whole install from a real disk image in a temp folder: the installed app bundle is replaced as a whole (a stale file
    /// in it is gone), a launcher outside a bundle gets the programs; the image's `Applications` link is never followed.
    #[cfg(target_os = "macos")]
    #[test]
    fn install_replaces_the_app_from_a_disk_image() {
        let root = std::env::temp_dir().join(format!("opensow-dmg-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (stage, work) = (root.join("stage"), root.join("work"));
        let new = stage.join("OpenSOW.app/Contents/MacOS");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("opensow-launcher"), b"new launcher").unwrap();
        std::fs::write(new.join("opensow-game"), b"new game").unwrap();
        std::os::unix::fs::symlink("/Applications", stage.join("Applications")).unwrap();
        let dmg = root.join("opensow-test-macos-universal.dmg");
        let made = Command::new("hdiutil")
            .args(["create", "-quiet", "-fs", "HFS+", "-format", "UDZO", "-volname", "OpenSOW", "-srcfolder"])
            .arg(&stage)
            .arg(&dmg)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            eprintln!("hdiutil cannot make a disk image here; skipped");
            return;
        }
        let sha = hex(&sha256(&std::fs::read(&dmg).unwrap()));
        // installed as an app bundle
        let old = root.join("Apps/OpenSOW.app/Contents/MacOS");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("opensow-launcher"), b"old").unwrap();
        std::fs::write(old.join("stale"), b"old").unwrap();
        assert_eq!(install(&dmg, "0000", &work, &old), Err(Fail::Checksum));
        assert_eq!(install(&dmg, &sha, &work, &old).unwrap(), vec!["opensow-launcher", "opensow-game"]);
        assert_eq!(std::fs::read(old.join("opensow-launcher")).unwrap(), b"new launcher");
        assert_eq!(std::fs::read(old.join("opensow-game")).unwrap(), b"new game");
        assert!(!old.join("stale").exists() && !root.join("Apps/OpenSOW.app.old").exists());
        // a plain folder (cargo build)
        let plain = root.join("bin");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(install(&dmg, &sha, &work, &plain).unwrap(), vec!["opensow-launcher", "opensow-game"]);
        assert_eq!(std::fs::read(plain.join("opensow-game")).unwrap(), b"new game");
        assert!(!work.join("mount").exists(), "the image is unmounted");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
