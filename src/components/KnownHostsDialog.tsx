import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { errorMessage, knownHosts, type KnownHost } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { ConfirmDialog } from "./ConfirmDialog";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

interface Props {
  onClose(): void;
  /** Entries were removed. */
  onChanged(): void;
}

const isHashed = (host: string) => host.startsWith("|1|");

/**
 * The host keys in `~/.ssh/known_hosts`, searchable (hashed host names by the name they hash,
 * as `ssh-keygen -F` finds them), each removable.
 */
export function KnownHostsDialog({ onClose, onChanged }: Props) {
  const { t } = useTranslation();
  // undefined while loading.
  const [entries, setEntries] = useState<KnownHost[] | undefined>(undefined);
  const [query, setQuery] = useState("");
  // Lines of hashed entries for the host typed in the search box, for the list they were
  // found in (removing an entry moves the lines after it).
  const [hashed, setHashed] = useState<{ entries: KnownHost[]; lines: number[] } | null>(null);
  const [removing, setRemoving] = useState<KnownHost | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    knownHosts.list().then(setEntries, (e) => {
      setEntries([]);
      setError(errorMessage(e));
    });
  }, []);
  useEffect(reload, [reload]);

  const dialog = useDialog(onClose);

  useEffect(() => {
    if (!entries?.some((entry) => entry.hosts.some(isHashed)) || !query.trim()) return;
    let current = true;
    knownHosts.find(query).then((lines) => current && setHashed({ entries, lines }), console.error);
    return () => {
      current = false;
    };
  }, [query, entries]);
  const hashedMatches = useMemo(
    () => (hashed && hashed.entries === entries && query.trim() ? hashed.lines : []),
    [hashed, entries, query],
  );

  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return (entries ?? []).filter((entry) => {
      if (hashedMatches.includes(entry.line)) return true;
      const text = [...entry.hosts.filter((host) => !isHashed(host)), entry.algorithm, entry.fingerprint, entry.comment]
        .join(" ")
        .toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [entries, query, hashedMatches]);

  const remove = (entry: KnownHost) => {
    setRemoving(null);
    knownHosts
      .remove(entry)
      .then(() => {
        setError(null);
        onChanged();
      })
      // Also when the file had changed: the list shows what is there now.
      .catch((e) => setError(errorMessage(e)))
      .finally(reload);
  };

  const removeMessage = (entry: KnownHost) => {
    const named = entry.hosts.filter((host) => !isHashed(host));
    if (named.length === 0) return t("knownHosts.removeMessageHashed", { algorithm: entry.algorithm });
    return t("knownHosts.removeMessage", { algorithm: entry.algorithm, hosts: named.join(", ") });
  };

  return (
    <Modal dialog={dialog}>
      <div className="dialog known-hosts-dialog">
        <h2>{t("knownHosts.title")}</h2>
        <input
          value={query}
          placeholder={t("knownHosts.search")}
          autoFocus
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        {error && <ErrorText className="panel-error">{error}</ErrorText>}
        <div className="known-hosts-list">
          {shown.map((entry) => (
            <div key={`${entry.line}:${entry.text}`} className="known-hosts-item">
              <div className="known-hosts-main">
                <span className="known-hosts-hosts">
                  {entry.hosts.map((host, i) =>
                    isHashed(host) ? (
                      <span key={i} className="known-hosts-hashed" title={t("knownHosts.hashedTip", { hash: host })}>
                        {t("knownHosts.hashed")}
                      </span>
                    ) : (
                      <span key={i} className="known-hosts-host">
                        {host}
                      </span>
                    ),
                  )}
                  {entry.marker && (
                    <span className="known-hosts-marker">
                      {entry.marker === "revoked" || entry.marker === "cert-authority"
                        ? t(`knownHosts.markers.${entry.marker}`)
                        : `@${entry.marker}`}
                    </span>
                  )}
                </span>
                <span className="known-hosts-key">
                  {entry.algorithm} · {entry.fingerprint ?? t("knownHosts.unreadableKey")}
                </span>
              </div>
              <button type="button" onClick={() => setRemoving(entry)}>
                {t("knownHosts.remove")}
              </button>
            </div>
          ))}
          {entries && shown.length === 0 && (
            <p className="known-hosts-empty">{t(entries.length === 0 ? "knownHosts.empty" : "knownHosts.noMatches")}</p>
          )}
        </div>
        <footer>
          {entries && <span className="hint">{t("knownHosts.count", { count: entries.length })}</span>}
          <span className="grow" />
          <button type="button" className="primary" onClick={onClose}>
            {t("common.close")}
          </button>
        </footer>
      </div>
      {removing && (
        <ConfirmDialog
          title={t("knownHosts.removeTitle")}
          message={removeMessage(removing)}
          confirmLabel={t("knownHosts.remove")}
          danger
          onConfirm={() => remove(removing)}
          onCancel={() => setRemoving(null)}
        />
      )}
    </Modal>
  );
}
