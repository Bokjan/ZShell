import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type WheelEvent } from "react";
import { useTranslation } from "react-i18next";

import { DEFAULT_GROUP, type CommandGroup, type QuickCommand, type QuickCommands } from "../lib/api";
import type { SendResult } from "../lib/compose";
import {
  addGroup,
  deleteCommand,
  deleteGroup,
  groupName,
  moveCommand,
  renameGroup,
  saveCommand,
} from "../lib/quickCommands";
import { CommandDialog } from "./CommandDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { NameDialog } from "./NameDialog";
import { PlusIcon } from "./icons";

interface Props {
  commands: QuickCommands;
  /** The active tab's group. */
  group: CommandGroup;
  onPickGroup(id: string): void;
  onChange(commands: QuickCommands): void;
  onRun(command: QuickCommand): SendResult;
  /**
   * Titles of the tabs a command goes to when that is more than the active tab (the compose
   * bar is open with another scope); the bar then turns to the warning color.
   */
  targets: string[] | null;
}

/** How far the pointer moves before a press on a command becomes a drag. */
const DRAG_THRESHOLD = 5;
const STATUS_MS = 3000;

/**
 * Buttons for the quick commands of a group, below the terminal: the group switcher on the
 * left (with group management), a button per command (right-click to edit, drag to
 * reorder), and + to add one.
 */
