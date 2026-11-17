//! Native archive package staging for ClawHub installs.
//!
//! One explicit archive version is downloaded and staged in an isolated
//! temporary directory. Nothing here publishes, touches the Canonical store or
//! writes an Agent directory; the caller owns the Mutation session.

use super::{
    client,
    detail::{self, VersionFiles},
};
use crate::{
    baseline::fingerprint_folder,
    error::AppError,
    marketplace::{Coordinate, MarketplaceSource, files, validate_version},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const DOWNLOAD_PATH: &str = "/api/v1/download";
const META_FILE: &str = "_meta.json";
const SKILL_FILE: &str = "SKILL.md";
const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4_096;
const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_META_BYTES: u64 = 1024 * 1024;
const MAX_COMPRESSION_RATIO: u128 = 100;

/// A verified skill folder in temporary storage. Dropping it removes the
/// staging directory.
#[derive(Debug)]
pub struct StagedPackage {
    _directory: tempfile::TempDir,
    root: PathBuf,
    pub name: String,
    pub content_hash: String,
    pub file_count: u64,
    pub total_bytes: u64,
}

impl StagedPackage {
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Revalidate the staging folder against the filesystem immediately before
    /// publication, mirroring the SkillHub staging check.
    pub fn check(&self) -> Result<(), AppError> {
        if !fs::symlink_metadata(&self.root).is_ok_and(|meta| meta.is_dir())
            || fs::canonicalize(&self.root).ok().as_ref() != Some(&self.root)
        {
            return Err(AppError::clawhub_package_invalid("暂存目录已被移动或替换"));
        }
        Ok(())
    }

    /// Finish a staging folder whose content was verified by a source-specific
    /// adapter (currently GitHub codeload) and capture its record metadata.
    pub(crate) fn from_verified(
        directory: tempfile::TempDir,
        root: PathBuf,
    ) -> Result<Self, AppError> {
        let base = fs::canonicalize(directory.path()).map_err(|error| {
            AppError::clawhub_package_invalid(format!("暂存目录不可用：{error}"))
        })?;
        let root = fs::canonicalize(&root).map_err(|error| {
            AppError::clawhub_package_invalid(format!("暂存目录不可用：{error}"))
        })?;
        if root == base || !root.starts_with(&base) {
            return Err(AppError::clawhub_package_invalid("暂存目录越界"));
        }
        let markdown = fs::read_to_string(root.join(SKILL_FILE)).map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法读取 SKILL.md：{error}"))
        })?;
        let name = crate::marketplace::original_name(&markdown)
            .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
        let mut file_count = 0u64;
        let mut total_bytes = 0u64;
        for entry in walkdir::WalkDir::new(&root).follow_links(false) {
            let entry = entry.map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法检查暂存内容：{error}"))
            })?;
            if entry.file_type().is_file() {
                file_count += 1;
                total_bytes = total_bytes.saturating_add(
                    entry
                        .metadata()
                        .map_err(|error| {
                            AppError::clawhub_package_invalid(format!(
                                "无法读取暂存文件信息：{error}"
                            ))
                        })?
                        .len(),
                );
            }
        }
        let fingerprint = fingerprint_folder(&root).map_err(AppError::skill_fingerprint_failed)?;
        Ok(Self {
            _directory: directory,
            root,
            name,
            content_hash: fingerprint.content_hash,
            file_count,
            total_bytes,
        })
    }
}

#[cfg(test)]
impl StagedPackage {
    pub(crate) fn from_folder(source: &Path) -> Result<Self, AppError> {
        let directory = tempfile::Builder::new()
            .prefix("skills-box-clawhub-test-")
            .tempdir()
            .map_err(|error| AppError::clawhub_package_invalid(error.to_string()))?;
        let base = fs::canonicalize(directory.path())
            .map_err(|error| AppError::clawhub_package_invalid(error.to_string()))?;
        let root = base.join("package");
        fs::create_dir(&root)
            .map_err(|error| AppError::clawhub_package_invalid(error.to_string()))?;
        crate::backup::copy_tree(source, &root)
            .map_err(|error| AppError::clawhub_package_invalid(error.to_string()))?;
        let markdown = fs::read_to_string(root.join(SKILL_FILE))
            .map_err(|error| AppError::clawhub_package_invalid(error.to_string()))?;
        let name = crate::marketplace::original_name(&markdown)
            .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
        let fingerprint = fingerprint_folder(&root).map_err(AppError::skill_fingerprint_failed)?;
        Ok(Self {
            _directory: directory,
            root,
            name,
            content_hash: fingerprint.content_hash,
            file_count: fingerprint.file_count,
            total_bytes: fingerprint.total_bytes,
        })
    }
}

