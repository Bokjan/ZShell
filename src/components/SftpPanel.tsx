import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

import { sftp, type FileEntry, type SessionId } from "../lib/api";
import { basename, formatMode, formatSize, formatTime } from "../lib/format";
import { ConfirmDialog } from "./ConfirmDialog";
import { TransferList, type Transfer } from "./TransferList";

interface Props {
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether this panel is visible and should receive file drops. */
  active: boolean;
}

interface Confirm {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  action(): void;
}

const joinPath = (dir: string, name: string) => (dir.endsWith("/") ? dir + name : `${dir}/${name}`);
const parentPath = (path: string) => path.replace(/\/[^/]+\/?$/, "") || "/";
const labelFor = (names: string[]) => (names.length === 1 ? names[0] : `${names[0]} 等 ${names.length} 项`);

export function SftpPanel({ sessionId, connected, active }: Props) {
  const [cwd, setCwd] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{ path: string; value: string } | null>(null);
  const [newFolder, setNewFolder] = useState<string | null>(null);
  const [chmodTarget, setChmodTarget] = useState<{ entry: FileEntry; value: string } | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const cwdRef = useRef<string | null>(null);
  cwdRef.current = cwd;

  const fail = (e: unknown) => setError(String(e));

  const load = useCallback(
    async (path: string) => {
      if (sessionId == null) return;
      setLoading(true);
      try {
        const listing = await sftp.list(sessionId, path);
        setCwd(listing.path);
        setPathInput(listing.path);
        setEntries(listing.entries);
        setSelected(null);
        setError(null);
      } catch (e) {
        setPathInput(cwdRef.current ?? "");
        fail(e);
      } finally {
        setLoading(false);
      }
    },
    [sessionId],
  );

  // (Re)open on connect, staying in the current directory across reconnects.
  useEffect(() => {
    if (sessionId == null || !connected) return;
    if (cwdRef.current) void load(cwdRef.current);
    else sftp.open(sessionId).then(load, fail);
  }, [sessionId, connected, load]);

  const refresh = () => cwd && load(cwd);

  const updateTransfer = (id: string, patch: Partial<Transfer>) =>
    setTransfers((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));

  const cancelled = useRef(new Set<string>());

  const runTransfer = async (kind: Transfer["kind"], names: string[], run: (id: string) => Promise<string[] | void>) => {
    const id = crypto.randomUUID();
    setTransfers((ts) => [{ id, kind, label: labelFor(names), progress: null, status: "running" }, ...ts]);
    try {
      const results = await run(id);
      updateTransfer(id, { status: "done", results: results ?? undefined });
    } catch (e) {
      updateTransfer(id, cancelled.current.has(id) ? { status: "cancelled" } : { status: "error", error: String(e) });
    }
  };

  const upload = (localPaths: string[], remoteDir: string) => {
    if (sessionId == null || localPaths.length === 0) return;
    const start = () =>
      runTransfer("upload", localPaths.map(basename), async (id) => {
        await sftp.upload(sessionId, id, localPaths, remoteDir, (progress) => updateTransfer(id, { progress }));
        if (cwdRef.current === remoteDir) void load(remoteDir);
      });
    const existing = new Set(remoteDir === cwd ? entries.map((e) => e.name) : []);
    const conflicts = localPaths.map(basename).filter((name) => existing.has(name));
    if (conflicts.length === 0) return void start();
    setConfirm({
      title: "覆盖文件",
      message: `以下项目已存在，继续将覆盖它们：\n${conflicts.join("\n")}`,
      confirmLabel: "覆盖",
      danger: true,
      action: start,
    });
  };

  const download = (entry: FileEntry) => {
    if (sessionId == null) return;
    void runTransfer("download", [entry.name], (id) =>
      sftp.download(sessionId, id, [entry.path], null, (progress) => updateTransfer(id, { progress })),
    );
  };

  const pickAndUpload = async (directory: boolean) => {
    if (!cwd) return;
    const picked = await open({ multiple: true, directory, title: directory ? "选择要上传的文件夹" : "选择要上传的文件" });
    if (picked) upload(Array.isArray(picked) ? picked : [picked], cwd);
  };

  // Accept files dragged in from Finder / Explorer while this panel is visible.
  const uploadRef = useRef(upload);
  uploadRef.current = upload;
  useEffect(() => {
    if (!active) return;
    const inside = (pos: { x: number; y: number }) => {
      const rect = panelRef.current?.getBoundingClientRect();
      const x = pos.x / window.devicePixelRatio;
      const y = pos.y / window.devicePixelRatio;
      return !!rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    };
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "leave") setDragOver(false);
      else if (payload.type === "enter" || payload.type === "over") setDragOver(inside(payload.position));
      else if (payload.type === "drop") {
        setDragOver(false);
        if (inside(payload.position) && cwdRef.current) uploadRef.current(payload.paths, cwdRef.current);
      }
    });
    return () => void unlisten.then((f) => f());
  }, [active]);

  const submitRename = async (e: FormEvent) => {
    e.preventDefault();
    if (!renaming || sessionId == null || !cwd) return;
    const name = renaming.value.trim();
    setRenaming(null);
    if (!name || name === basename(renaming.path)) return;
    try {
      await sftp.rename(sessionId, renaming.path, joinPath(cwd, name));
      await load(cwd);
    } catch (err) {
      fail(err);
    }
  };

  const submitNewFolder = async (e: FormEvent) => {
    e.preventDefault();
    const name = newFolder?.trim();
    setNewFolder(null);
    if (!name || sessionId == null || !cwd) return;
    try {
      await sftp.mkdir(sessionId, joinPath(cwd, name));
      await load(cwd);
    } catch (err) {
      fail(err);
    }
  };

  const submitChmod = async (e: FormEvent) => {
    e.preventDefault();
    if (!chmodTarget || sessionId == null || !cwd) return;
    if (!/^[0-7]{3,4}$/.test(chmodTarget.value)) {
      setError("权限需为 3–4 位八进制数，例如 644 或 0755");
      return;
    }
    const { entry, value } = chmodTarget;
    setChmodTarget(null);
    try {
      await sftp.chmod(sessionId, entry.path, parseInt(value, 8));
      await load(cwd);
    } catch (err) {
      fail(err);
    }
  };

  const askRemove = (entry: FileEntry) =>
    setConfirm({
      title: "删除",
      message: entry.isDir && !entry.isSymlink
        ? `确定删除文件夹「${entry.name}」及其中的所有内容吗？此操作无法撤销。`
        : `确定删除「${entry.name}」吗？此操作无法撤销。`,
      confirmLabel: "删除",
      danger: true,
      action: async () => {
        if (sessionId == null || !cwd) return;
        try {
          await sftp.remove(sessionId, entry.path);
          await load(cwd);
        } catch (err) {
          fail(err);
        }
      },
    });

  const activate = (entry: FileEntry) => (entry.isDir ? void load(entry.path) : download(entry));

  if (!connected && cwd == null) {
    return (
      <div className="sftp-panel" ref={panelRef}>
        <div className="sftp-empty">连接建立后可浏览远程文件</div>
      </div>
    );
  }

  return (
    <div className={`sftp-panel${dragOver ? " drag-over" : ""}`} ref={panelRef}>
      <div className="sftp-toolbar">
        <button className="icon-button" title="上级目录" disabled={!cwd || cwd === "/"} onClick={() => cwd && load(parentPath(cwd))}>
          ↑
        </button>
        <button className="icon-button" title="刷新" disabled={!cwd} onClick={refresh}>
          ⟳
        </button>
        <form
          className="sftp-path"
          onSubmit={(e) => {
            e.preventDefault();
            void load(pathInput.trim() || ".");
          }}
        >
          <input value={pathInput} onChange={(e) => setPathInput(e.target.value)} spellCheck={false} title="输入路径后回车跳转" />
        </form>
      </div>
      <div className="sftp-actions">
        <button disabled={!connected || !cwd} onClick={() => pickAndUpload(false)}>
          上传文件
        </button>
        <button disabled={!connected || !cwd} onClick={() => pickAndUpload(true)}>
          上传文件夹
        </button>
        <button disabled={!connected || !cwd} onClick={() => setNewFolder("")}>
          新建文件夹
        </button>
      </div>

      {error && (
        <div className="sftp-error" onClick={() => setError(null)} title="点击关闭">
          {error}
        </div>
      )}
      {!connected && <div className="sftp-error">连接已断开</div>}

      <div className={`sftp-list${loading ? " loading" : ""}`}>
        <table>
          <colgroup>
            <col />
            <col className="col-size" />
            <col className="col-time" />
          </colgroup>
          <tbody>
            {newFolder != null && (
              <tr>
                <td colSpan={3}>
                  <form onSubmit={submitNewFolder}>
                    <input
                      autoFocus
                      placeholder="新文件夹名称"
                      value={newFolder}
                      onChange={(e) => setNewFolder(e.target.value)}
                      onBlur={() => setNewFolder(null)}
                      onKeyDown={(e) => e.key === "Escape" && setNewFolder(null)}
                    />
                  </form>
                </td>
              </tr>
            )}
            {entries.map((entry) => (
              <tr
                key={entry.path}
                className={entry.path === selected ? "selected" : undefined}
                onClick={() => setSelected(entry.path)}
                onDoubleClick={() => activate(entry)}
                title={`${entry.name}\n${formatMode(entry.permissions, entry.isDir, entry.isSymlink)}`}
              >
                <td>
                  <div className="file-name">
                    <span className="file-icon">{entry.isDir ? "📁" : "📄"}</span>
                    {renaming?.path === entry.path ? (
                      <form onSubmit={submitRename}>
                        <input
                          autoFocus
                          value={renaming.value}
                          onChange={(e) => setRenaming({ path: entry.path, value: e.target.value })}
                          onBlur={() => setRenaming(null)}
                          onKeyDown={(e) => e.key === "Escape" && setRenaming(null)}
                          onFocus={(e) => e.target.setSelectionRange(0, entry.name.lastIndexOf(".") > 0 ? entry.name.lastIndexOf(".") : entry.name.length)}
                        />
                      </form>
                    ) : (
                      <span className={entry.isSymlink ? "symlink" : undefined}>{entry.name}</span>
                    )}
                  </div>
                </td>
                <td className="file-size">{entry.isDir ? "" : formatSize(entry.size)}</td>
                <td className="file-time">
                  <span className="file-time-text">{formatTime(entry.modified)}</span>
                  <span className="row-actions">
                    <button title="下载到“下载”文件夹" onClick={() => download(entry)}>
                      ⬇
                    </button>
                    <button title="重命名" onClick={() => setRenaming({ path: entry.path, value: entry.name })}>
                      ✎
                    </button>
                    <button
                      title="修改权限"
                      onClick={() =>
                        setChmodTarget({ entry, value: ((entry.permissions ?? 0o644) & 0o7777).toString(8).padStart(3, "0") })
                      }
                    >
                      ⚿
                    </button>
                    <button title="删除" onClick={() => askRemove(entry)}>
                      🗑
                    </button>
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {entries.length === 0 && !loading && cwd && <div className="sftp-empty">空文件夹</div>}
      </div>

      <TransferList
        transfers={transfers}
        onCancel={(id) => {
          cancelled.current.add(id);
          void sftp.cancel(id);
        }}
        onDismiss={(id) => setTransfers((ts) => ts.filter((t) => t.id !== id))}
        onClearFinished={() => setTransfers((ts) => ts.filter((t) => t.status === "running"))}
      />

      {dragOver && <div className="drop-hint">松开以上传到 {cwd}</div>}

      {chmodTarget && (
        <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && setChmodTarget(null)}>
          <form className="dialog" onSubmit={submitChmod}>
            <h2>修改权限</h2>
            <label>
              「{chmodTarget.entry.name}」的权限（八进制）
              <input
                autoFocus
                value={chmodTarget.value}
                onChange={(e) => setChmodTarget({ ...chmodTarget, value: e.target.value })}
                onKeyDown={(e) => e.key === "Escape" && setChmodTarget(null)}
              />
            </label>
            <p className="hint">{/^[0-7]{3,4}$/.test(chmodTarget.value) && formatMode(parseInt(chmodTarget.value, 8), chmodTarget.entry.isDir, false)}</p>
            <footer>
              <span className="grow" />
              <button type="button" onClick={() => setChmodTarget(null)}>
                取消
              </button>
              <button type="submit" className="primary">
                确定
              </button>
            </footer>
          </form>
        </div>
      )}
      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          message={confirm.message}
          confirmLabel={confirm.confirmLabel}
          danger={confirm.danger}
          onConfirm={() => {
            setConfirm(null);
            confirm.action();
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}
