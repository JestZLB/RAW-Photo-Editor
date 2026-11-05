use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::PersistenceError;

/// Project data and cache layout.
///
/// ```text
/// {data}/projects/{project}/assets/...
/// {cache}/projects/{project}/previews/...
/// {cache}/projects/{project}/thumbnails/...
/// ```
#[derive(Debug, Clone)]
pub struct AssetStore {
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl AssetStore {
    pub fn new(data_dir: PathBuf, cache_dir: PathBuf) -> Result<Self, PersistenceError> {
        fs::create_dir_all(data_dir.join("projects"))?;
        fs::create_dir_all(cache_dir.join("projects"))?;
        Ok(Self {
            data_dir,
            cache_dir,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn project_dir(&self, project_id: Uuid) -> PathBuf {
        self.data_dir.join("projects").join(project_id.to_string())
    }

    pub fn assets_dir(&self, project_id: Uuid) -> PathBuf {
        self.project_dir(project_id).join("assets")
    }

    pub fn previews_dir(&self, project_id: Uuid) -> PathBuf {
        self.cache_dir
            .join("projects")
            .join(project_id.to_string())
            .join("previews")
    }

    pub fn project_cache_dir(&self, project_id: Uuid) -> PathBuf {
        self.cache_dir.join("projects").join(project_id.to_string())
    }

    /// Remove generated render previews only. Source previews, thumbnails and
    /// AI assets are kept so recent projects and applied layers stay usable.
    pub fn clear_render_cache(&self) -> Result<(usize, u64), PersistenceError> {
        let projects_dir = self.cache_dir.join("projects");
        if !projects_dir.exists() {
            return Ok((0, 0));
        }
        let mut files = 0usize;
        let mut bytes = 0u64;
        for project in fs::read_dir(&projects_dir)? {
            let project = project?;
            let previews = project.path().join("previews");
            if !previews.exists() {
                continue;
            }
            for entry in fs::read_dir(previews)? {
                let entry = entry?;
                let path = entry.path();
                let is_render_preview = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("render-") && name.ends_with(".png"));
                if !is_render_preview {
                    continue;
                }
                if let Ok(metadata) = entry.metadata() {
                    bytes += metadata.len();
                }
                if fs::remove_file(&path).is_ok() {
                    files += 1;
                }
            }
        }
        Ok((files, bytes))
    }

    /// Delete all managed files for a project after its database row is gone.
    pub fn remove_project_files(&self, project_id: Uuid) {
        let _ = fs::remove_dir_all(self.project_dir(project_id));
        let _ = fs::remove_dir_all(self.project_cache_dir(project_id));
    }

    pub fn thumbnails_dir(&self, project_id: Uuid) -> PathBuf {
        self.cache_dir
            .join("projects")
            .join(project_id.to_string())
            .join("thumbnails")
    }

    pub fn ensure_project_dirs(&self, project_id: Uuid) -> Result<(), PersistenceError> {
        fs::create_dir_all(self.assets_dir(project_id))?;
        fs::create_dir_all(self.previews_dir(project_id))?;
        fs::create_dir_all(self.thumbnails_dir(project_id))?;
        Ok(())
    }

    pub fn source_preview_path(
        &self,
        project_id: Uuid,
        asset_id: Uuid,
        max_dimension: u32,
    ) -> PathBuf {
        self.previews_dir(project_id)
            .join(format!("source-{asset_id}-{max_dimension}.png"))
    }

    pub fn source_thumbnail_path(
        &self,
        project_id: Uuid,
        asset_id: Uuid,
        max_dimension: u32,
    ) -> PathBuf {
        self.thumbnails_dir(project_id)
            .join(format!("source-{asset_id}-{max_dimension}.png"))
    }

    pub fn render_preview_path(
        &self,
        project_id: Uuid,
        revision: u64,
        max_dimension: u32,
    ) -> PathBuf {
        self.render_preview_path_variant(project_id, revision, max_dimension, false)
    }

    /// `full` previews show the uncropped frame for the crop tool; they live
    /// beside the normal ones and are pruned the same way.
    pub fn render_preview_path_variant(
        &self,
        project_id: Uuid,
        revision: u64,
        max_dimension: u32,
        full_frame: bool,
    ) -> PathBuf {
        let suffix = if full_frame { "-full" } else { "" };
        self.previews_dir(project_id)
            .join(format!("render-r{revision}-d{max_dimension}{suffix}.png"))
    }

    pub fn ai_preview_path(&self, project_id: Uuid, asset_id: Uuid, max_dimension: u32) -> PathBuf {
        self.previews_dir(project_id)
            .join(format!("ai-{asset_id}-{max_dimension}.png"))
    }

    pub fn ai_result_path(&self, project_id: Uuid, job_id: Uuid) -> PathBuf {
        self.assets_dir(project_id).join(format!("ai-{job_id}.png"))
    }

    /// Drop everything but the newest `keep` render previews per dimension.
    ///
    /// Every revision that gets rendered writes a new `render-r{revision}-d{dim}.png`,
    /// so without this a single long editing session leaves hundreds of megabytes
    /// behind. The revision in the name is the ordering: modification times can
    /// tie inside one burst of slider commits.
    pub fn prune_render_previews(
        &self,
        project_id: Uuid,
        keep: usize,
        current_revision: Option<u64>,
    ) -> Result<usize, PersistenceError> {
        let keep = keep.max(1);
        let Ok(entries) = fs::read_dir(self.previews_dir(project_id)) else {
            return Ok(0);
        };
        let mut by_dimension: HashMap<u32, Vec<(u64, PathBuf)>> = HashMap::new();
        for entry in entries.flatten() {
            let Some((revision, dimension)) = parse_render_preview_name(&entry.file_name()) else {
                continue;
            };
            by_dimension
                .entry(dimension)
                .or_default()
                .push((revision, entry.path()));
        }
        let mut removed = 0usize;
        for mut group in by_dimension.into_values() {
            // A crash can leave previews from a newer in-memory revision than
            // the one persisted on disk. Those are unreachable: deleting them
            // stops them from consuming the keep budget and hiding the fresh
            // render from the canvas.
            if let Some(current) = current_revision {
                group.retain(|(revision, path)| {
                    if *revision > current {
                        if fs::remove_file(path).is_ok() {
                            removed += 1;
                        }
                        false
                    } else {
                        true
                    }
                });
            }
            if group.len() <= keep {
                continue;
            }
            group.sort_by_key(|(revision, _)| std::cmp::Reverse(*revision));
            for (_, path) in &group[keep..] {
                if fs::remove_file(path).is_ok() {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    pub fn imported_source_path(
        &self,
        project_id: Uuid,
        asset_id: Uuid,
        extension: &str,
    ) -> PathBuf {
        self.assets_dir(project_id)
            .join(format!("import-{asset_id}.{extension}"))
    }

    /// Managed copy of an imported `.cube` LUT. Stored per project so deleting
    /// the project also removes the look-up table.
    pub fn lut_path(&self, project_id: Uuid, asset_id: Uuid) -> PathBuf {
        self.assets_dir(project_id)
            .join(format!("lut-{asset_id}.cube"))
    }

    /// Embedded source ICC profile, kept so an export can be written back in
    /// the photo's own colour space.
    pub fn icc_path(&self, project_id: Uuid, asset_id: Uuid) -> PathBuf {
        self.assets_dir(project_id)
            .join(format!("profile-{asset_id}.icc"))
    }

    /// Write bytes atomically through a temp file in the destination directory.
    /// Parent directories are created for managed app data only.
    pub fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(bytes)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist(path)
            .map_err(|e| PersistenceError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }
}

/// Parse `render-r{revision}-d{dimension}.png` back into its parts.
fn parse_render_preview_name(name: &OsStr) -> Option<(u64, u32)> {
    let body = name
        .to_str()?
        .strip_suffix(".png")?
        .strip_prefix("render-r")?;
    let (revision, dimension) = body.split_once("-d")?;
    Some((revision.parse().ok()?, dimension.parse().ok()?))
}

/// Make a user-supplied file name safe as a single path component. This is
/// defense-in-depth: managed asset paths are UUID-based and never use user
/// names directly, but display names and explicit byte imports still sanitize.
pub fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let mut out = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let mut cleaned: String = out
        .trim()
        .trim_matches(|c| c == '.' || c == ' ')
        .chars()
        .take(120)
        .collect();
    while cleaned.ends_with('.') || cleaned.ends_with(' ') {
        cleaned.pop();
    }
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return "unnamed".to_string();
    }
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        cleaned.insert(0, '_');
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_path_traversal_and_reserved_names() {
        for input in [
            "../evil.png",
            "..\\evil.png",
            "C:\\Windows\\evil.png",
            "/etc/passwd",
            "..",
            ".",
            "",
            "CON.png",
            "bad\x00name.png",
        ] {
            let safe = sanitize_file_name(input);
            assert!(!safe.contains('/'), "{input} -> {safe}");
            assert!(!safe.contains('\\'), "{input} -> {safe}");
            assert!(!safe.contains('\0'), "{input} -> {safe}");
            assert_ne!(safe, "..");
            assert_eq!(
                Path::new(&safe).components().count(),
                1,
                "{input} -> {safe}"
            );
        }
        assert_eq!(sanitize_file_name("photo.png"), "photo.png");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
    }

    #[test]
    fn writes_atomically_and_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let store = AssetStore::new(dir.path().join("data"), dir.path().join("cache")).unwrap();
        let project_id = Uuid::new_v4();
        let path = store.imported_source_path(project_id, Uuid::new_v4(), "png");
        store.write_atomic(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        store.write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(
            store.source_preview_path(project_id, Uuid::nil(), 1600),
            store.source_preview_path(project_id, Uuid::nil(), 1600)
        );
        assert!(store
            .source_preview_path(project_id, Uuid::nil(), 1600)
            .to_string_lossy()
            .contains("previews"));
    }

    #[test]
    fn prune_keeps_newest_render_previews_per_dimension() {
        let dir = tempfile::tempdir().unwrap();
        let store = AssetStore::new(dir.path().join("data"), dir.path().join("cache")).unwrap();
        let project_id = Uuid::new_v4();
        store.ensure_project_dirs(project_id).unwrap();
        for revision in 0..6u64 {
            fs::write(
                store.render_preview_path(project_id, revision, 1600),
                b"png",
            )
            .unwrap();
        }
        for revision in 0..3u64 {
            fs::write(
                store.render_preview_path(project_id, revision, 1200),
                b"png",
            )
            .unwrap();
        }
        let source_preview = store.source_preview_path(project_id, Uuid::new_v4(), 1600);
        fs::write(&source_preview, b"png").unwrap();

        let removed = store.prune_render_previews(project_id, 2, None).unwrap();
        assert_eq!(removed, 5, "4 stale 1600 + 1 stale 1200");

        let name = |revision: u64, dimension: u32| {
            store
                .render_preview_path(project_id, revision, dimension)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        let kept: Vec<String> = fs::read_dir(store.previews_dir(project_id))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            kept.len(),
            5,
            "two render previews per dimension plus the untouched source preview"
        );
        for expected in [
            name(4, 1600),
            name(5, 1600),
            name(1, 1200),
            name(2, 1200),
            source_preview
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ] {
            assert!(kept.contains(&expected), "missing {expected}");
        }
        assert_eq!(
            store
                .prune_render_previews(Uuid::new_v4(), 2, None)
                .unwrap(),
            0,
            "a project without previews is a no-op"
        );
    }

    #[test]
    fn prune_removes_previews_from_a_newer_crashed_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = AssetStore::new(dir.path().join("data"), dir.path().join("cache")).unwrap();
        let project_id = Uuid::new_v4();
        store.ensure_project_dirs(project_id).unwrap();
        for revision in 0..6u64 {
            fs::write(
                store.render_preview_path(project_id, revision, 1600),
                b"png",
            )
            .unwrap();
        }
        // The persisted document is revision 2, but previews 3..5 were left by
        // a crashed session. They must not evict the fresh revision-2 render.
        let removed = store.prune_render_previews(project_id, 2, Some(2)).unwrap();
        assert_eq!(removed, 4, "3 stale newer files + 1 beyond the keep budget");
        let kept: Vec<u64> = fs::read_dir(store.previews_dir(project_id))
            .unwrap()
            .flatten()
            .filter_map(|entry| parse_render_preview_name(&entry.file_name()))
            .map(|(revision, _)| revision)
            .collect();
        assert!(kept.contains(&2), "the current revision must survive");
        assert!(kept.iter().all(|revision| *revision <= 2));
    }
}