/// Download one archive release and return its verified staging folder.
pub async fn stage_archive(
    coordinate: &Coordinate,
    version: &str,
) -> Result<StagedPackage, AppError> {
    if coordinate.source != MarketplaceSource::Clawhub {
        return Err(AppError::clawhub_package_invalid(
            "原生安装包需要 ClawHub 来源坐标",
        ));
    }
    coordinate.validate()?;
    validate_version(version)?;
    let listing = detail::version_files(coordinate, version).await?;
    let expected = expected_files(&listing)?;
    let query = [
        ("slug", coordinate.slug.clone()),
        ("ownerHandle", coordinate.namespace.clone()),
        ("version", version.to_string()),
    ];
    let payload = client::get_bytes_at(DOWNLOAD_PATH, &query, MAX_DOWNLOAD_BYTES).await?;
    let bytes = match payload {
        client::Payload::NotFound => {
            return Err(AppError::clawhub_version_missing(
                version,
                "下载接口返回 HTTP 404",
            ));
        }
        client::Payload::TooLarge => {
            return Err(AppError::clawhub_package_invalid(
                "ClawHub 安装包超过 50 MB 下载限制",
            ));
        }
        client::Payload::Body(bytes) => bytes,
    };
    let version = listing.version.clone();
    tokio::task::spawn_blocking(move || extract_archive(bytes, &version, expected))
        .await
        .map_err(|error| AppError::clawhub_package_invalid(format!("解压任务失败：{error}")))?
}

#[derive(Debug, Clone)]
struct ExpectedFile {
    size: u64,
    sha256: String,
}

/// Validate the version file list itself before trusting it as the expected
/// content set. Every file must carry a SHA-256; otherwise the package cannot
/// be verified and installation is refused.
fn expected_files(listing: &VersionFiles) -> Result<BTreeMap<String, ExpectedFile>, AppError> {
    let mut lower_paths = BTreeMap::new();
    let mut expected = BTreeMap::new();
    let mut total = 0u64;
    for entry in &listing.files {
        files::validate_path(&entry.path)
            .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
        let sha256 = entry
            .sha256
            .as_deref()
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| {
                AppError::clawhub_package_invalid(format!(
                    "版本文件 {} 缺少可校验的 SHA-256",
                    entry.path
                ))
            })?;
        if entry.size_bytes > MAX_FILE_BYTES {
            return Err(AppError::clawhub_package_invalid(format!(
                "版本文件 {} 超过单文件大小限制",
                entry.path
            )));
        }
        total = total.saturating_add(entry.size_bytes);
        if total > MAX_UNCOMPRESSED_BYTES {
            return Err(AppError::clawhub_package_invalid(
                "版本内容超过解压后大小限制",
            ));
        }
        let lower = entry.path.to_lowercase();
        if lower_paths.insert(lower, entry.path.clone()).is_some() {
            return Err(AppError::clawhub_package_invalid(format!(
                "版本文件列表包含大小写冲突：{}",
                entry.path
            )));
        }
        expected.insert(
            entry.path.clone(),
            ExpectedFile {
                size: entry.size_bytes,
                sha256: sha256.to_ascii_lowercase(),
            },
        );
    }
    if !expected.contains_key(SKILL_FILE) {
        return Err(AppError::clawhub_package_invalid(
            "版本文件列表缺少 SKILL.md",
        ));
    }
    for path in expected.keys() {
        for (index, _) in path.match_indices('/') {
            if lower_paths.contains_key(&path[..index].to_lowercase()) {
                return Err(AppError::clawhub_package_invalid(format!(
                    "版本文件与目录路径冲突：{path}"
                )));
            }
        }
    }
    Ok(expected)
}

