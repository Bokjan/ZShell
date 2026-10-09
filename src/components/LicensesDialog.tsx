import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";

import { useDialog } from "../lib/dialogs";
import { isComposing } from "../lib/platform";
import { Modal } from "./Modal";

/** Written by `pnpm licenses:generate`; local builds may not have it. */
const NOTICES_URL = "/third-party-licenses.json";

interface Package {
  name: string;
  version: string;
  /** The SPDX expression the package declares. */
  license: string;
  url: string | null;
  /** Indices into `Notices.texts`. */
  texts: number[];
}

interface Notices {
  rust: Package[];
  /** Set when the build had no cargo-about. */
  rustMissing: boolean;
  npm: Package[];
  texts: string[];
}

type Ecosystem = "rust" | "npm";
const ECOSYSTEMS: Ecosystem[] = ["rust", "npm"];

interface Entry {
  key: string;
  ecosystem: Ecosystem;
  pkg: Package;
}

/** The third-party license notices that ship with the app: packages on the left, the
 *  selected one's license texts on the right. */
export function LicensesDialog({ onClose }: { onClose(): void }) {
  const { t } = useTranslation();
  // undefined while loading, null when the build has no notices.
  const [notices, setNotices] = useState<Notices | null | undefined>(undefined);
  const [query, setQuery] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current = true;
    fetch(NOTICES_URL)
      // Both the dev server and Tauri answer a missing file with index.html.
      .then((res) => (res.ok && res.headers.get("content-type")?.includes("json") ? res.json() : null))
      .catch(() => null)
      .then((loaded: Notices | null) => current && setNotices(loaded));
    return () => {
      current = false;
    };
  }, []);

  const dialog = useDialog(onClose);

  const entries = useMemo(() => {
    if (!notices) return [];
    const needle = query.trim().toLowerCase();
    return ECOSYSTEMS.flatMap((ecosystem) =>
      notices[ecosystem]
        .filter((pkg) => !needle || pkg.name.toLowerCase().includes(needle) || pkg.license.toLowerCase().includes(needle))
        .map((pkg): Entry => ({ key: `${ecosystem}:${pkg.name}@${pkg.version}`, ecosystem, pkg })),
    );
  }, [notices, query]);

  // The selection stays on its package while it matches the search, else the first match.
  const index = Math.max(
    entries.findIndex((entry) => entry.key === selectedKey),
    0,
  );
  const selected = entries[index];

  useEffect(() => {
    listRef.current?.querySelector(".licenses-item.selected")?.scrollIntoView({ block: "nearest" });
  }, [selected?.key]);

  const onSearchKey = (e: KeyboardEvent) => {
    if (isComposing(e)) return;
    let next: number;
    if (e.key === "ArrowDown") next = Math.min(index + 1, entries.length - 1);
    else if (e.key === "ArrowUp") next = Math.max(index - 1, 0);
    else return;
    e.preventDefault();
    if (entries[next]) setSelectedKey(entries[next].key);
  };

  return (
    <Modal dialog={dialog}>
      <div className="dialog licenses-dialog">
        <h2>{t("licenses.title")}</h2>
        {notices === null ? (
          <p className="dialog-message">{t("licenses.missing")}</p>
        ) : (
          <div className="licenses-body">
            <div className="licenses-nav">
              <input
                value={query}
                placeholder={t("licenses.search")}
                autoFocus
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={onSearchKey}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
              />
              <div className="licenses-list" ref={listRef}>
                {notices &&
                  ECOSYSTEMS.map((ecosystem) => {
                    const group = entries.filter((entry) => entry.ecosystem === ecosystem);
                    if (group.length === 0 && !(ecosystem === "rust" && notices.rustMissing)) return null;
                    return (
                      <div key={ecosystem}>
                        <div className="licenses-group">
                          {t(`licenses.groups.${ecosystem}`, { count: group.length })}
                        </div>
                        {ecosystem === "rust" && notices.rustMissing && (
                          <p className="licenses-empty">{t("licenses.rustMissing")}</p>
                        )}
                        {group.map((entry) => (
                          <div
                            key={entry.key}
                            className={`licenses-item${entry === selected ? " selected" : ""}`}
                            onClick={() => setSelectedKey(entry.key)}
                          >
                            <span className="licenses-name">
                              {entry.pkg.name} <span className="licenses-version">{entry.pkg.version}</span>
                            </span>
                            <span className="licenses-license">{entry.pkg.license}</span>
                          </div>
                        ))}
                      </div>
                    );
                  })}
                {notices && entries.length === 0 && <p className="licenses-empty">{t("licenses.noMatches")}</p>}
              </div>
            </div>
            <div className="licenses-detail">
              {notices && selected && (
                <>
                  <div className="licenses-heading">
                    <strong>
                      {selected.pkg.name} {selected.pkg.version}
                    </strong>
                    <span>{selected.pkg.license}</span>
                    {selected.pkg.url && (
                      <button
                        type="button"
                        className="link"
                        onClick={() => void openUrl(selected.pkg.url!).catch(console.error)}
                      >
                        {selected.pkg.url}
                      </button>
                    )}
                  </div>
                  <pre className="licenses-text">
                    {selected.pkg.texts.map((i) => notices.texts[i]).join("\n\n" + "-".repeat(40) + "\n\n")}
                  </pre>
                </>
              )}
            </div>
          </div>
        )}
        <footer>
          <span className="grow" />
          <button type="button" className="primary" onClick={onClose}>
            {t("common.close")}
          </button>
        </footer>
      </div>
    </Modal>
  );
}
