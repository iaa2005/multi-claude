import type { ComponentType } from "react";
import { Monitor, Terminal } from "lucide-react";
import type { AppId } from "@/lib/api";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";

/** 应用名：品牌名不翻译，四种语言都写原文。 */
export const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  "claude-science": "Claude Science",
  codex: "Codex",
  gemini: "Gemini CLI",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax Code",
};

const APP_ICON_NAME: Record<AppId, string> = {
  claude: "claude",
  "claude-desktop": "claude",
  "claude-science": "claude",
  codex: "openai",
  gemini: "gemini",
  grokbuild: "grok",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
  pi: "pi",
  mcode: "minimax",
};

type BadgeIcon = ComponentType<{
  className?: string;
  strokeWidth?: number | string;
}>;

/** DNA（dna-line-duotone），Claude Science 的角标，接口和 lucide 图标一致。 */
function DnaIcon({
  className,
  strokeWidth = 1.5,
}: {
  className?: string;
  strokeWidth?: number | string;
}) {
  return (
    <svg viewBox="0 0 24 24" className={className}>
      <g
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeWidth={strokeWidth}
      >
        <path d="M11.8476 12.1524L12.0462 12.2208C15.3052 13.3428 19.2301 12.2699 22 9.5M11.8476 12.1524L11.2902 11.9606C8.24848 10.9134 4.58525 11.9147 2 14.5M11.8476 12.1524L11.7792 11.9538C10.6572 8.69481 11.7301 4.76991 14.5 2M11.8476 12.1524L12.0394 12.7098C13.0866 15.7515 12.0853 19.4147 9.5 22" />
        <path d="M6.76465 11.8433L11.8431 16.9217" opacity=".5" />
        <path d="M11.8438 6.76465L16.9222 11.8431" opacity=".5" />
        <path d="M3.99976 13L7.49976 16.5" opacity=".5" />
        <path d="M19.7783 10.8906L16.2783 7.39062" opacity=".5" />
        <path d="M9.30469 18.1914L10.8917 19.7784" opacity=".5" />
        <path d="M14.5869 5.58691L12.9999 3.99989" opacity=".5" />
      </g>
    </svg>
  );
}

// Claude Code、Desktop、Science 用同一个图标，靠右下角的小角标区分终端、桌面与科研
const APP_BADGE_ICON: Partial<Record<AppId, BadgeIcon>> = {
  claude: Terminal,
  "claude-desktop": Monitor,
  "claude-science": DnaIcon,
};

interface AppGlyphProps {
  app: AppId;
  /** 图标边长：侧栏 16，页头 20 */
  size?: number;
  /** 角标底色要和所在的行一致，否则看起来像缺了一块 */
  badgeClassName?: string;
  className?: string;
}

/** 应用图标 + 角标（装饰性，名称由旁边的文字或按钮的 aria-label 提供）。 */
export function AppGlyph({
  app,
  size = 16,
  badgeClassName,
  className,
}: AppGlyphProps) {
  const BadgeIcon = APP_BADGE_ICON[app];
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center",
        className,
      )}
      style={{ width: size + 2, height: size + 2 }}
    >
      <ProviderIcon
        icon={APP_ICON_NAME[app]}
        name=""
        size={size}
        showFallback={false}
      />
      {BadgeIcon && (
        <span
          className={cn(
            "absolute -bottom-1 -right-1 flex h-[11px] w-[11px] items-center justify-center rounded-[3px] bg-sidebar text-fg-2",
            badgeClassName,
          )}
        >
          <BadgeIcon className="h-[9px] w-[9px]" strokeWidth={3} />
        </span>
      )}
    </span>
  );
}
