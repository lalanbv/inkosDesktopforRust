import { Settings, Feather, Wand2, Film, Settings2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { NAV_SECTIONS, sectionTitle } from "@/lib/nav-sections";
import type { NavSectionId } from "@/lib/nav-sections";
import type { Nav } from "@/lib/nav";
import type { TFunction } from "@/hooks/use-i18n";

const SECTION_ICONS: Record<string, LucideIcon> = {
  feather: Feather,
  wand: Wand2,
  "settings-2": Settings2,
  film: Film,
};

/**
 * 活动栏（P3-1）：48px 窄列，四区图标 + 底部设置。
 * 点击切换 SidePanel 的区；选中态来自 App（用户手动选择优先，否则跟随路由）。
 */
export function ActivityBar({ nav, activeSection, onSelectSection, t, lang }: {
  nav: Nav;
  activeSection: NavSectionId;
  onSelectSection: (section: NavSectionId) => void;
  t: TFunction;
  lang: "zh" | "en";
}) {
  return (
    <nav
      data-slot="activity-bar"
      aria-label={t("nav.activityBar")}
      className="w-12 shrink-0 flex flex-col items-center gap-1 border-r border-border bg-background/80 backdrop-blur-md py-3 select-none"
    >
      {NAV_SECTIONS.map((section) => {
        const Icon = SECTION_ICONS[section.icon] ?? Feather;
        const isActive = section.id === activeSection;
        return (
          <button
            key={section.id}
            type="button"
            data-testid={`activity-${section.id}`}
            aria-label={sectionTitle(section, lang)}
            aria-current={isActive ? "page" : undefined}
            title={sectionTitle(section, lang)}
            onClick={() => onSelectSection(section.id)}
            className={`flex size-9 items-center justify-center rounded-lg transition-colors ${
              isActive
                ? "bg-secondary text-primary shadow-sm border border-border"
                : "text-muted-foreground/70 hover:text-foreground hover:bg-secondary/40"
            }`}
          >
            <Icon size={19} className="shrink-0" />
          </button>
        );
      })}

      <div className="mt-auto">
        <button
          type="button"
          data-testid="activity-settings"
          aria-label={t("nav.projectSettings")}
          title={t("nav.projectSettings")}
          onClick={nav.toProjectSettings}
          className="flex size-9 items-center justify-center rounded-lg text-muted-foreground/70 hover:text-foreground hover:bg-secondary/40 transition-colors"
        >
          <Settings size={19} className="shrink-0" />
        </button>
      </div>
    </nav>
  );
}
