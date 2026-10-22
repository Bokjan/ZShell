import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type WheelEvent } from "react";
import { useTranslation } from "react-i18next";

import { DEFAULT_GROUP, type CommandGroup, type QuickCommand } from "../ipc/api";
import { announce } from "../lib/announce";
import { usePressDrag } from "../hooks/drag";
import { useStableHandlers } from "../hooks/stableHandlers";
import { useQuickCommands } from "../hooks/stores";
import { useStableProps } from "../hooks/stableProps";
import type { SendResult } from "../lib/compose";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import {
  addGroup,
  deleteCommand,
  deleteGroup,
  groupName,
  moveCommand,
  quickCommandStore,
  renameGroup,
  saveCommand,
} from "../lib/quickCommands";
import { CommandDialog } from "./CommandDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { IconButton } from "./IconButton";
import { NameDialog } from "./NameDialog";
import { ChevronIcon, PlusIcon } from "./icons";
import { useLocale } from "../hooks/locale";

interface Props {
  /** The active tab's group. */
  group: CommandGroup;
  onPickGroup(id: string): void;
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
export function QuickCommandBar({ group, onPickGroup, onRun, targets }: Props) {
  const { t } = useTranslation();
  const { formatList } = useLocale();
  const { commands } = useQuickCommands();
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

  // Shown for a few seconds, and said by screen readers (the focus stays in the terminal).
  const show = (message: string) => {
    setStatus(message);
    announce(message);
  };

  const run = (command: QuickCommand) => {
    const { sent, skipped } = onRun(command);
    if (sent.length === 0) show(t("compose.noTargets"));
    else if (skipped > 0) show(t("quick.sentSkipped", { names: formatList(sent), count: skipped }));
    else if (targets) show(t("quick.sentTo", { names: formatList(sent) }));
  };

  const groupMenu = (e: ReactMouseEvent<HTMLButtonElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const isDefault = group.id === DEFAULT_GROUP;
    const items: MenuItem[] = [
      ...(commands?.groups ?? []).map((g) => ({
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

  // A command's menu: at the pointer, or below its button from the keyboard.
  const commandMenu = (x: number, y: number, command: QuickCommand) => {
    setMenu({
      x,
      y,
      items: [
        { label: t("quick.run"), onSelect: () => run(command) },
        "separator",
        { label: t("quick.edit"), onSelect: () => setEditing({ command }) },
        {
          label: t("quick.duplicate"),
          onSelect: () =>
            quickCommandStore.save((commands) =>
              saveCommand(commands, group.id, { ...command, id: "", name: t("quick.copyName", { name: command.name }) }),
            ),
        },
        "separator",
        { label: t("quick.delete"), danger: true, onSelect: () => setDeleting({ kind: "command", command }) },
      ],
    });
  };

  // Reorders live while dragging; saved on release.
  const pressDrag = usePressDrag();
  const startDrag = (e: ReactMouseEvent, id: string) => {
    if (e.button !== 0) return;
    let ids = group.commands.map((c) => c.id);
    dragged.current = false;
    pressDrag(e, {
      threshold: DRAG_THRESHOLD,
      axis: "x",
      onMove: (ev) => {
        setDragging(id);
        const others = [...listRef.current!.querySelectorAll<HTMLElement>(".quick-command")].filter((el) => el.dataset.id !== id);
        const index = others.filter((el) => {
          const rect = el.getBoundingClientRect();
          return rect.left + rect.width / 2 < ev.clientX;
        }).length;
        const rest = ids.filter((x) => x !== id);
        ids = [...rest.slice(0, index), id, ...rest.slice(index)];
        setOrder(ids);
      },
      onEnd: (moved) => {
        if (moved) {
          dragged.current = true;
          quickCommandStore.save((commands) => moveCommand(commands, id, ids.indexOf(id)));
        }
        setOrder(null);
        setDragging(null);
      },
    });
  };

  // A mouse wheel scrolls the buttons sideways when they don't fit.
  const onWheel = (e: WheelEvent) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) listRef.current!.scrollLeft += e.deltaY;
  };

  // The same from render to render, so that a button renders again only when its command changes.
  const actions = useStableHandlers<CommandActions>({
    onMouseDown: startDrag,
    onClick: (command) => {
      if (dragged.current) dragged.current = false;
      else run(command);
    },
    onMenu: commandMenu,
  });

  const shown = order ? order.map((id) => group.commands.find((c) => c.id === id)).filter((c): c is QuickCommand => !!c) : group.commands;
  const tooltip = (command: QuickCommand) =>
    targets ? `${command.text}\n\n${t("quick.targets", { names: formatList(targets) })}` : command.text;

  return (
    <>
      {/* Pressing a button doesn't take focus from the terminal. */}
      <div className={`quick-bar${targets ? " many" : ""}`} onMouseDown={(e) => e.preventDefault()}>
        <button className="quick-group" title={t("quick.groupHint")} onClick={groupMenu}>
          {groupName(group, t)}
          <ChevronIcon />
        </button>
        {targets && <span className="quick-targets">{t("quick.sendsTo", { count: targets.length })}</span>}
        <div className="quick-commands" ref={listRef} onWheel={onWheel}>
          {shown.length === 0 && <span className="quick-empty">{t("quick.empty")}</span>}
          {shown.map((command) => (
            <CommandButton
              key={command.id}
              command={command}
              dragging={command.id === dragging}
              tooltip={tooltip(command)}
              actions={actions}
            />
          ))}
        </div>
        {status && <span className="quick-status">{status}</span>}
        <IconButton className="quick-add" label={t("quick.newCommand")} onClick={() => setEditing({ command: null })}>
          <PlusIcon />
        </IconButton>
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {editing && (
        <CommandDialog
          command={editing.command}
          groupId={group.id}
          groups={commands?.groups ?? [group]}
          onSave={(groupId, command) => quickCommandStore.save((commands) => saveCommand(commands, groupId, command))}
          onClose={() => setEditing(null)}
        />
      )}
      {naming && (
        <NameDialog
          title={naming.id ? t("quick.renameGroupTitle") : t("quick.newGroupTitle")}
          label={t("quick.groupName")}
          initial={naming.name}
          onSave={(name) =>
            quickCommandStore.save((commands) => (naming.id ? renameGroup(commands, naming.id, name) : addGroup(commands, name)))
          }
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
            quickCommandStore.save((commands) => deleteCommand(commands, deleting.command.id));
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
            quickCommandStore.save((commands) => deleteGroup(commands, group.id));
            onPickGroup(DEFAULT_GROUP);
            setDeleting(null);
          }}
          onCancel={() => setDeleting(null)}
        />
      )}
    </>
  );
}

/** What a command's button does: one object for every button, the same from render to render. */
interface CommandActions {
  onMouseDown(e: ReactMouseEvent, id: string): void;
  onClick(command: QuickCommand): void;
  /** Opens the command's menu at `x`, `y`. */
  onMenu(x: number, y: number, command: QuickCommand): void;
}

/**
 * A command's button, its own component so that React Compiler memoizes it: the bar renders
 * again as the active pane changes (a busy terminal changes its title several times a second),
 * and the buttons keep their elements and handlers.
 */
function CommandButton(props: { command: QuickCommand; dragging: boolean; tooltip: string; actions: CommandActions }) {
  useStableProps("CommandButton", props, ["actions"]);
  const { command, dragging, tooltip, actions } = props;
  return (
    <button
      data-id={command.id}
      className={`quick-command${dragging ? " dragging" : ""}`}
      title={tooltip}
      onMouseDown={(e) => actions.onMouseDown(e, command.id)}
      onClick={() => actions.onClick(command)}
      onContextMenu={(e) => {
        if (followsMenuKey(e)) return;
        e.preventDefault();
        actions.onMenu(e.clientX, e.clientY, command);
      }}
      onKeyDown={(e) => {
        if (!isMenuKey(e)) return;
        e.preventDefault();
        openedByMenuKey();
        const rect = e.currentTarget.getBoundingClientRect();
        actions.onMenu(rect.left, rect.bottom, command);
      }}
    >
      {command.name}
      {!command.enter && <span className="quick-no-enter">…</span>}
    </button>
  );
}
