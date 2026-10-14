import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Layers } from "lucide-react";
import type { Provider } from "@/types";
import type { ProxyStackMember } from "@/types/proxy";
import { cn } from "@/lib/utils";

interface AggregatedModelsPanelProps {
  members: ProxyStackMember[];
  providers: Record<string, Provider>;
}

/**
 * 聚合模式下发布给客户端的完整模型清单（v7 S4）：按名单顺序，每家一行，模型按显示名列出。
 *
 * 这里显示的就是 Claude Desktop `/v1/models` 拿到的那份——两边对不上时先看这里，再去查
 * Desktop 的模型选择器。Codex 的模型由目录条目描述，`models` 为空，只显示数量。
 */
export function AggregatedModelsPanel({
  members,
  providers,
}: AggregatedModelsPanelProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const rows = useMemo(
    () =>
      members.map((member) => ({
        id: member.providerId,
        name: providers[member.providerId]?.name ?? member.providerId,
        models: member.models ?? [],
        count: member.modelIds.length,
        route: member.route,
      })),
    [members, providers],
  );

  const total = rows.reduce((sum, row) => sum + row.count, 0);
  if (total === 0) {
    return null;
  }

  return (
    <section className="mb-3 rounded-panel bg-stack-soft p-3">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 text-start text-body text-fg-1"
      >
        <Layers className="size-4 shrink-0 text-stack-text" />
        <span className="flex-1">
          {t("mode.stack.aggregatedModels", { count: total })}
        </span>
        <span className="text-2xs text-fg-2">
          {t("mode.stack.aggregatedMembers", { count: rows.length })}
        </span>
        <ChevronDown
          className={cn(
            "size-4 shrink-0 text-fg-2 transition-transform",
            open && "rotate-180",
          )}
        />
      </button>

      {open && (
        <ul className="mt-2 space-y-2">
          {rows.map((row) => (
            <li key={row.id} className="text-body">
              <div className="flex items-baseline gap-2">
                <span className="font-medium text-fg-1">{row.name}</span>
                {row.route && (
                  <span className="text-2xs text-fg-2">
                    {t("mode.stack.defaultProvider")}
                  </span>
                )}
                <span className="text-2xs text-fg-2">
                  {t("mode.stack.modelCount", { count: row.count })}
                </span>
              </div>
              {row.models.length > 0 ? (
                <div className="mt-1 flex flex-wrap gap-1">
                  {row.models.map((model) => (
                    <span
                      key={model.id}
                      title={model.id}
                      className="rounded-chip bg-panel px-1.5 py-0.5 text-2xs text-fg-2"
                    >
                      {model.displayName}
                      {model.supports1m ? " · 1M" : ""}
                    </span>
                  ))}
                </div>
              ) : (
                <div className="mt-1 text-2xs text-fg-2">
                  {t("mode.stack.modelsFromCatalog")}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
