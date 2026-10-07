import { useTranslation } from "react-i18next";

import type { Profile } from "../lib/api";

interface Props {
  profiles: Profile[];
  onOpen(profile: Profile): void;
  onEdit(profile: Profile): void;
  onNew(): void;
}

export function Sidebar({ profiles, onOpen, onEdit, onNew }: Props) {
  const { t } = useTranslation();
  return (
    <aside className="sidebar">
      <header>
        <span>{t("sidebar.title")}</span>
        <button className="icon-button" title={t("sidebar.newSession")} onClick={onNew}>
          +
        </button>
      </header>
      {profiles.length === 0 ? (
        <p className="sidebar-empty">{t("sidebar.empty")}</p>
      ) : (
        <ul>
          {profiles.map((p) => (
            <li key={p.id} onClick={() => onOpen(p)} title={t("sidebar.openHint")}>
              <div className="profile-name">{p.name}</div>
              <div className="profile-meta">
                {p.username}@{p.host}
                {p.port !== 22 && `:${p.port}`}
              </div>
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
    </aside>
  );
}
