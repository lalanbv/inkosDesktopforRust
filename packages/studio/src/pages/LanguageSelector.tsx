import { useState } from "react";
import { Dialog, DialogContent } from "@/components/ui/dialog";

/**
 * 首启语言选择（project.languageExplicit=false 时弹出）。
 * P1-3 起改为骨架壳之上的强制 Dialog：不可点外/ESC 关闭，必须选一门语言。
 * 卡片内容抽成 LanguageOptions 便于 SSR 冒烟测试（Portal 内容不参与 SSR）。
 */
export function LanguageSelector({ onSelect }: { onSelect: (lang: "zh" | "en") => void }) {
  const [selected, setSelected] = useState<"zh" | "en" | null>(null);

  const handleSelect = (lang: "zh" | "en") => {
    if (selected) return;
    setSelected(lang);
    // Brief pause for the selection animation before transitioning
    setTimeout(() => onSelect(lang), 400);
  };

  return (
    <Dialog open disablePointerDismissal>
      <DialogContent
        showCloseButton={false}
        className="w-fit gap-8 p-10 sm:max-w-[760px]"
      >
        <LanguageOptions selected={selected} onSelect={handleSelect} />
      </DialogContent>
    </Dialog>
  );
}

export function LanguageOptions({ selected, onSelect }: {
  selected: "zh" | "en" | null;
  onSelect: (lang: "zh" | "en") => void;
}) {
  return (
    <div className="flex flex-col gap-8" data-slot="language-selector">
      {/* Logo — cinematic scale */}
        <div className="text-center">
          <div className="flex items-baseline justify-center gap-1.5 mb-4">
            <span className="font-serif text-6xl italic text-primary">Ink</span>
            <span className="text-5xl font-semibold tracking-tight text-foreground">OS</span>
          </div>
          <div className="text-base text-muted-foreground tracking-widest uppercase">Studio</div>
        </div>

        {/* Language cards — generous, distinct, immersive */}
        <div className="flex gap-8">
          <button
            data-language="zh"
            onClick={() => onSelect("zh")}
            className={`group w-80 border rounded-lg p-10 text-left transition-all duration-300 ${
              selected === "zh"
                ? "border-primary bg-primary/10 scale-[1.02]"
                : selected
                  ? "border-border bg-card/50 opacity-50"
                  : "border-border bg-card/50 hover:border-primary/50 hover:bg-card"
            }`}
          >
            <div className="font-serif text-3xl mb-4 text-foreground">中文创作</div>
            <div className="text-base text-foreground/70 leading-relaxed mb-6">
              玄幻 · 仙侠 · 都市 · 恐怖 · 通用
            </div>
            <div className="text-sm text-muted-foreground">
              番茄小说 · 起点中文网 · 飞卢
            </div>
          </button>

          <button
            data-language="en"
            onClick={() => onSelect("en")}
            className={`group w-80 border rounded-lg p-10 text-left transition-all duration-300 ${
              selected === "en"
                ? "border-primary bg-primary/10 scale-[1.02]"
                : selected
                  ? "border-border bg-card/50 opacity-50"
                  : "border-border bg-card/50 hover:border-primary/50 hover:bg-card"
            }`}
          >
            <div className="font-serif text-3xl italic mb-4 text-foreground">English Writing</div>
            <div className="text-base text-foreground/70 leading-relaxed mb-6">
              LitRPG · Progression · Romantasy · Sci-Fi · Isekai
            </div>
            <div className="text-sm text-muted-foreground">
              Royal Road · Kindle Unlimited · Scribble Hub
            </div>
          </button>
        </div>

        <div className="text-center text-sm text-muted-foreground">
          可在设置中更改 · Can be changed in Settings
        </div>
      </div>
  );
}
