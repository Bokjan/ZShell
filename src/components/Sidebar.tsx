import { useTranslation } from "react-i18next";

import type { Profile } from "../lib/api";
import { settingsShortcutLabel } from "../lib/platform";

interface Props {
  profiles: Profile[];
  onOpen(profile: Profile): void;
  onEdit(profile: Profile): void;
  onNew(): void;
  onImport(): void;
  onSettings(): void;
}

export function Sidebar({ profiles, onOpen, onEdit, onNew, onImport, onSettings }: Props) {
  const { t } = useTranslation();
  const meta = (p: Profile) => {
    const address = `${p.username}@${p.host}${p.port !== 22 ? `:${p.port}` : ""}`;
    if (p.jumpHosts.length === 0) return address;
    const names = p.jumpHosts.map((id) => profiles.find((j) => j.id === id)?.name ?? "?").join(", ");
    return t("sidebar.metaVia", { address, names });
  };
  return (
    <aside className="sidebar">
      <header>
        <span>{t("sidebar.title")}</span>
        <span className="sidebar-actions">
          <button className="icon-button" title={t("sidebar.import")} onClick={onImport}>
            ⇣
          </button>
          <button className="icon-button" title={t("sidebar.newSession")} onClick={onNew}>
            +
          </button>
        </span>
      </header>
      {profiles.length === 0 ? (
        <p className="sidebar-empty">{t("sidebar.empty")}</p>
      ) : (
        <ul>
          {profiles.map((p) => (
            <li key={p.id} onClick={() => onOpen(p)} title={t("sidebar.openHint")}>
              <div className="profile-name">{p.name}</div>
              <div className="profile-meta">{meta(p)}</div>
              <button
                className="icon-button profile-edit"
                title={t("sidebar.edit")}
                onClick={(e) => {
                  e.stopPropagation();
                  onEdit(p);
                }}
              >
                ⋯
              </button>
            </li>
          ))}
        </ul>
      )}
      <footer className="sidebar-footer">
        <button className="sidebar-settings" onClick={onSettings}>
          <SettingsIcon />
          <span>{t("sidebar.settings")}</span>
          <kbd>{settingsShortcutLabel}</kbd>
        </button>
      </footer>
    </aside>
  );
}

/** Sliders, drawn as an SVG rather than a text glyph so it centers the same in every font. */
function SettingsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true">
      <path d="M2 4h1.4M6.6 4H14M2 8h7.4M12.6 8H14M2 12h3.4M8.6 12H14" />
      <circle cx="5" cy="4" r="1.6" />
      <circle cx="11" cy="8" r="1.6" />
      <circle cx="7" cy="12" r="1.6" />
    </svg>
  );
}
