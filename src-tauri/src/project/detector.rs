//! 项目类型识别器

use crate::project::types::ProjectType;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// 项目类型检测器
pub struct ProjectDetector;

impl ProjectDetector {
    /// 检测项目类型（基于特征文件）
    ///
    /// 优先级：Node.js > Python > Rust > Go > Java > .NET > Generic
    pub fn detect(path: &Path) -> Result<ProjectType> {
        let (project_type, _) = Self::detect_with_name(path)?;
        Ok(project_type)
    }

    /// 检测项目类型并提取项目名称
    ///
    /// 返回：(ProjectType, 项目名称)
    /// 名称来源：
    /// - Node.js: package.json 的 "name" 字段
    /// - Rust: Cargo.toml 的 "package.name" 字段
    /// - Python: pyproject.toml 的 "project.name" 或 setup.py 的目录名
    /// - 其他：目录名
    pub fn detect_with_name(path: &Path) -> Result<(ProjectType, String)> {
        if !path.is_dir() {
            anyhow::bail!("Not a directory: {:?}", path);
        }

        // Node.js（优先级最高）
        if let Some(name) = Self::detect_nodejs(path)? {
            return Ok((ProjectType::NodeJs, name));
        }

        // Python
        if let Some(name) = Self::detect_python(path)? {
            return Ok((ProjectType::Python, name));
        }

        // Rust
        if let Some(name) = Self::detect_rust(path)? {
            return Ok((ProjectType::Rust, name));
        }

        // Go
        if let Some(name) = Self::detect_go(path)? {
            return Ok((ProjectType::Go, name));
        }

        // Java
        if let Some(name) = Self::detect_java(path)? {
            return Ok((ProjectType::Java, name));
        }

        // .NET
        if let Some(name) = Self::detect_dotnet(path)? {
            return Ok((ProjectType::Dotnet, name));
        }

        // Generic（无特征文件）
        let name = Self::extract_dir_name(path);
        Ok((ProjectType::Generic, name))
    }

    /// 验证是否是有效的项目根目录
    pub fn is_valid_project_root(path: &Path) -> bool {
        Self::detect(path).is_ok() && Self::detect(path).unwrap() != ProjectType::Generic
    }

    /// 检测 Node.js 项目
    fn detect_nodejs(path: &Path) -> Result<Option<String>> {
        let package_json = path.join("package.json");
        if !package_json.exists() {
            return Ok(None);
        }

        // 解析 package.json 获取项目名称
        let content = fs::read_to_string(&package_json)
            .context("Failed to read package.json")?;

        let json: serde_json::Value = serde_json::from_str(&content)
            .context("Failed to parse package.json")?;

        let name = if let Some(name_str) = json.get("name").and_then(|v| v.as_str()) {
            name_str.to_string()
        } else {
            Self::extract_dir_name(path)
        };

        Ok(Some(name))
    }

    /// 检测 Python 项目
    fn detect_python(path: &Path) -> Result<Option<String>> {
        // 优先 pyproject.toml
        let pyproject = path.join("pyproject.toml");
        if pyproject.exists() {
            let content = fs::read_to_string(&pyproject)
                .context("Failed to read pyproject.toml")?;

            // 简单解析 TOML（避免引入 toml crate，手动提取 name）
            if let Some(name) = Self::extract_toml_name(&content, "project.name") {
                return Ok(Some(name));
            }
            // 如果没有 name 字段，返回目录名
            return Ok(Some(Self::extract_dir_name(path)));
        }

        // requirements.txt
        if path.join("requirements.txt").exists() {
            return Ok(Some(Self::extract_dir_name(path)));
        }

        // setup.py
        if path.join("setup.py").exists() {
            return Ok(Some(Self::extract_dir_name(path)));
        }

        Ok(None)
    }

    /// 检测 Rust 项目
    fn detect_rust(path: &Path) -> Result<Option<String>> {
        let cargo_toml = path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&cargo_toml)
            .context("Failed to read Cargo.toml")?;

