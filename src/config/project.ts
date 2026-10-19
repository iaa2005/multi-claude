/**
 * Where MultiClaude lives. Release tags carry a `-iaa2005` suffix
 * (`v4.0.13-iaa2005`) so they never collide with upstream CC Switch tags;
 * the app version itself stays plain semver for the updater and installers.
 */
export const REPO_URL = "https://github.com/iaa2005/multi-claude";
export const RELEASES_URL = `${REPO_URL}/releases`;
export const RELEASE_TAG_SUFFIX = "-iaa2005";

/** `4.0.13` or `v4.0.13` → the release page of `v4.0.13-iaa2005`. */
export function releaseUrl(version?: string | null): string {
  const plain = (version ?? "").trim().replace(/^v/, "");
  if (!plain) return RELEASES_URL;
  const tag = plain.endsWith(RELEASE_TAG_SUFFIX)
    ? `v${plain}`
    : `v${plain}${RELEASE_TAG_SUFFIX}`;
  return `${RELEASES_URL}/tag/${tag}`;
}
