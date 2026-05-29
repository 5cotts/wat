use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum VfsError {
    NotFound(String),
    NotADirectory(String),
    IsADirectory(String),
    AlreadyExists(String),
    PermissionDenied(String),
    NotEmpty(String),
}

impl std::fmt::Display for VfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VfsError::NotFound(p) => write!(f, "{}: No such file or directory", p),
            VfsError::NotADirectory(p) => write!(f, "{}: Not a directory", p),
            VfsError::IsADirectory(p) => write!(f, "{}: Is a directory", p),
            VfsError::AlreadyExists(p) => write!(f, "{}: File exists", p),
            VfsError::PermissionDenied(p) => write!(f, "{}: Permission denied", p),
            VfsError::NotEmpty(p) => write!(f, "{}: Directory not empty", p),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    File,
    Dir,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
}

/// Filesystem abstraction used by both MemoryVfs (WASM) and NativeVfs (native).
pub trait Vfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError>;
    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), VfsError>;
    fn list(&self, path: &str) -> Result<Vec<DirEntry>, VfsError>;
    fn mkdir(&mut self, path: &str) -> Result<(), VfsError>;
    /// Remove a file or directory. `recursive` is required for non-empty dirs.
    fn remove(&mut self, path: &str, recursive: bool) -> Result<(), VfsError>;
    fn is_dir(&self, path: &str) -> bool;
    fn exists(&self, path: &str) -> bool;
    fn copy(&mut self, src: &str, dst: &str) -> Result<(), VfsError>;
    fn rename(&mut self, src: &str, dst: &str) -> Result<(), VfsError>;
}

// ── MemoryVfs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    File { content: Vec<u8> },
    Dir { entries: BTreeMap<String, Node> },
}

impl Node {
    fn is_dir(&self) -> bool {
        matches!(self, Node::Dir { .. })
    }
}

/// An in-memory filesystem tree. Used as the WASM VFS and in tests.
pub struct MemoryVfs {
    root: Node,
}

impl MemoryVfs {
    pub fn new() -> Self {
        MemoryVfs { root: Node::Dir { entries: BTreeMap::new() } }
    }

    /// Create a seeded VFS with the personality layout for the Zo Site.
    pub fn new_seeded() -> Self {
        let mut vfs = Self::new();
        vfs.mkdir("/home").unwrap();
        vfs.mkdir("/home/5cotts").unwrap();
        vfs.mkdir("/etc").unwrap();
        vfs.write(
            "/home/5cotts/whoami.sh",
            b"#!/bin/sh\necho i am scott\n",
        )
        .unwrap();
        vfs.write(
            "/home/5cotts/.hints",
            b"You found the hints file!\n\
              Try: cat /etc/motd\n\
              Try: ./whoami.sh\n\
              Try: sudo rm -rf /\n\
              There is a konami code...\n",
        )
        .unwrap();
        vfs.write(
            "/etc/motd",
            b"Welcome to wat - a small shell that compiles to WebAssembly.\n\
              Type `help` to get started.\n",
        )
        .unwrap();
        vfs
    }

    /// Walk the path components and return a mutable reference to the node.
    fn get_node(&self, path: &str) -> Option<&Node> {
        let path = normalize(path);
        if path == "/" {
            return Some(&self.root);
        }
        let mut current = &self.root;
        for part in path.trim_start_matches('/').split('/') {
            if part.is_empty() {
                continue;
            }
            match current {
                Node::Dir { entries } => match entries.get(part) {
                    Some(n) => current = n,
                    None => return None,
                },
                _ => return None,
            }
        }
        Some(current)
    }

    fn get_node_mut(&mut self, path: &str) -> Option<&mut Node> {
        let path = normalize(path);
        if path == "/" {
            return Some(&mut self.root);
        }
        let mut current = &mut self.root;
        for part in path.trim_start_matches('/').split('/') {
            if part.is_empty() {
                continue;
            }
            match current {
                Node::Dir { entries } => match entries.get_mut(part) {
                    Some(n) => current = n,
                    None => return None,
                },
                _ => return None,
            }
        }
        Some(current)
    }

    fn parent_and_name(path: &str) -> (String, String) {
        let path = normalize(path);
        match path.rsplit_once('/') {
            Some(("", name)) => ("/".to_string(), name.to_string()),
            Some((parent, name)) => (parent.to_string(), name.to_string()),
            None => ("/".to_string(), path),
        }
    }
}

