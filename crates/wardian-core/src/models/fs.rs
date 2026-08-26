use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub extension: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirectoryTreeResult {
    pub nodes: Vec<FileNode>,
    pub truncated: bool,
    /// Offset for the next bounded page, when more children exist.
    pub next_offset: Option<usize>,
}
