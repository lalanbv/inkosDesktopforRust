//! 路径归一化工具。
//!
//! 移植自 `packages/core/src/utils/posix-path.ts`（6 行，纯函数）。

/// 将项目相对路径归一化为 POSIX 分隔符（`/`）。
///
/// Windows 上 `node:path.join/relative` 产出 `\`，但下游消费者（manifest 持久化、
/// pipeline 结果、URL）不得出现 `\`。此函数在持久化/返回前统一转为 `/`。
///
/// 移植自 TS `toPosixPath(value) = value.replace(/\\/gu, "/")`。
pub fn to_posix_path(value: &str) -> String {
    value.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_unchanged() {
        assert_eq!(to_posix_path("a/b/c"), "a/b/c");
        assert_eq!(to_posix_path("/abs/path"), "/abs/path");
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
}