fn normalize(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

impl Default for MemoryVfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs for MemoryVfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        match self.get_node(path) {
            Some(Node::File { content }) => Ok(content.clone()),
            Some(Node::Dir { .. }) => Err(VfsError::IsADirectory(path.to_string())),
            None => Err(VfsError::NotFound(path.to_string())),
        }
    }

    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), VfsError> {
        let (parent, name) = Self::parent_and_name(path);
        match self.get_node_mut(&parent) {
            Some(Node::Dir { entries }) => {
                entries.insert(name, Node::File { content: content.to_vec() });
                Ok(())
            }
            Some(_) => Err(VfsError::NotADirectory(parent)),
            None => Err(VfsError::NotFound(parent)),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>, VfsError> {
        match self.get_node(path) {
            Some(Node::Dir { entries }) => Ok(entries
                .iter()
                .map(|(name, node)| DirEntry {
                    name: name.clone(),
                    file_type: if node.is_dir() { FileType::Dir } else { FileType::File },
                })
                .collect()),
            Some(_) => Err(VfsError::NotADirectory(path.to_string())),
            None => Err(VfsError::NotFound(path.to_string())),
        }
    }

    fn mkdir(&mut self, path: &str) -> Result<(), VfsError> {
        let (parent, name) = Self::parent_and_name(path);
        match self.get_node_mut(&parent) {
            Some(Node::Dir { entries }) => {
                if entries.contains_key(&name) {
                    return Err(VfsError::AlreadyExists(path.to_string()));
                }
                entries.insert(name, Node::Dir { entries: BTreeMap::new() });
                Ok(())
            }
            Some(_) => Err(VfsError::NotADirectory(parent)),
            None => Err(VfsError::NotFound(parent)),
        }
    }

    fn remove(&mut self, path: &str, recursive: bool) -> Result<(), VfsError> {
        let (parent, name) = Self::parent_and_name(path);
        match self.get_node_mut(&parent) {
            Some(Node::Dir { entries }) => match entries.get(&name) {
                Some(Node::Dir { entries: child_entries }) => {
                    if !recursive && !child_entries.is_empty() {
                        return Err(VfsError::NotEmpty(path.to_string()));
                    }
                    entries.remove(&name);
                    Ok(())
                }
                Some(_) => {
                    entries.remove(&name);
                    Ok(())
                }
                None => Err(VfsError::NotFound(path.to_string())),
            },
            Some(_) => Err(VfsError::NotADirectory(parent)),
            None => Err(VfsError::NotFound(path.to_string())),
        }
    }

    fn is_dir(&self, path: &str) -> bool {
        matches!(self.get_node(path), Some(Node::Dir { .. }))
    }

    fn exists(&self, path: &str) -> bool {
        self.get_node(path).is_some()
    }

    fn copy(&mut self, src: &str, dst: &str) -> Result<(), VfsError> {
        let content = self.read(src)?;
        self.write(dst, &content)
    }

    fn rename(&mut self, src: &str, dst: &str) -> Result<(), VfsError> {
        let content = self.read(src)?;
        self.write(dst, &content)?;
        let (parent, name) = Self::parent_and_name(src);
        if let Some(Node::Dir { entries }) = self.get_node_mut(&parent) {
            entries.remove(&name);
        }
        Ok(())
    }
}

// ── NativeVfs ─────────────────────────────────────────────────────────────

#[cfg(feature = "native-fs")]
pub struct NativeVfs;

#[cfg(feature = "native-fs")]
impl NativeVfs {
    pub fn new() -> Self {
        NativeVfs
    }
}

#[cfg(feature = "native-fs")]
impl Default for NativeVfs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "native-fs")]
impl Vfs for NativeVfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        std::fs::read(path).map_err(|_| VfsError::NotFound(path.to_string()))
    }

    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), VfsError> {
        std::fs::write(path, content).map_err(|_| VfsError::PermissionDenied(path.to_string()))
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>, VfsError> {
        let rd = std::fs::read_dir(path).map_err(|_| VfsError::NotFound(path.to_string()))?;
        let mut entries = Vec::new();
        for e in rd.flatten() {
            let ft = e.file_type().ok();
            entries.push(DirEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                file_type: if ft.map(|f| f.is_dir()).unwrap_or(false) {
                    FileType::Dir
                } else {
                    FileType::File
                },
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), VfsError> {
        std::fs::create_dir_all(path)
            .map_err(|_| VfsError::PermissionDenied(path.to_string()))
    }

    fn remove(&mut self, path: &str, recursive: bool) -> Result<(), VfsError> {
        let meta =
            std::fs::metadata(path).map_err(|_| VfsError::NotFound(path.to_string()))?;
        if meta.is_dir() {
            if recursive {
                std::fs::remove_dir_all(path)
                    .map_err(|_| VfsError::PermissionDenied(path.to_string()))
            } else {
                std::fs::remove_dir(path).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                        VfsError::NotEmpty(path.to_string())
                    } else {
                        VfsError::PermissionDenied(path.to_string())
                    }
                })
            }
        } else {
            std::fs::remove_file(path)
                .map_err(|_| VfsError::PermissionDenied(path.to_string()))
        }
    }

    fn is_dir(&self, path: &str) -> bool {
        std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
    }

    fn exists(&self, path: &str) -> bool {
        std::fs::metadata(path).is_ok()
    }

    fn copy(&mut self, src: &str, dst: &str) -> Result<(), VfsError> {
        std::fs::copy(src, dst)
            .map(|_| ())
            .map_err(|_| VfsError::NotFound(src.to_string()))
    }

    fn rename(&mut self, src: &str, dst: &str) -> Result<(), VfsError> {
        std::fs::rename(src, dst).map_err(|_| VfsError::PermissionDenied(src.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> MemoryVfs {
        MemoryVfs::new()
    }

    #[test]
    fn mkdir_and_list() {
        let mut vfs = fresh();
        vfs.mkdir("/foo").unwrap();
        let entries = vfs.list("/").unwrap();
        assert!(entries.iter().any(|e| e.name == "foo" && e.file_type == FileType::Dir));
    }

    #[test]
    fn write_and_read() {
        let mut vfs = fresh();
        vfs.write("/hello.txt", b"hello").unwrap();
        assert_eq!(vfs.read("/hello.txt").unwrap(), b"hello");
    }

    #[test]
    fn nested_mkdir() {
        let mut vfs = fresh();
        vfs.mkdir("/a").unwrap();
        vfs.mkdir("/a/b").unwrap();
        assert!(vfs.is_dir("/a/b"));
    }

    #[test]
    fn write_in_subdir() {
        let mut vfs = fresh();
        vfs.mkdir("/home").unwrap();
        vfs.write("/home/test.txt", b"data").unwrap();
        assert_eq!(vfs.read("/home/test.txt").unwrap(), b"data");
    }

    #[test]
    fn remove_file() {
        let mut vfs = fresh();
        vfs.write("/f.txt", b"x").unwrap();
        vfs.remove("/f.txt", false).unwrap();
        assert!(!vfs.exists("/f.txt"));
    }

    #[test]
    fn remove_empty_dir() {
        let mut vfs = fresh();
        vfs.mkdir("/empty").unwrap();
        vfs.remove("/empty", false).unwrap();
        assert!(!vfs.exists("/empty"));
    }

    #[test]
    fn remove_nonempty_without_recursive_fails() {
        let mut vfs = fresh();
        vfs.mkdir("/nonempty").unwrap();
        vfs.write("/nonempty/f", b"x").unwrap();
        assert!(matches!(vfs.remove("/nonempty", false), Err(VfsError::NotEmpty(_))));
    }

    #[test]
    fn remove_recursive() {
        let mut vfs = fresh();
        vfs.mkdir("/d").unwrap();
        vfs.write("/d/f", b"x").unwrap();
        vfs.remove("/d", true).unwrap();
        assert!(!vfs.exists("/d"));
    }

    #[test]
    fn read_dir_as_file_errors() {
        let mut vfs = fresh();
        vfs.mkdir("/d").unwrap();
        assert!(matches!(vfs.read("/d"), Err(VfsError::IsADirectory(_))));
    }

    #[test]
    fn not_found_errors() {
        let vfs = fresh();
        assert!(matches!(vfs.read("/nope"), Err(VfsError::NotFound(_))));
        assert!(matches!(vfs.list("/nope"), Err(VfsError::NotFound(_))));
    }

    #[test]
    fn copy_file() {
        let mut vfs = fresh();
        vfs.write("/src.txt", b"data").unwrap();
        vfs.copy("/src.txt", "/dst.txt").unwrap();
        assert_eq!(vfs.read("/dst.txt").unwrap(), b"data");
        assert!(vfs.exists("/src.txt")); // original still there
    }

    #[test]
    fn rename_file() {
        let mut vfs = fresh();
        vfs.write("/old.txt", b"data").unwrap();
        vfs.rename("/old.txt", "/new.txt").unwrap();
        assert_eq!(vfs.read("/new.txt").unwrap(), b"data");
        assert!(!vfs.exists("/old.txt"));
    }

    #[test]
    fn seeded_vfs_has_motd() {
        let vfs = MemoryVfs::new_seeded();
        let motd = vfs.read("/etc/motd").unwrap();
        assert!(!motd.is_empty());
    }

    #[test]
    fn seeded_vfs_has_home_dir() {
        let vfs = MemoryVfs::new_seeded();
        assert!(vfs.is_dir("/home/5cotts"));
    }

    #[test]
    fn seeded_vfs_has_hints() {
        let vfs = MemoryVfs::new_seeded();
        assert!(vfs.exists("/home/5cotts/.hints"));
    }
}