        // 提取 [package] name
        let name = Self::extract_toml_name(&content, "package.name")
            .unwrap_or_else(|| Self::extract_dir_name(path));

        Ok(Some(name))
    }

    /// 检测 Go 项目
    fn detect_go(path: &Path) -> Result<Option<String>> {
        let go_mod = path.join("go.mod");
        if !go_mod.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&go_mod)
            .context("Failed to read go.mod")?;

        // 提取 module 名称（第一行 "module xxx"）
        let name = content
            .lines()
            .find(|line| line.starts_with("module "))
            .and_then(|line| line.strip_prefix("module "))
            .map(|module_path| {
                // 提取最后一段作为项目名（例如 github.com/user/repo → repo）
                module_path.split('/').next_back().unwrap_or(module_path).to_string()
            })
            .unwrap_or_else(|| Self::extract_dir_name(path));

        Ok(Some(name))
    }

    /// 检测 Java 项目
    fn detect_java(path: &Path) -> Result<Option<String>> {
        // Maven
        let pom_xml = path.join("pom.xml");
        if pom_xml.exists() {
            let content = fs::read_to_string(&pom_xml)
                .context("Failed to read pom.xml")?;

            // 简单提取 <artifactId>
            let name = Self::extract_xml_tag(&content, "artifactId")
                .unwrap_or_else(|| Self::extract_dir_name(path));

            return Ok(Some(name));
        }

        // Gradle
        let build_gradle = path.join("build.gradle");
        let build_gradle_kts = path.join("build.gradle.kts");
        if build_gradle.exists() || build_gradle_kts.exists() {
            return Ok(Some(Self::extract_dir_name(path)));
        }

        Ok(None)
    }

    /// 检测 .NET 项目
    fn detect_dotnet(path: &Path) -> Result<Option<String>> {
        // 查找 *.csproj 或 *.sln
        let entries = fs::read_dir(path).context("Failed to read directory")?;

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            if file_name_str.ends_with(".csproj") {
                // 提取项目名（去掉 .csproj 后缀）
                let name = file_name_str.trim_end_matches(".csproj").to_string();
                return Ok(Some(name));
            }

            if file_name_str.ends_with(".sln") {
                // 提取解决方案名（去掉 .sln 后缀）
                let name = file_name_str.trim_end_matches(".sln").to_string();
                return Ok(Some(name));
            }
        }

        Ok(None)
    }

    /// 提取目录名称
    fn extract_dir_name(path: &Path) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    /// 从 TOML 内容中提取指定字段（简单解析）
    ///
    /// 示例：
    /// - extract_toml_name(content, "package.name") → 提取 [package] 下的 name
    /// - extract_toml_name(content, "project.name") → 提取 [project] 下的 name
    fn extract_toml_name(content: &str, field_path: &str) -> Option<String> {
        let parts: Vec<&str> = field_path.split('.').collect();
        if parts.len() != 2 {
            return None;
        }

        let section = parts[0];
        let key = parts[1];

        let mut in_section = false;
        for line in content.lines() {
            let line = line.trim();

            // 检查是否进入目标 section
            if line == format!("[{}]", section) {
                in_section = true;
                continue;
            }

            // 如果进入了其他 section，退出
            if in_section && line.starts_with('[') {
                break;
            }

            // 提取 key = "value"
            if in_section && line.starts_with(key) {
                if let Some(value) = line.split('=').nth(1) {
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    return Some(value.to_string());
                }
            }
        }

        None
    }

    /// 从 XML 内容中提取指定标签（简单解析）
    ///
    /// 示例：extract_xml_tag(content, "artifactId") → 提取 <artifactId>xxx</artifactId>
    fn extract_xml_tag(content: &str, tag: &str) -> Option<String> {
        let start_tag = format!("<{}>", tag);
        let end_tag = format!("</{}>", tag);

        if let Some(start_pos) = content.find(&start_tag) {
            let after_start = &content[start_pos + start_tag.len()..];
            if let Some(end_pos) = after_start.find(&end_tag) {
                let value = &after_start[..end_pos];
                return Some(value.trim().to_string());
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project(files: Vec<(&str, &str)>) -> TempDir {
        let temp = TempDir::new().unwrap();
        for (file, content) in files {
            let file_path = temp.path().join(file);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(file_path, content).unwrap();
        }
        temp
    }

    #[test]
    fn test_detect_nodejs() {
        let temp = create_test_project(vec![(
            "package.json",
            r#"{"name": "my-app", "version": "1.0.0"}"#,
        )]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::NodeJs);
        assert_eq!(name, "my-app");
    }

    #[test]
    fn test_detect_nodejs_without_name() {
        let temp = create_test_project(vec![("package.json", r#"{"version": "1.0.0"}"#)]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::NodeJs);
        // 名称应该是目录名
        assert!(!name.is_empty());
    }

    #[test]
    fn test_detect_python_pyproject() {
        let temp = create_test_project(vec![(
            "pyproject.toml",
            r#"
[project]
name = "my-python-app"
version = "0.1.0"
"#,
        )]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Python);
        assert_eq!(name, "my-python-app");
    }

    #[test]
    fn test_detect_python_requirements() {
        let temp = create_test_project(vec![("requirements.txt", "requests==2.28.0")]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Python);
        assert!(!name.is_empty());
    }

    #[test]
    fn test_detect_rust() {
        let temp = create_test_project(vec![(
            "Cargo.toml",
            r#"
[package]
name = "my-rust-app"
version = "0.1.0"
"#,
        )]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Rust);
        assert_eq!(name, "my-rust-app");
    }

    #[test]
    fn test_detect_go() {
        let temp = create_test_project(vec![(
            "go.mod",
            r#"
module github.com/user/my-go-app

go 1.19
"#,
        )]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Go);
        assert_eq!(name, "my-go-app");
    }

    #[test]
    fn test_detect_java_maven() {
        let temp = create_test_project(vec![(
            "pom.xml",
            r#"
<project>
    <artifactId>my-java-app</artifactId>
    <version>1.0.0</version>
</project>
"#,
        )]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Java);
        assert_eq!(name, "my-java-app");
    }

    #[test]
    fn test_detect_java_gradle() {
        let temp = create_test_project(vec![("build.gradle", "")]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Java);
        assert!(!name.is_empty());
    }

    #[test]
    fn test_detect_dotnet_csproj() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("MyApp.csproj"), "").unwrap();

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Dotnet);
        assert_eq!(name, "MyApp");
    }

    #[test]
    fn test_detect_dotnet_sln() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("MySolution.sln"), "").unwrap();

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Dotnet);
        assert_eq!(name, "MySolution");
    }

    #[test]
    fn test_detect_generic() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("README.md"), "").unwrap();

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Generic);
        assert!(!name.is_empty());
    }

    #[test]
    fn test_priority_nodejs_over_python() {
        // 同时包含 package.json 和 requirements.txt
        let temp = create_test_project(vec![
            ("package.json", r#"{"name": "hybrid-app"}"#),
            ("requirements.txt", "requests"),
        ]);

        let (project_type, name) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::NodeJs); // Node.js 优先级更高
        assert_eq!(name, "hybrid-app");
    }

    #[test]
    fn test_priority_python_over_rust() {
        // 同时包含 pyproject.toml 和 Cargo.toml
        let temp = create_test_project(vec![
            ("pyproject.toml", "[project]\nname = \"py-app\""),
            ("Cargo.toml", "[package]\nname = \"rust-app\""),
        ]);

        let (project_type, _) = ProjectDetector::detect_with_name(temp.path()).unwrap();
        assert_eq!(project_type, ProjectType::Python); // Python 优先级更高
    }

    #[test]
    fn test_is_valid_project_root() {
        let nodejs_temp = create_test_project(vec![("package.json", "{}")]);
        assert!(ProjectDetector::is_valid_project_root(nodejs_temp.path()));

        let generic_temp = TempDir::new().unwrap();
        assert!(!ProjectDetector::is_valid_project_root(generic_temp.path()));
    }

    #[test]
    fn test_detect_invalid_path() {
        let result = ProjectDetector::detect(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }
}
