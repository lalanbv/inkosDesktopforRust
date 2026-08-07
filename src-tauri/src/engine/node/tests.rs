    use super::*;

    #[test]
    fn platformarch_detect_matches_target() {
        let pa = PlatformArch::detect();
        if cfg!(target_os = "macos") {
            assert_eq!(pa.os, "darwin");
            assert_eq!(pa.archive_ext, "tar.gz");
        } else if cfg!(target_os = "windows") {
            assert_eq!(pa.os, "win");
            assert_eq!(pa.archive_ext, "zip");
        } else {
            assert_eq!(pa.os, "linux");
        }
        assert!(pa.arch == "x64" || pa.arch == "arm64");
    }

    #[test]
    fn tarball_name_matches_node_dist_convention() {
        let pa = PlatformArch { os: "darwin", arch: "arm64", archive_ext: "tar.gz" };
        assert_eq!(pa.tarball_name("22.11.0"), "node-v22.11.0-darwin-arm64.tar.gz");
        let win = PlatformArch { os: "win", arch: "x64", archive_ext: "zip" };
        assert_eq!(win.tarball_name("22.11.0"), "node-v22.11.0-win-x64.zip");
    }

    #[test]
    fn node_bin_rel_unix_and_windows() {
        let unix = PlatformArch { os: "linux", arch: "x64", archive_ext: "tar.gz" };
        assert_eq!(
            BootstrappingResolver::node_bin_rel(&unix, "22.11.0"),
            PathBuf::from("node-v22.11.0-linux-x64/bin/node")
        );
        let win = PlatformArch { os: "win", arch: "x64", archive_ext: "zip" };
        assert_eq!(
            BootstrappingResolver::node_bin_rel(&win, "22.11.0"),
            PathBuf::from("node-v22.11.0-win-x64/node.exe")
        );
    }

    #[test]
    fn binary_url_and_shasums_url_format() {
        assert_eq!(
            binary_url("https://nodejs.org/dist", "22.11.0", "node-v22.11.0-darwin-arm64.tar.gz"),
            "https://nodejs.org/dist/v22.11.0/node-v22.11.0-darwin-arm64.tar.gz"
        );
        assert_eq!(shasums_url("22.11.0"), "https://nodejs.org/dist/v22.11.0/SHASUMS256.txt");
    }

    #[test]
    fn parse_shasums_basic_and_takes_basename() {
        let text = "abc123def4567890abcdef1234567890abcdef1234567890abcdef1234567890  node-v22.11.0-darwin-arm64.tar.gz\n\
                    000000000000000000000000000000000000000000000000000000000000abcd  */node-v22.11.0-linux-x64.tar.gz\n\
                    \n\
                    not-a-hash-line foo\n";
        let m = parse_shasums(text);
        assert_eq!(
            m.get("node-v22.11.0-darwin-arm64.tar.gz").unwrap(),
            "abc123def4567890abcdef1234567890abcdef1234567890abcdef1234567890"
        );
        assert_eq!(
            m.get("node-v22.11.0-linux-x64.tar.gz").unwrap(),
            "000000000000000000000000000000000000000000000000000000000000abcd"
        );
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn parse_shasums_empty_and_garbage() {
        assert!(parse_shasums("").is_empty());
        assert!(parse_shasums("garbage\nmore garbage\n").is_empty());
    }

    #[tokio::test]
    async fn bootstrapping_resolver_cache_hit_skips_network() {
        // 预置缓存：node bin 已存在 → resolve 直接返回，不发网络。
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().to_path_buf();
        let pa = PlatformArch::detect();
        let key = pa.cache_key(NODE_BOOTSTRAP_VERSION);
        let bin = cache
            .join(&key)
            .join(BootstrappingResolver::node_bin_rel(&pa, NODE_BOOTSTRAP_VERSION));
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, b"fake node").unwrap();

        struct NoNet;
        impl MirrorSelector for NoNet {
            fn binary_mirrors(&self) -> Vec<&'static str> {
                vec![] // 缓存命中不应触达
            }
        }
        let r = BootstrappingResolver::new(cache, Box::new(NoNet));
        assert_eq!(r.resolve().await.unwrap(), bin);
    }

    #[test]
    fn short_host_strips_scheme_and_path() {
        assert_eq!(short_host("https://cdn.npmmirror.com/binaries/node"), "cdn.npmmirror.com");
        assert_eq!(short_host("https://nodejs.org/dist"), "nodejs.org");
    }

    // 真实下载冒烟（需网络 + nodejs.org 可达）。CI 可选跑。
    #[tokio::test]
    #[ignore]
    async fn real_bootstrap_downloads_and_verifies() {
        let tmp = tempfile::tempdir().unwrap();
        let r = BootstrappingResolver::new(tmp.path().to_path_buf(), Box::new(LocaleMirrorSelector));
        let bin = r.resolve().await.expect("bootstrap 应成功");
        assert!(bin.is_file(), "node 二进制应存在: {}", bin.display());
    }

    /// 构造 tar.gz：`(路径, 内容, unix mode)` 列表。
    fn build_tar_gz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        for (path, content, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// `unpack_in` 逐条目解压必须保持与 `unpack()` 相同的目录结构与权限位——
    /// `bin/node` 丢掉可执行位会让 bootstrap 静默产出不可用的 node。
    #[test]
    fn extract_archive_preserves_layout_and_exec_bit() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball = tmp.path().join("t.tar.gz");
        std::fs::write(
            &tarball,
            build_tar_gz(&[
                ("node-v22.11.0-darwin-arm64/bin/node", b"#!/bin/sh\n", 0o755),
                ("node-v22.11.0-darwin-arm64/README.md", b"hi", 0o644),
            ]),
        )
        .unwrap();

        let dest = tmp.path().join("out");
        extract_archive(&tarball, &dest).expect("解压应成功");

        let node_bin = dest.join("node-v22.11.0-darwin-arm64/bin/node");
        assert!(node_bin.is_file(), "顶层目录结构应保留");
        assert!(
            dest.join("node-v22.11.0-darwin-arm64/README.md").is_file(),
            "同级文件应解出"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&node_bin).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "bin/node 须保留可执行位，实际 {mode:o}");
        }
    }

    /// 声明总量超 MAX_EXTRACTED_BYTES 即中止（gz bomb 防护）。
    /// header.size 是稀疏声明，无需真实写入 512MiB。
    #[test]
    fn extract_archive_rejects_oversized_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball = tmp.path().join("bomb.tar.gz");

        // 单条目声明 600 MiB > 512 MiB 上限：首次累计检查即中止。
        // 用单条目而非多条目——多条目时 tar-rs 读下一条需先跳过前一条声明的
        // 数据量，内容为空会先报 "unexpected EOF during skip"，掩盖体积检查。
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(600 * 1024 * 1024);
        header.set_mode(0o644);
        header.set_path("huge.bin").unwrap();
        header.set_cksum();
        // header 声明大小远超实际内容，正是压缩炸弹的构造特征
        builder.append(&header, std::io::empty()).unwrap();
        std::fs::write(&tarball, builder.into_inner().unwrap().finish().unwrap()).unwrap();

        let err = extract_archive(&tarball, &tmp.path().join("out"))
            .expect_err("超上限应被拒绝");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("压缩炸弹") || msg.contains("超上限"),
            "错误应指明体积超限，实际: {msg}"
        );
    }
