import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { claudeScienceApi } from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * The daemon a provider switch restarts, as a status line.
 *
 * Science keeps its state in a process, not in a file: "switched" is only
 * true once `claude-science serve` came back up. A user who quits Science
 * from its tray needs to see that here rather than discover it as a failed
 * chat.
 */
export function ClaudeScienceStatusBar() {
  const { t } = useTranslation();
  const { data, isFetching, refetch } = useQuery({
    queryKey: ["claudeScienceStatus"],
    queryFn: claudeScienceApi.status,
    refetchInterval: 5000,
  });

  const label = !data
    ? t("claudeScience.statusChecking", {
        defaultValue: "Checking the Claude Science daemon…",
      })
    : !data.installed
      ? t("claudeScience.statusMissing", {
          defaultValue: "Claude Science is not installed",
        })
      : data.error
        ? data.error
        : data.running
          ? t("claudeScience.statusRunning", {
              defaultValue: "Daemon running on :{{port}} · v{{version}}",
              port: data.port ?? "?",
              version: data.version ?? "?",
            })
          : t("claudeScience.statusStopped", {
              defaultValue: "Daemon stopped — switching a provider starts it",
            });

  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-card px-3 py-2 text-xs">
      <div className="flex min-w-0 items-center gap-2">
        <span
          className={cn(
            "size-2 shrink-0 rounded-full",
            data?.running ? "bg-emerald-500" : "bg-muted-foreground/40",
          )}
        />
        <span className="truncate text-fg-2">{label}</span>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 shrink-0 gap-1.5 px-2 text-xs"
        onClick={() => void refetch()}
        disabled={isFetching}
      >
        <RefreshCw className={cn("size-3.5", isFetching && "animate-spin")} />
        {t("common.refresh", { defaultValue: "Refresh" })}
      </Button>
    </div>
  );
}