#[derive(Debug)]
struct PlannedEntry {
    index: usize,
    path: String,
    expected: Option<ExpectedFile>,
    exec_bits: u32,
}

fn extract_archive(
    bytes: Vec<u8>,
    version: &str,
    expected: BTreeMap<String, ExpectedFile>,
) -> Result<StagedPackage, AppError> {
    let directory = tempfile::Builder::new()
        .prefix("skills-box-clawhub-")
        .tempdir()
        .map_err(|error| AppError::clawhub_package_invalid(format!("无法创建暂存目录：{error}")))?;
    let base = fs::canonicalize(directory.path())
        .map_err(|error| AppError::clawhub_package_invalid(format!("暂存目录不可用：{error}")))?;
    let root = base.join("package");
    fs::create_dir(&root)
        .map_err(|error| AppError::clawhub_package_invalid(format!("无法创建暂存目录：{error}")))?;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        AppError::clawhub_package_invalid(format!("安装包不是有效 ZIP：{error}"))
    })?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AppError::clawhub_package_invalid("安装包条目数超过限制"));
    }
    let expected_dirs = expected_directories(&expected);
    let mut planned = Vec::new();
    let mut seen = BTreeMap::new();
    let mut total_uncompressed = 0u64;
    let mut total_compressed = 0u64;
    let mut meta_extra = false;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法读取安装包条目：{error}"))
        })?;
        if file.encrypted() {
            return Err(AppError::clawhub_package_invalid("安装包包含加密条目"));
        }
        if file.is_symlink() {
            return Err(AppError::clawhub_package_invalid("安装包包含符号链接"));
        }
        if file.enclosed_name().is_none() {
            return Err(AppError::clawhub_package_invalid(
                "安装包条目路径越界或为绝对路径",
            ));
        }
        let raw = file
            .name()
            .map_err(|error| {
                AppError::clawhub_package_invalid(format!("安装包条目名称无效：{error}"))
            })?
            .into_owned();
        let is_dir = file.is_dir();
        let path = if is_dir {
            raw.strip_suffix('/').unwrap_or(&raw).to_string()
        } else {
            if raw.ends_with('/') {
                return Err(AppError::clawhub_package_invalid(format!(
                    "安装包文件路径无效：{raw}"
                )));
            }
            raw.clone()
        };
        files::validate_path(&path)
            .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
        let lower = path.to_lowercase();
        if seen.insert(lower.clone(), path.clone()).is_some() {
            return Err(AppError::clawhub_package_invalid(format!(
                "安装包包含重复路径：{path}"
            )));
        }
        if is_dir {
            if !expected_dirs.contains(&lower) {
                return Err(AppError::clawhub_package_invalid(format!(
                    "安装包包含未声明的目录：{path}"
                )));
            }
            continue;
        }
        let size = file.size();
        let compressed = file.compressed_size();
        if size > MAX_FILE_BYTES {
            return Err(AppError::clawhub_package_invalid(format!(
                "安装包文件 {path} 超过单文件大小限制"
            )));
        }
        if size > 0 && compressed == 0 {
            return Err(AppError::clawhub_package_invalid(format!(
                "安装包文件 {path} 的压缩信息无效"
            )));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        total_compressed = total_compressed.saturating_add(compressed);
        let expected = match expected.get(&path) {
            Some(expected) => {
                if expected.size != size {
                    return Err(AppError::clawhub_package_invalid(format!(
                        "安装包文件 {path} 的大小与版本文件列表不一致"
                    )));
                }
                Some(expected.clone())
            }
            None if path == META_FILE => {
                if size > MAX_META_BYTES {
                    return Err(AppError::clawhub_package_invalid("_meta.json 超过大小限制"));
                }
                if meta_extra {
                    return Err(AppError::clawhub_package_invalid(
                        "安装包包含重复的 _meta.json",
                    ));
                }
                meta_extra = true;
                None
            }
            None => {
                return Err(AppError::clawhub_package_invalid(format!(
                    "安装包包含版本文件列表之外的文件：{path}"
                )));
            }
        };
        planned.push(PlannedEntry {
            index,
            path,
            expected,
            exec_bits: file.unix_mode().unwrap_or(0) & 0o111,
        });
    }
    for path in expected.keys() {
        if !seen.contains_key(&path.to_lowercase()) {
            return Err(AppError::clawhub_package_invalid(format!(
                "安装包缺少版本文件列表中的文件：{path}"
            )));
        }
    }
    if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
        return Err(AppError::clawhub_package_invalid(
            "安装包解压后大小超过限制",
        ));
    }
    if total_uncompressed > 0
        && (total_compressed == 0
            || u128::from(total_uncompressed)
                > u128::from(total_compressed) * MAX_COMPRESSION_RATIO)
    {
        return Err(AppError::clawhub_package_invalid(
            "安装包压缩比异常，已拒绝解压",
        ));
    }

    let mut written_files = 0usize;
    let mut written_total = 0u64;
    for entry in planned {
        let mut source = archive.by_index(entry.index).map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法读取安装包条目：{error}"))
        })?;
        let destination = root.join(&entry.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法创建技能目录：{error}"))
            })?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法写入技能文件：{error}"))
            })?;
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(|error| {
                AppError::clawhub_package_invalid(format!("解压 {} 失败：{error}", entry.path))
            })?;
            if read == 0 {
                break;
            }
            written = written.saturating_add(read as u64);
            let limit = entry
                .expected
                .as_ref()
                .map_or(MAX_META_BYTES, |expected| expected.size);
            if written > limit {
                return Err(AppError::clawhub_package_invalid(format!(
                    "解压 {} 时内容超过声明大小",
                    entry.path
                )));
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read]).map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法写入技能文件：{error}"))
            })?;
        }
        if let Some(expected) = &entry.expected {
            if written != expected.size {
                return Err(AppError::clawhub_package_invalid(format!(
                    "解压 {} 后的大小与版本文件列表不一致",
                    entry.path
                )));
            }
            if hex(&hasher.finalize()) != expected.sha256 {
                return Err(AppError::clawhub_package_invalid(format!(
                    "解压 {} 后的 SHA-256 与版本文件列表不一致",
                    entry.path
                )));
            }
        }
        #[cfg(unix)]
        if entry.exec_bits != 0 {
            // Archive modes are untrusted; keep only executable bits on top of
            // the default file mode.
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&destination)
                .map_err(|error| {
                    AppError::clawhub_package_invalid(format!("无法读取技能文件权限：{error}"))
                })?
                .permissions();
            permissions.set_mode(permissions.mode() | entry.exec_bits);
            fs::set_permissions(&destination, permissions).map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法设置技能文件权限：{error}"))
            })?;
        }
        written_files += 1;
        written_total = written_total.saturating_add(written);
    }
    if written_files != expected.len() + usize::from(meta_extra) {
        return Err(AppError::clawhub_package_invalid("安装包文件数量不一致"));
    }
    if meta_extra {
        validate_meta(&root.join(META_FILE), version)?;
    }
    verify_staged_tree(&root)?;

    let markdown = fs::read_to_string(root.join(SKILL_FILE)).map_err(|error| {
        AppError::clawhub_package_invalid(format!("无法读取 SKILL.md：{error}"))
    })?;
    let name = crate::marketplace::original_name(&markdown)
        .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
    let fingerprint = fingerprint_folder(&root).map_err(AppError::skill_fingerprint_failed)?;
    Ok(StagedPackage {
        _directory: directory,
        root,
        name,
        content_hash: fingerprint.content_hash,
        file_count: written_files as u64,
        total_bytes: written_total,
    })
}

