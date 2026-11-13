//! 桌面壳用户直显面语言机制（1009 号）。
//!
//! ## 为什么双语直出而非 EN 基准+前端映射（1005/1006 政策边界定性）
//!
//! AppError 的 message/suggestion、dialog 标题、系统通知是**纯展示面**：
//! 前端（picker/settings）按 `kind` 分类渲染，从不解析 message 内容做判定
//! ——没有协议判定面，迂回 EN 基准化只增加映射表负担。协议字面 EN 化政策
//! （write-stopped 1005 / provider 诊断 1006 / 409 1004）针对的是「判定面」；
//! 展示面直接按 locale 输出双语（对齐 cli TuiCopy 双语表形态）。
//!
//! ## locale 来源与默认
//!
//! 前端 `navigator.language`（picker/settings 启动即上报 cmd_set_locale）——
//! 与前端字典同一信号源；未上报时默认 Zh（原行为，零行为漂移）。

use std::sync::atomic::{AtomicU8, Ordering};

/// 桌面壳 UI 语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    /// 中文（默认——与前端 `LANG` 检测的 zh 回落一致）
    Zh,
    /// English
    En,
}

impl Locale {
    /// 解析前端上报的 locale 标签（navigator.language 形态，如 "zh-CN"/"en-US"）。
    pub fn parse(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("en") {
            Locale::En
        } else {
            Locale::Zh
        }
    }
}

// 0=Zh 1=En；未知值回落 Zh（与 parse 的非 en 即 zh 判据一致）。
static LOCALE: AtomicU8 = AtomicU8::new(0);

/// 设置全局 UI 语言（cmd_set_locale 落点；前端启动即上报）。
pub fn set_locale(locale: Locale) {
    LOCALE.store(locale as u8, Ordering::Relaxed);
}

/// 当前 UI 语言（未上报时默认 Zh）。
pub fn current_locale() -> Locale {
    match LOCALE.load(Ordering::Relaxed) {
        1 => Locale::En,
        _ => Locale::Zh,
    }
}

/// 按当前 locale 选择文案：`pick("中文", "English")`。
pub fn pick(zh: &'static str, en: &'static str) -> &'static str {
    match current_locale() {
        Locale::En => en,
        Locale::Zh => zh,
    }
}

/// 动态拼接文案的 owned 形态（含变量的 format! 串——两支都求值，调用点稀少）。
pub fn pick_owned(zh: String, en: String) -> String {
    match current_locale() {
        Locale::En => en,
        Locale::Zh => zh,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// locale 全局态测试互斥锁——LOCALE 是进程级单例，cargo test 并行线程
    /// 下「设 locale → 断言」窗口会被其它 locale 敏感用例交错（1009 号实测
    /// test_error_serialization 单跑过/全跑挂）。所有设 locale 或断言
    /// locale 敏感文案的用例必须持锁到用例结束。
    pub(crate) fn test_locale_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // 测试间共享进程级全局态：每个用例先固定起点再断言，避免顺序耦合。
    #[test]
    fn 默认_locale_为_zh_且_pick_选中文案() {
        let _g = test_locale_lock();
        set_locale(Locale::Zh);
        assert_eq!(current_locale(), Locale::Zh);
        assert_eq!(pick("中文", "English"), "中文");
    }

    #[test]
    fn set_en_后_pick_选英文案() {
        let _g = test_locale_lock();
        set_locale(Locale::En);
        assert_eq!(current_locale(), Locale::En);
        assert_eq!(pick("中文", "English"), "English");
        // 复位，避免影响其它用例的全局态
        set_locale(Locale::Zh);
    }

    #[test]
    fn parse_遵循_en_前缀判据且忽略大小写与地区后缀() {
        assert_eq!(Locale::parse("en-US"), Locale::En);
        assert_eq!(Locale::parse("EN"), Locale::En);
        assert_eq!(Locale::parse("en-GB"), Locale::En);
        assert_eq!(Locale::parse("zh-CN"), Locale::Zh);
        assert_eq!(Locale::parse("ja-JP"), Locale::Zh); // 非 en 均回落 zh（桌面壳仅双语）
        assert_eq!(Locale::parse(""), Locale::Zh);
    }

    // 1009 号：picker 静态页直显 CJK 残留卫兵（零预算）。扫描域=<script> 块
    // 内（HTML 静态直显由 data-i18n 机制管，多行 HTML/CSS 注释中间行无法行级
    // 识别——裁域避开形态学盲区）；判据=含 CJK 且非字典行（zh 默认值
    // `": "` 形态）/非注释行/非 console 日志。台账判据盲区备案：JS 对象字面
    // 量直显（`key: "中文"`）与字典行同形会被豁免——picker 域已函数化清零
    // （CAP/TYPE 表 1009 号改 t() 取值），新增时勿用对象字面量直显。
    #[test]
    fn picker_静态页直显_cjk_残留为零() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        for page in ["index.html", "settings.html"] {
            let path = std::path::Path::new(manifest).join("picker").join(page);
            let body = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
            // 裁域：只保留 <script> 与 </script> 之间的行（单 script 块/页）
            let mut in_script = false;
            let residues: Vec<(usize, String)> = body
                .lines()
                .enumerate()
                .filter(|(_, line)| {
                    let trimmed = line.trim();
                    if trimmed.starts_with("<script>") {
                        in_script = true;
                        return false;
                    }
                    if trimmed.starts_with("</script>") {
                        in_script = false;
                        return false;
                    }
                    in_script
                })
                .filter(|(_, line)| {
                    let has_cjk = line
                        .chars()
                        .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
                    if !has_cjk {
                        return false;
                    }
                    let trimmed = line.trim();
                    // 残留 = 含 CJK 且不属合规形态：字典条目（zh 值）/ 注释 / console
                    !(trimmed.contains("\": \"")
                        || trimmed.starts_with("//")
                        || trimmed.starts_with("/*")
                        || trimmed.starts_with("*")
                        || trimmed.ends_with("*/")
                        || trimmed.contains("// ")
                        || trimmed.contains("console."))
                })
                .map(|(i, line)| (i + 1, line.trim().to_string()))
                .collect();
            assert!(
                residues.is_empty(),
                "{page} 存在字典外 CJK 直显残留（进 I18N 字典或 t() 化）：\n{residues:#?}"
            );
        }
    }
}
