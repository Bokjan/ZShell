import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";

import { useDialog } from "../lib/dialogs";
import { clampIndex, navigateList } from "../lib/listNavigation";
import { ExternalLinkIcon } from "./icons";
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
  const listId = useId();
  const optionId = (i: number) => `${listId}-${i}`;

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
  const index = clampIndex(
    entries.findIndex((entry) => entry.key === selectedKey),
    entries.length,
  );
  const selected = entries[index];

  useEffect(() => {
    listRef.current?.querySelector(".licenses-item.selected")?.scrollIntoView({ block: "nearest" });
  }, [selected?.key]);

  const onSearchKey = (e: KeyboardEvent) => {
    navigateList(e, entries.length, index, (next) => setSelectedKey(entries[next].key));
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
                role="combobox"
                aria-label={t("licenses.search")}
                aria-controls={listId}
                aria-expanded={entries.length > 0}
                aria-autocomplete="list"
                aria-activedescendant={index < 0 ? undefined : optionId(index)}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
              />
              {/* Grouped by ecosystem; a list without items, which shows a message instead, has no role (see Sidebar). */}
              <div
                className="licenses-list"
                ref={listRef}
                id={listId}
                role={entries.length > 0 ? "listbox" : undefined}
                aria-label={t("licenses.title")}
              >
                {notices &&
                  ECOSYSTEMS.map((ecosystem) => {
                    const group = entries.filter((entry) => entry.ecosystem === ecosystem);
                    if (group.length === 0 && !(ecosystem === "rust" && notices.rustMissing)) return null;
                    const title = t(`licenses.groups.${ecosystem}`, { count: group.length });
                    return (
                      <div key={ecosystem} role="group" aria-label={title}>
                        <div className="licenses-group" aria-hidden="true">
                          {title}
                        </div>
                        {ecosystem === "rust" && notices.rustMissing && (
                          <p className="licenses-empty" role="presentation">
                            {t("licenses.rustMissing")}
                          </p>
                        )}
                        {group.map((entry) => (
                          <div
                            key={entry.key}
                            id={optionId(entries.indexOf(entry))}
                            role="option"
                            aria-selected={entry === selected}
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
                        <ExternalLinkIcon />
                        <span className="visually-hidden">{t("common.opensInBrowser")}</span>
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