fn expected_directories(expected: &BTreeMap<String, ExpectedFile>) -> BTreeSet<String> {
    let mut directories = BTreeSet::new();
    for path in expected.keys() {
        for (index, _) in path.match_indices('/') {
            directories.insert(path[..index].to_lowercase());
        }
    }
    directories
}

fn validate_meta(path: &Path, version: &str) -> Result<(), AppError> {
    let raw = fs::read(path).map_err(|error| {
        AppError::clawhub_package_invalid(format!("无法读取 _meta.json：{error}"))
    })?;
    let value: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|error| AppError::clawhub_package_invalid(format!("_meta.json 无效：{error}")))?;
    if value.get("version").and_then(serde_json::Value::as_str) != Some(version) {
        return Err(AppError::clawhub_package_invalid(
            "_meta.json 的版本与请求版本不一致",
        ));
    }
    Ok(())
}

fn verify_staged_tree(root: &Path) -> Result<(), AppError> {
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法检查暂存内容：{error}"))
        })?;
        let kind = entry.file_type();
        if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
            return Err(AppError::clawhub_package_invalid(
                "暂存内容包含符号链接或特殊文件",
            ));
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::MarketplaceSource;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader},
        net::TcpListener,
    };

    #[derive(Clone)]
    struct Reply {
        status: u16,
        body: Vec<u8>,
        declared: Option<usize>,
        written: Option<usize>,
    }

    impl Reply {
        fn json(value: Value) -> Self {
            Self {
                status: 200,
                body: serde_json::to_vec(&value).unwrap(),
                declared: None,
                written: None,
            }
        }
        fn bytes(body: Vec<u8>) -> Self {
            Self {
                status: 200,
                body,
                declared: None,
                written: None,
            }
        }
        fn truncated(body: Vec<u8>) -> Self {
            let written = body.len() / 2;
            Self {
                status: 200,
                body,
                declared: Some(1_000_000),
                written: Some(written),
            }
        }
        fn status(status: u16) -> Self {
            Self {
                status,
                body: Vec::new(),
                declared: None,
                written: None,
            }
        }
        fn too_large() -> Self {
            Self {
                status: 200,
                body: Vec::new(),
                declared: Some((MAX_DOWNLOAD_BYTES + 1) as usize),
                written: Some(0),
            }
        }
    }

    fn serve(routes: Vec<(&str, Reply)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let routes: HashMap<String, Reply> = routes
            .into_iter()
            .map(|(path, reply)| (path.to_string(), reply))
            .collect();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(clone) = stream.try_clone() else {
                    continue;
                };
                let mut reader = BufReader::new(clone);
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err()
                        || header == "\r\n"
                        || header.is_empty()
                    {
                        break;
                    }
                }
                let target = request.split_whitespace().nth(1).unwrap_or("/");
                let path = target.split('?').next().unwrap_or(target);
                let reply = routes.get(path).cloned().unwrap_or(Reply::status(404));
                let declared = reply.declared.unwrap_or(reply.body.len());
                let reason = match reply.status {
                    200 => "OK",
                    404 => "Not Found",
                    429 => "Too Many Requests",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {} {reason}\r\nContent-Type: application/octet-stream\r\nCache-Control: public, max-age=0\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n",
                    reply.status
                );
                let _ = stream.write_all(response.as_bytes());
                let written = reply.written.unwrap_or(reply.body.len());
                let _ = stream.write_all(&reply.body[..written]);
            }
        });
        address
    }

    fn use_server(routes: Vec<(&str, Reply)>) -> client::TestBaseGuard {
        let address = serve(routes);
        let guard = client::use_test_base(&address);
        client::reset_test_state();
        guard
    }

    fn options() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
    }

    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in entries {
            if let Some(directory) = name.strip_suffix('/') {
                writer.add_directory(directory, options()).unwrap();
            } else {
                writer.start_file(*name, options()).unwrap();
                writer.write_all(body).unwrap();
            }
        }
        writer.finish().unwrap().into_inner()
    }

    fn zip_with_mode(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body, mode) in entries {
            writer
                .start_file(*name, options().unix_permissions(*mode))
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn sha(body: &[u8]) -> String {
        hex(&Sha256::digest(body))
    }

    /// The writer refuses duplicate names, so patch a same-length entry name to
    /// build the reader-side duplicate fixture.
    fn duplicate_name_archive() -> Vec<u8> {
        let archive = zip(&[
            ("SKILL.md", MARKDOWN),
            ("aa.md", b"x"),
            ("bb.md", b"y"),
            ("_meta.json", META),
        ]);
        let from = b"bb.md";
        let to = b"aa.md";
        let mut patched = archive.clone();
        let mut index = 0;
        while index + from.len() <= patched.len() {
            if &patched[index..index + from.len()] == from {
                patched[index..index + from.len()].copy_from_slice(to);
                index += from.len();
            } else {
                index += 1;
            }
        }
        patched
    }

    fn coordinate(slug: &str) -> Coordinate {
        Coordinate::from_canonical_for(MarketplaceSource::Clawhub, &format!("@owner/{slug}"))
            .unwrap()
    }

    fn version_path(slug: &str) -> String {
        format!("/api/v1/skills/{slug}/versions/1.0.0")
    }

    fn listing(slug: &str, files: Vec<Value>) -> Value {
        json!({
            "skill": {"slug": slug},
            "version": {"version": "1.0.0", "files": files}
        })
    }

    fn file(path: &str, body: &[u8]) -> Value {
        json!({"path": path, "size": body.len(), "sha256": sha(body)})
    }

    const MARKDOWN: &[u8] = b"---\nname: weather\n---\n# Weather\n";
    const SCRIPT: &[u8] = b"print('hi')\n";
    const META: &[u8] = br#"{"version":"1.0.0"}"#;

    async fn stage_with(
        slug: &str,
        files: Vec<Value>,
        archive: Vec<u8>,
    ) -> Result<StagedPackage, AppError> {
        let _guard = use_server(vec![
            (&version_path(slug), Reply::json(listing(slug, files))),
            ("/api/v1/download", Reply::bytes(archive)),
        ]);
        stage_archive(&coordinate(slug), "1.0.0").await
    }

    #[tokio::test]
    async fn stages_an_archive_package_and_verifies_every_file() {
        let archive = zip(&[
            ("SKILL.md", MARKDOWN),
            ("scripts/", b""),
            ("scripts/run.py", SCRIPT),
            ("_meta.json", META),
        ]);
        let package = stage_with(
            "weather",
            vec![file("SKILL.md", MARKDOWN), file("scripts/run.py", SCRIPT)],
            archive,
        )
        .await
        .unwrap();
        assert_eq!(package.name, "weather");
        assert_eq!(package.file_count, 3);
        assert_eq!(package.content_hash.len(), 64);
        assert_eq!(
            fs::read(package.path().join("scripts/run.py")).unwrap(),
            SCRIPT
        );
        assert!(package.path().join("_meta.json").is_file());
        assert!(!package.path().join(".clawhub").exists());
        package.check().unwrap();
    }

    #[tokio::test]
    async fn rejects_hash_size_missing_extra_and_meta_mismatches() {
        let cases: Vec<(&str, Vec<Value>, Vec<u8>)> = vec![
            (
                "hash-mismatch",
                vec![json!({"path": "SKILL.md", "size": MARKDOWN.len(), "sha256": "0".repeat(64)})],
                zip(&[("SKILL.md", MARKDOWN), ("_meta.json", META)]),
            ),
            (
                "size-mismatch",
                vec![
                    json!({"path": "SKILL.md", "size": MARKDOWN.len() + 1, "sha256": sha(MARKDOWN)}),
                ],
                zip(&[("SKILL.md", MARKDOWN), ("_meta.json", META)]),
            ),
            (
                "missing-file",
                vec![file("SKILL.md", MARKDOWN), file("notes.md", b"missing")],
                zip(&[("SKILL.md", MARKDOWN), ("_meta.json", META)]),
            ),
            (
                "extra-file",
                vec![file("SKILL.md", MARKDOWN)],
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    ("notes.md", b"extra"),
                    ("_meta.json", META),
                ]),
            ),
            (
                "clawhub-metadata",
                vec![file("SKILL.md", MARKDOWN)],
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    (".clawhub/origin.json", b"{}"),
                    ("_meta.json", META),
                ]),
            ),
            (
                "meta-version",
                vec![file("SKILL.md", MARKDOWN)],
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    ("_meta.json", br#"{"version":"9.9.9"}"#),
                ]),
            ),
            (
                "missing-digest",
                vec![json!({"path": "SKILL.md", "size": MARKDOWN.len(), "sha256": null})],
                zip(&[("SKILL.md", MARKDOWN), ("_meta.json", META)]),
            ),
        ];
        for (slug, files, archive) in cases {
            let error = stage_with(slug, files, archive).await.unwrap_err();
            assert_eq!(error.code, crate::error::ErrorCode::VerifyFailed, "{slug}");
        }
    }

    #[tokio::test]
    async fn rejects_unsafe_archive_entries() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "traversal",
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    ("../escape.txt", b"x"),
                    ("_meta.json", META),
                ]),
            ),
            (
                "absolute",
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    ("/etc/passwd", b"x"),
                    ("_meta.json", META),
                ]),
            ),
            ("duplicate", duplicate_name_archive()),
            (
                "case-collision",
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    ("A.md", b"x"),
                    ("a.md", b"y"),
                    ("_meta.json", META),
                ]),
            ),
            (
                "unexpected-dir",
                zip(&[
                    ("SKILL.md", MARKDOWN),
                    ("extra/", b""),
                    ("_meta.json", META),
                ]),
            ),
            (
                "symlink",
                zip_with_mode(&[
                    ("SKILL.md", MARKDOWN, 0o100644),
                    ("link", b"SKILL.md", 0o120777),
                    ("_meta.json", META, 0o100644),
                ]),
            ),
        ];
        for (slug, archive) in cases {
            let error = stage_with(slug, vec![file("SKILL.md", MARKDOWN)], archive)
                .await
                .unwrap_err();
            assert_eq!(error.code, crate::error::ErrorCode::VerifyFailed, "{slug}");
        }
    }

    #[tokio::test]
    async fn rejects_compression_bombs_oversized_and_truncated_downloads() {
        let zeros = vec![0u8; 2 * 1024 * 1024];
        let bomb = zip(&[
            ("SKILL.md", MARKDOWN),
            ("big.bin", &zeros),
            ("_meta.json", META),
        ]);
        let error = stage_with(
            "bomb",
            vec![file("SKILL.md", MARKDOWN), file("big.bin", &zeros)],
            bomb,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::VerifyFailed);

        let _guard = use_server(vec![
            (
                &version_path("oversized"),
                Reply::json(listing("oversized", vec![file("SKILL.md", MARKDOWN)])),
            ),
            ("/api/v1/download", Reply::too_large()),
        ]);
        let error = stage_archive(&coordinate("oversized"), "1.0.0")
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::VerifyFailed);

        drop(_guard);
        let archive = zip(&[("SKILL.md", MARKDOWN), ("_meta.json", META)]);
        let _guard = use_server(vec![
            (
                &version_path("truncated"),
                Reply::json(listing("truncated", vec![file("SKILL.md", MARKDOWN)])),
            ),
            ("/api/v1/download", Reply::truncated(archive)),
        ]);
        let error = stage_archive(&coordinate("truncated"), "1.0.0")
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::MarketplaceUnavailable);
    }

    #[tokio::test]
    async fn honors_the_shared_clawhub_rate_limit_cooldown() {
        let _guard = use_server(vec![(&version_path("limited"), Reply::status(429))]);
        let error = stage_archive(&coordinate("limited"), "1.0.0")
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::MarketplaceRateLimited);
        assert!(error.retry_after_seconds.is_some());
    }

    #[tokio::test]
    #[ignore = "downloads one explicit ClawHub archive release; requires network"]
    async fn clawhub_live_archive_stages_and_verifies() {
        let coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/weather")
                .unwrap();
        let package = stage_archive(&coordinate, "1.0.0").await.unwrap();
        assert!(package.path().join("SKILL.md").is_file());
        assert_eq!(package.content_hash.len(), 64);
        package.check().unwrap();
    }
}
