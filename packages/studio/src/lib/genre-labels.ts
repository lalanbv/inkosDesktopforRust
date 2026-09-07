/**
 * Genre id → 双语显示名（207 号）：消除书卡/工作台的原始枚举暴露
 * （「OTHER」/「urban」）。id 集合对齐 `packages/core/genres/` 内置题材
 * 目录；未知 id（自定义题材）回退原值展示。
 */
const GENRE_LABELS: Record<string, { zh: string; en: string }> = {
  cozy: { zh: "治愈系", en: "Cozy" },
  cultivation: { zh: "修真", en: "Cultivation" },
  "dungeon-core": { zh: "地下城核心", en: "Dungeon Core" },
  horror: { zh: "恐怖", en: "Horror" },
  isekai: { zh: "异世界", en: "Isekai" },
  litrpg: { zh: "数值流", en: "LitRPG" },
  other: { zh: "其他", en: "Other" },
  progression: { zh: "成长流", en: "Progression" },
  romantasy: { zh: "浪漫奇幻", en: "Romantasy" },
  "sci-fi": { zh: "科幻", en: "Sci-Fi" },
  "system-apocalypse": { zh: "系统末日", en: "System Apocalypse" },
  "tower-climber": { zh: "爬塔流", en: "Tower Climber" },
  urban: { zh: "都市", en: "Urban" },
  xianxia: { zh: "仙侠", en: "Xianxia" },
  xuanhuan: { zh: "玄幻", en: "Xuanhuan Fantasy" },
};

export function genreLabel(genreId: string, lang: "zh" | "en"): string {
  const entry = GENRE_LABELS[genreId];
  if (!entry) return genreId;
  return lang === "en" ? entry.en : entry.zh;
}
