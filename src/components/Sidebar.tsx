import type { Profile } from "../lib/api";

interface Props {
  profiles: Profile[];
  onOpen(profile: Profile): void;
  onEdit(profile: Profile): void;
  onNew(): void;
}

export function Sidebar({ profiles, onOpen, onEdit, onNew }: Props) {
  return (
    <aside className="sidebar">
      <header>
        <span>会话</span>
        <button className="icon-button" title="新建会话" onClick={onNew}>
          +
        </button>
      </header>
      {profiles.length === 0 ? (
        <p className="sidebar-empty">还没有会话，点击 + 新建</p>
      ) : (
        <ul>
          {profiles.map((p) => (
            <li key={p.id} onClick={() => onOpen(p)} title="点击打开新标签页">
              <div className="profile-name">{p.name}</div>
              <div className="profile-meta">
                {p.username}@{p.host}
                {p.port !== 22 && `:${p.port}`}
              </div>
              <button
                className="icon-button profile-edit"
                title="编辑"
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
