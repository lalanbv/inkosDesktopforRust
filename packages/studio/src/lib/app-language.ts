// 全局应用语言：非 React 模块（store slice、parts-builder、error-copy 等）无法用
// useI18n hook，从这里读取。App.tsx 在项目配置加载/切换语言时调用 setAppLanguage 同步。
// 219 号：ja 为界面语言候选（tr 内联双语的 ja 暂回退 en——310 处调用点
// 的三语化另批渐进；current 为 "ja" 时 tr 走 en 分支）。
export type AppLanguage = "zh" | "en" | "ja";

let current: AppLanguage = "zh";

export function setAppLanguage(lang: AppLanguage): void {
  current = lang;
}

export function getAppLanguage(): AppLanguage {
  return current;
}

/** 内联双语：tr("中文", "English")。默认中文，保持既有测试与默认体验不变。 */
export function tr(zh: string, en: string): string {
  return current === "zh" ? zh : en;
}
