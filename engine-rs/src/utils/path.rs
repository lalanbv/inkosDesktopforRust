//! 路径归一化与安全工具。
//!
//! 移植自 `packages/core/src/utils/posix-path.ts`（to_posix_path）+
//! `packages/core/src/utils/path-safety.ts`（safe_child_path）。

use std::path::{Component, Path, PathBuf};

/// 将项目相对路径归一化为 POSIX 分隔符（`/`）。
///
/// Windows 上 `node:path.join/relative` 产出 `\`，但下游消费者（manifest 持久化、
/// pipeline 结果、URL）不得出现 `\`。此函数在持久化/返回前统一转为 `/`。
///
/// 移植自 TS `toPosixPath(value) = value.replace(/\\/gu, "/")`。
pub fn to_posix_path(value: &str) -> String {
    value.replace('\\', "/")
}

/// 路径遍历防护：确保 `requested_path` 解析后仍落在 `root` 之内（含等于 root）。
///
/// 对齐 TS `safeChildPath`。返回逻辑 resolve 后的绝对路径；越界（`..` 逃逸 root 或绝对路径劫持）
/// 返回 `Err`（对齐 TS throw `Path traversal blocked`）。
///
/// **不实际 canonicalize**（TS `resolve` 是纯字符串逻辑，不要求路径存在）——用组件级
/// 逻辑归一（`.` 跳过、`..` 弹上层），避免对 fs 的依赖，使函数可纯单测。
pub fn safe_child_path(root: &str, requested_path: &str) -> Result<PathBuf, String> {
    let resolved_root = logical_resolve(Path::new(root));
    let resolved_path = logical_resolve(&resolved_root.join(requested_path));

    if is_same_or_descendant(&resolved_root, &resolved_path) {
        Ok(resolved_path)
    } else {
        Err(format!("Path traversal blocked: {requested_path}"))
    }
}

/// 逻辑 resolve：遍历组件，消解 `.`/`..`（不碰 fs）。对齐 TS `resolve` 的纯字符串语义。
fn logical_resolve(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::RootDir => {
                out = PathBuf::from("/");
            }
            Component::Prefix(p) => {
                out = PathBuf::from(p.as_os_str());
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    out
}

/// `descendant` 是否等于 `root` 或在其之下（按组件前缀匹配，防 "rootx" 假前缀）。
fn is_same_or_descendant(root: &Path, descendant: &Path) -> bool {
    let root_comps: Vec<_> = root.components().collect();
    let desc_comps: Vec<_> = descendant.components().collect();
    if desc_comps.len() < root_comps.len() {
        return false;
    }
    root_comps.iter().zip(desc_comps.iter()).all(|(r, d)| r == d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_unchanged() {
        assert_eq!(to_posix_path("a/b/c"), "a/b/c");
    }

    #[test]
    fn backslashes_converted() {
        assert_eq!(to_posix_path("a\\b\\c"), "a/b/c");
        assert_eq!(to_posix_path("C:\\Users\\inkos"), "C:/Users/inkos");
    }

    #[test]
    fn mixed_separators() {
        assert_eq!(to_posix_path("a\\b/c\\d"), "a/b/c/d");
    }

    #[test]
    fn empty_and_root() {
        assert_eq!(to_posix_path(""), "");
        assert_eq!(to_posix_path("\\"), "/");
    }

    #[test]
    fn safe_child_path_allows_descendant() {
        let r = safe_child_path("/book", "chapters/1.md").unwrap();
        assert_eq!(r, PathBuf::from("/book/chapters/1.md"));
    }

    #[test]
    fn safe_child_path_allows_dot_segments() {
        let r = safe_child_path("/book", "chapters/../chapters/2.md").unwrap();
        assert_eq!(r, PathBuf::from("/book/chapters/2.md"));
    }

    #[test]
    fn safe_child_path_blocks_traversal_escape() {
        // ../ 逃逸 root → Err。
        assert!(safe_child_path("/book", "../../etc/passwd").is_err());
        assert!(safe_child_path("/book", "chapters/../../etc").is_err());
    }

    #[test]
    fn safe_child_path_allows_root_equal() {
        // resolved == root（rel 为空）→ Ok(root)。
        let r = safe_child_path("/book", ".").unwrap();
        assert_eq!(r, PathBuf::from("/book"));
    }

    #[test]
    fn safe_child_path_blocks_absolute_override() {
        // 绝对路径劫持：requestedPath 是绝对路径，TS resolve(root, /abs) = /abs → 越界。
        assert!(safe_child_path("/book", "/etc/passwd").is_err());
    }
}
