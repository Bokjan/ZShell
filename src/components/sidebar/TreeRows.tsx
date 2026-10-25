import { useRef, type MouseEvent as ReactMouseEvent } from "react";
import type { TFunction } from "i18next";

import type { Folder, Profile, TreeItem } from "../../ipc/api";
import { isComposing } from "../../lib/platform";
import { address, notesPreview } from "../../lib/sessions";
import { useStableProps } from "../../hooks/stableProps";
import { DisclosureIcon, ProtocolIcon } from "../icons";

/**
 * Indentation per folder level, in pixels: a row's icon (a folder's disclosure triangle, a
 * session's protocol) and the gap after it, so that the rows in a folder start below its name.
 */
export const INDENT = 20;

/** What a row of the list asks of the sidebar; one object that stays the same. */
export interface RowActions {
  /** A press on the row, which may start dragging `item`. */
  press(e: ReactMouseEvent, key: string, item: TreeItem | null): void;
  select(key: string): void;
  /** A double-click on the row. */
  open(key: string, profile: Profile): void;
  profileMenu(e: ReactMouseEvent, profile: Profile): void;
  folderMenu(e: ReactMouseEvent, folder: Folder, count: number): void;
  toggle(folder: Folder, expanded: boolean): void;
  finishRename(folder: Folder, name: string | null): void;
}

/**
 * A session's row, its own component so that React Compiler memoizes it: with thousands of
 * sessions, the others don't render again as one is selected, dragged over or searched for.
 */
export function SessionRow(props: {
  id: string;
  rowKey: string;
  profile: Profile;
  depth: number;
  /** Shown after the name instead of the address. */
  meta: string | undefined;
  searching: boolean;
  /** Selected in the tree, or highlighted among the search results. */
  current: boolean;
  /** Its session is selected (also as a recent one). */
  selected: boolean;
  /** More classes. */
  extra: string;
  actions: RowActions;
  /** The list's, rather than each row listening for the language. */
  t: TFunction;
}) {
  useStableProps("SessionRow", props, ["actions"]);
  const { id, rowKey: key, profile, depth, meta, searching, current, selected, extra, actions, t } = props;
  return (
    <div
      id={id}
      role={searching ? "option" : "treeitem"}
      aria-selected={current}
      aria-level={searching ? undefined : depth + 1}
      // Rather than the tooltip, which is also read, as a hint.
      aria-label={t(key.startsWith("recent:") ? "sidebar.recentRowLabel" : "sidebar.rowLabel", {
        name: profile.name,
        address: meta ?? address(profile.connection),
      })}
      data-key={key.startsWith("p:") ? key : undefined}
      data-recent={key.startsWith("recent:") || undefined}
      className={`tree-row session${selected ? " selected" : ""}${extra}`}
      style={{ paddingLeft: 8 + depth * INDENT }}
      onMouseDown={(e) => actions.press(e, key, key.startsWith("p:") ? { kind: "profile", id: profile.id } : null)}
      onClick={() => actions.select(`p:${profile.id}`)}
      onDoubleClick={() => actions.open(key, profile)}
      onContextMenu={(e) => actions.profileMenu(e, profile)}
      title={[profile.name, address(profile.connection), profile.notes && `\n${notesPreview(profile.notes)}\n`, t("sidebar.openHint")]
        .filter(Boolean)
        .join("\n")}
    >
      <span className="tree-icon">
        <ProtocolIcon protocol={profile.connection.protocol} />
      </span>
      <span className="tree-name">{profile.name}</span>
      <span className="tree-meta">{meta ?? address(profile.connection)}</span>
    </div>
  );
}

/** A folder's row, its own component as `SessionRow`. */
export function FolderRow(props: {
  id: string;
  folder: Folder;
  depth: number;
  expanded: boolean;
  /** How many sessions it holds. */
  count: number;
  selected: boolean;
  renaming: boolean;
  /** More classes. */
  extra: string;
  actions: RowActions;
  /** As for `SessionRow`. */
  t: TFunction;
}) {
  useStableProps("FolderRow", props, ["actions"]);
  const { id, folder, depth, expanded, count, selected, renaming, extra, actions, t } = props;
  const key = `f:${folder.id}`;
  return (
    <div
      id={id}
      role="treeitem"
      aria-selected={selected}
      aria-expanded={expanded}
      aria-level={depth + 1}
      aria-label={t("sidebar.folderLabel", { name: folder.name, count })}
      data-key={key}
      className={`tree-row folder${selected ? " selected" : ""}${extra}`}
      style={{ paddingLeft: 8 + depth * INDENT }}
      onMouseDown={(e) => actions.press(e, key, { kind: "folder", id: folder.id })}
      onClick={() => actions.toggle(folder, expanded)}
      onContextMenu={(e) => actions.folderMenu(e, folder, count)}
    >
      <span className="tree-icon">
        <DisclosureIcon open={expanded} />
      </span>
      {renaming ? (
        <NameEditor initial={folder.name} onDone={(name) => actions.finishRename(folder, name)} />
      ) : (
        <span className="tree-name">{folder.name}</span>
      )}
      <span className="tree-count">{count}</span>
    </div>
  );
}

/** Inline editor for a folder name: Enter or leaving keeps it, Escape cancels (`null`). */
function NameEditor({ initial, onDone }: { initial: string; onDone(name: string | null): void }) {
  const done = useRef(false);
  const finish = (name: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(name);
  };
  return (
    <input
      className="tree-name-input"
      defaultValue={initial}
      autoFocus
      onFocus={(e) => e.target.select()}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (isComposing(e)) return;
        if (e.key === "Enter") finish(e.currentTarget.value.trim() || null);
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish(e.target.value.trim() || null)}
      autoCapitalize="off"
      autoCorrect="off"
    />
  );
}