export function QuickCommandBar({ commands, group, onPickGroup, onChange, onRun, targets }: Props) {
  const { t } = useTranslation();
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [editing, setEditing] = useState<{ command: QuickCommand | null } | null>(null);
  const [naming, setNaming] = useState<{ id: string | null; name: string } | null>(null);
  const [deleting, setDeleting] = useState<{ kind: "command"; command: QuickCommand } | { kind: "group" } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  // Command ids in their order while one is dragged, shown before the order is saved.
  const [order, setOrder] = useState<string[] | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  // A drag ends with a click on the dragged button, which must not run it.
  const dragged = useRef(false);

  useEffect(() => {
    if (status === null) return;
    const timer = setTimeout(() => setStatus(null), STATUS_MS);
    return () => clearTimeout(timer);
  }, [status]);

  const run = (command: QuickCommand) => {
    const { sent, skipped } = onRun(command);
    if (sent.length === 0) setStatus(t("compose.noTargets"));
    else if (skipped > 0) setStatus(t("quick.sentSkipped", { names: sent.join(", "), count: skipped }));
    else if (targets) setStatus(t("quick.sentTo", { names: sent.join(", ") }));
  };

  const groupMenu = (e: ReactMouseEvent<HTMLButtonElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const isDefault = group.id === DEFAULT_GROUP;
    const items: MenuItem[] = [
      ...commands.groups.map((g) => ({
        label: groupName(g, t),
        checked: g.id === group.id,
        onSelect: () => onPickGroup(g.id),
      })),
      "separator",
      { label: t("quick.newGroup"), onSelect: () => setNaming({ id: null, name: t("quick.newGroupName") }) },
      { label: t("quick.renameGroup"), disabled: isDefault, onSelect: () => setNaming({ id: group.id, name: group.name }) },
      { label: t("quick.deleteGroup"), disabled: isDefault, danger: true, onSelect: () => setDeleting({ kind: "group" }) },
    ];
    // At the button; near the bottom of the window, the menu moves up to stay inside it.
    setMenu({ x: rect.left, y: rect.top, items });
  };

  const commandMenu = (e: ReactMouseEvent, command: QuickCommand) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: t("quick.run"), onSelect: () => run(command) },
        "separator",
        { label: t("quick.edit"), onSelect: () => setEditing({ command }) },
        {
          label: t("quick.duplicate"),
          onSelect: () => onChange(saveCommand(commands, group.id, { ...command, id: "", name: t("quick.copyName", { name: command.name }) })),
        },
        "separator",
        { label: t("quick.delete"), danger: true, onSelect: () => setDeleting({ kind: "command", command }) },
      ],
    });
  };

  // Reorders live while dragging; saved on release.
  const startDrag = (e: ReactMouseEvent, id: string) => {
    if (e.button !== 0) return;
    const startX = e.clientX;
    let ids = group.commands.map((c) => c.id);
    let moved = false;
    dragged.current = false;
    const move = (ev: MouseEvent) => {
      if (!moved) {
        if (Math.abs(ev.clientX - startX) < DRAG_THRESHOLD) return;
        moved = true;
        setDragging(id);
      }
      const others = [...listRef.current!.querySelectorAll<HTMLElement>(".quick-command")].filter((el) => el.dataset.id !== id);
      const index = others.filter((el) => {
        const rect = el.getBoundingClientRect();
        return rect.left + rect.width / 2 < ev.clientX;
      }).length;
      const rest = ids.filter((x) => x !== id);
      ids = [...rest.slice(0, index), id, ...rest.slice(index)];
      setOrder(ids);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (moved) {
        dragged.current = true;
        onChange(moveCommand(commands, id, ids.indexOf(id)));
      }
      setOrder(null);
      setDragging(null);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  // A mouse wheel scrolls the buttons sideways when they don't fit.
  const onWheel = (e: WheelEvent) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) listRef.current!.scrollLeft += e.deltaY;
  };

  const shown = order
    ? order.map((id) => group.commands.find((c) => c.id === id)).filter((c): c is QuickCommand => !!c)
    : group.commands;
  const tooltip = (command: QuickCommand) =>
    targets ? `${command.text}\n\n${t("quick.targets", { names: targets.join(", ") })}` : command.text;

  return (
    <>
      {/* Pressing a button doesn't take focus from the terminal. */}
      <div className={`quick-bar${targets ? " many" : ""}`} onMouseDown={(e) => e.preventDefault()}>
        <button className="quick-group" title={t("quick.groupHint")} onClick={groupMenu}>
          {groupName(group, t)} ▾
        </button>
        {targets && <span className="quick-targets">{t("quick.sendsTo", { count: targets.length })}</span>}
        <div className="quick-commands" ref={listRef} onWheel={onWheel}>
          {shown.length === 0 && <span className="quick-empty">{t("quick.empty")}</span>}
          {shown.map((command) => (
            <button
              key={command.id}
              data-id={command.id}
              className={`quick-command${command.id === dragging ? " dragging" : ""}`}
              title={tooltip(command)}
              onMouseDown={(e) => startDrag(e, command.id)}
              onClick={() => {
                if (dragged.current) dragged.current = false;
                else run(command);
              }}
              onContextMenu={(e) => commandMenu(e, command)}
            >
              {command.name}
              {!command.enter && <span className="quick-no-enter">…</span>}
            </button>
          ))}
        </div>
        {status && <span className="quick-status">{status}</span>}
        <button className="quick-add" title={t("quick.newCommand")} onClick={() => setEditing({ command: null })}>
          <PlusIcon />
        </button>
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {editing && (
        <CommandDialog
          command={editing.command}
          groupId={group.id}
          groups={commands.groups}
          onSave={(groupId, command) => onChange(saveCommand(commands, groupId, command))}
          onClose={() => setEditing(null)}
        />
      )}
      {naming && (
        <NameDialog
          title={naming.id ? t("quick.renameGroupTitle") : t("quick.newGroupTitle")}
          label={t("quick.groupName")}
          initial={naming.name}
          onSave={(name) => onChange(naming.id ? renameGroup(commands, naming.id, name) : addGroup(commands, name))}
          onClose={() => setNaming(null)}
        />
      )}
      {deleting?.kind === "command" && (
        <ConfirmDialog
          title={t("quick.deleteTitle")}
          message={t("quick.deleteMessage", { name: deleting.command.name })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => {
            onChange(deleteCommand(commands, deleting.command.id));
            setDeleting(null);
          }}
          onCancel={() => setDeleting(null)}
        />
      )}
      {deleting?.kind === "group" && (
        <ConfirmDialog
          title={t("quick.deleteGroupTitle")}
          message={t("quick.deleteGroupMessage", { name: group.name, count: group.commands.length })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => {
            onChange(deleteGroup(commands, group.id));
            onPickGroup(DEFAULT_GROUP);
            setDeleting(null);
          }}
          onCancel={() => setDeleting(null)}
        />
      )}
    </>
  );
}
