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
        <span>Sessions</span>
        <button className="icon-button" title="New session" onClick={onNew}>
          +
        </button>
      </header>
      {profiles.length === 0 ? (
        <p className="sidebar-empty">No sessions yet. Click + to add one.</p>
      ) : (
        <ul>
          {profiles.map((p) => (
            <li key={p.id} onClick={() => onOpen(p)} title="Click to open in a new tab">
              <div className="profile-name">{p.name}</div>
              <div className="profile-meta">
                {p.username}@{p.host}
                {p.port !== 22 && `:${p.port}`}
              </div>
              <button
                className="icon-button profile-edit"
                title="Edit"
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
