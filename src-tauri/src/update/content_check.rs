//! Content fingerprint evaluation shared by global Git and project checks.

use super::archive::SourceArchive;
use super::content_hash::{self, FolderHashError};
use super::parent_path;
use crate::inventory::UpdateState;

#[derive(Debug, Clone)]
pub(crate) struct ContentTarget {
    pub name: String,
    pub skill_path: String,
    pub lock_hash: String,
}

pub(crate) struct Evaluation {
    pub state: UpdateState,
    pub remote_hash: Option<String>,
    pub note: Option<String>,
}

pub(crate) fn evaluate_target(target: &ContentTarget, archive: &SourceArchive) -> Evaluation {
    let folder = if archive.contains_file(&target.skill_path) {
        parent_path(&target.skill_path)
    } else if archive.contains_folder(&target.skill_path) {
        target.skill_path.clone()
    } else {
        return Evaluation {
            state: UpdateState::PathMissing,
            remote_hash: None,
            note: Some("来源中已找不到记录的 skillPath，可能已被删除或移动。".to_string()),
        };
    };
    let entries = archive.files_under(&folder);
    match content_hash::hash_entries(entries) {
        Ok(remote_hash) => {
            let state = if remote_hash.eq_ignore_ascii_case(&target.lock_hash) {
                UpdateState::Current
            } else {
                UpdateState::Update
            };
            Evaluation {
                state,
                remote_hash: Some(remote_hash),
                note: None,
            }
        }
        Err(FolderHashError::UnsupportedPath(detail)) => Evaluation {
            state: UpdateState::Unknown,
            remote_hash: None,
            note: Some(format!(
                "skill 文件夹 {detail}，无法用 CLI 的排序规则计算内容指纹。"
            )),
        },
        Err(FolderHashError::Io(detail)) => Evaluation {
            state: UpdateState::Unknown,
            remote_hash: None,
            note: Some(format!("无法读取来源文件：{detail}")),
        },
    }
}
