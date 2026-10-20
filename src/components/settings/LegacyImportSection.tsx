import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { legacyImportApi, type LegacyImportReport } from "@/lib/api/settings";
import { extractErrorMessage } from "@/utils/errorUtils";
import { SettingsBlock, SettingsCard, SettingsRow } from "./SettingsLayout";

const PREVIEW_KEY = ["legacy-import-preview"];

function counts(report: LegacyImportReport) {
  return {
    providers: report.providers,
    logs: report.requestLogs,
    rollups: report.rollups,
    other: report.otherRows,
    skills: report.skillFolders,
    backups: report.backups,
  };
}

function pendingTotal(report: LegacyImportReport) {
  return Object.values(counts(report)).reduce((sum, n) => sum + n, 0);
}

/**
 * 从官方 CC Switch 的 ~/.cc-switch 补进本应用缺的数据：只加不改，导入前先备份数据库。
 * 没有 ~/.cc-switch 的机器上整块不显示。
 */
export function LegacyImportSection() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: preview } = useQuery({
    queryKey: PREVIEW_KEY,
    queryFn: () => legacyImportApi.preview(),
  });

  const importMutation = useMutation({
    mutationFn: () => legacyImportApi.apply(),
    onSuccess: async (report) => {
      toast.success(t("settings.data.legacy.done", counts(report)));
      // 供应商、用量、提示词、MCP、技能都可能变了
      await queryClient.invalidateQueries();
    },
    onError: (error) => {
      toast.error(
        t("settings.data.legacy.failed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    },
  });

  if (!preview?.found) return null;
  const pending = pendingTotal(preview);

  return (
    <SettingsBlock
      title={t("settings.data.legacy.title")}
      help={{
        title: t("settings.data.legacy.title"),
        body: t("settings.data.legacy.description", { path: preview.path }),
      }}
    >
      <SettingsCard>
        <SettingsRow
          label={preview.path}
          description={
            pending > 0
              ? t("settings.data.legacy.pending", counts(preview))
              : t("settings.data.legacy.upToDate")
          }
          control={
            <Button
              variant="solid"
              size="compact"
              disabled={pending === 0 || importMutation.isPending}
              onClick={() => importMutation.mutate()}
            >
              {importMutation.isPending && (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              )}
              {importMutation.isPending
                ? t("settings.data.legacy.importing")
                : t("settings.data.legacy.import")}
            </Button>
          }
        />
      </SettingsCard>
    </SettingsBlock>
  );
}
