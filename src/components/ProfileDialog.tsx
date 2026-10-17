import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  DEFAULT_GROUP,
  DEFAULT_PORTS,
  defaultKeyPath,
  deleteProfile,
  errorMessage,
  keyFileExists,
  saveProfile,
  serialPorts,
  type CommandGroup,
  type Profile,
  type Protocol,
  type SerialPortInfo,
} from "../ipc/api";
import { useConfirmButton } from "../hooks/confirm";
import { useDialog } from "../hooks/dialogs";
import { directory } from "../lib/directory";
import { useDirectory } from "../hooks/stores";
import { open as openDialog } from "../ipc/fileDialog";
import { rovingTarget } from "../lib/listNavigation";
import { passwordUpdate } from "../lib/password";
import { contractHome, expandHome, startsWithHome } from "../lib/paths";
import { useHomeDirectory } from "../hooks/paths";
import {
  fromProfile,
  isChanged,
  PAGES,
  toProfile,
  validate,
  withProtocol,
  type Page,
  type ProfileDefaults,
  type ProfileForm,
} from "../lib/profileForm";
import { useSubmitting } from "../hooks/submitting";
import { attempt } from "../lib/attempt";
import { ConfirmDialog } from "./ConfirmDialog";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";
import { AppearancePage } from "./profile/AppearancePage";
import { ConnectionPage } from "./profile/ConnectionPage";
import { GeneralPage } from "./profile/GeneralPage";
import { TerminalPage } from "./profile/TerminalPage";
import { ProxyDialog } from "./ProxyDialog";

export type { ProfileDefaults } from "../lib/profileForm";

interface Props {
  /** null creates a new profile. */
  profile: Profile | null;
  defaults?: ProfileDefaults;
  /** To pick the quick command group its tabs show first; null until they have loaded. */
  commandGroups: CommandGroup[] | null;
  onClose(): void;
  /** Called with the saved profile (not on delete). */
  onSaved?(profile: Profile): void;
}

/**
 * Creates, edits or deletes a saved session, on four pages. The fields are one form object
 * (`lib/profileForm.ts`), checked as a whole when saving: the first problem shows on its
 * page, with its field focused. Closing with Esc or a click outside asks first if anything
 * was changed.
 */
export function ProfileDialog({ profile, defaults, commandGroups, onClose, onSaved }: Props) {
  const { t } = useTranslation();
  const [started] = useState(() => fromProfile(profile, defaults));
  const [form, setForm] = useState<ProfileForm>(started);
  const set = (patch: Partial<ProfileForm>) => setForm((current) => ({ ...current, ...patch }));
  // Saving and deleting, one at a time.
  const saving = useSubmitting();
  const [page, setPage] = useState<Page>("general");
  const pageId = useId();
  const tabsRef = useRef<HTMLDivElement>(null);
  const formRef = useRef<HTMLFormElement>(null);
  // All the sessions, to pick jump hosts from, and the proxies (another window's new ones too).
  const { loaded, profiles, proxies } = useDirectory();
  const [creatingProxy, setCreatingProxy] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  const home = useHomeDirectory();
  // The serial ports found here, listed once the serial protocol is chosen.
  const [ports, setPorts] = useState<SerialPortInfo[] | null>(null);
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const { ref: deleteRef, armed: deleteArmed, setArmed: setDeleteArmed, onBlur: onDeleteBlur } = useConfirmButton();
  const [error, setError] = useState<string | null>(null);
  const { protocol, authType } = form;
  const ssh = protocol === "ssh";
  // Automatic authentication falls back to a password, so it can keep a stored one too;
  // Telnet types it at the password prompt.
  const usesPassword = protocol === "telnet" || (ssh && (authType === "password" || authType === "auto"));
  // A group deleted since counts as the default group, as its tabs do. Until the groups have
  // loaded (or if they couldn't), the profile's stays as it is.
  const shownGroup = !commandGroups || commandGroups.some((group) => group.id === form.commandGroup) ? form.commandGroup : DEFAULT_GROUP;

  // A session set to use a key file starts with a default key found here, as automatic
  // authentication would use; with none, the field stays empty for the user to choose one.
  const [defaultKey, setDefaultKey] = useState<string | null>(null);
  useEffect(() => void defaultKeyPath().then(setDefaultKey, () => {}), []);
  useEffect(() => {
    if (ssh && authType === "publicKey" && !form.keyPath && defaultKey) set({ keyPath: defaultKey });
  }, [ssh, authType, defaultKey]);

  // A key file that isn't here is only pointed out: the session may be for another computer
  // (exported), or the key may be on a drive not mounted now. Checked once typing pauses.
  const keyPath = form.keyPath.trim();
  const [missingKey, setMissingKey] = useState<string | null>(null);
  useEffect(() => {
    if (!ssh || authType !== "publicKey" || !keyPath) return;
    let live = true;
    const timer = setTimeout(() => {
      void keyFileExists(keyPath).then(
        (exists) => live && setMissingKey(exists ? null : keyPath),
        () => {},
      );
    }, 300);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [ssh, authType, keyPath]);

  const refreshPorts = () => void serialPorts().then(setPorts, () => setPorts([]));
  useEffect(() => {
    if (protocol === "serial" && ports === null) refreshPorts();
  }, [protocol, ports]);

  // Starts in the folder of the current key, or in ~/.ssh; keys in the home folder are kept as
  // `~/…`, which works on other computers too (exported sessions).
  const chooseKey = async () => {
    let start: string | undefined = form.keyPath.trim() || "~/.ssh";
    if (startsWithHome(start)) start = home ? expandHome(start, home) : undefined;
    const picked = await openDialog({ title: t("profile.chooseKey"), defaultPath: start }).catch(() => null);
    if (typeof picked === "string") set({ keyPath: home ? contractHome(picked, home) : picked });
  };

  const changed = isChanged(form, started) || password !== "" || clearPassword;
  // Esc and a click outside: the Cancel button closes without asking.
  const dialog = useDialog(() => (changed ? setDiscarding(true) : onClose()));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    // Checked here rather than with `required`: the field may be on another page.
    const problem = validate(form, t);
    if (problem) {
      setPage(problem.page);
      setError(problem.message);
      // Once its page is shown.
      requestAnimationFrame(() => {
        const field = formRef.current?.elements.namedItem(problem.field);
        if (field instanceof HTMLElement) field.focus();
      });
      return;
    }
    const update = passwordUpdate({ keeps: usesPassword, existed: !!profile, clear: clearPassword, password });
    await saving.submit(async () => {
      return attempt(
        async () => {
          const saved = await saveProfile(toProfile(form, profile, defaults?.folder, shownGroup), update);
          void directory.reload();
          onSaved?.(saved);
          onClose();
        },
        {
          onError: (err) => {
            setError(errorMessage(err));
          },
        },
      );
    });
  };

  const remove = async () => {
    if (!profile) return;
    if (!deleteArmed) {
      setDeleteArmed(true);
      return;
    }
    await saving.submit(async () => {
      return attempt(
        async () => {
          await deleteProfile(profile.id);
          void directory.reload();
          onClose();
        },
        {
          onError: (err) => {
            setError(errorMessage(err));
          },
        },
      );
    });
  };

  const pages: Record<Page, ReactNode> = {
    general: (
      <GeneralPage
        form={form}
        set={set}
        changeProtocol={(next: Protocol) => setForm((current) => withProtocol(current, next, DEFAULT_PORTS))}
        profile={profile}
        password={{ value: password, set: setPassword, clear: clearPassword, setClear: setClearPassword }}
        usesPassword={usesPassword}
        home={home}
        chooseKey={() => void chooseKey()}
        keyMissing={missingKey !== null && missingKey === keyPath}
        ports={ports}
        refreshPorts={refreshPorts}
      />
    ),
    connection: (
      <ConnectionPage
        form={form}
        set={set}
        profile={profile}
        profiles={profiles}
        proxies={loaded ? proxies : null}
        newProxy={() => setCreatingProxy(true)}
      />
    ),
    terminal: <TerminalPage form={form} set={set} commandGroups={commandGroups} shownGroup={shownGroup} />,
    appearance: <AppearancePage form={form} set={set} />,
  };

  // The page tabs are one stop for Tab; the arrow keys (and Home / End) switch pages.
  const onTabKeyDown = (e: KeyboardEvent) => {
    const next = rovingTarget(e.key, PAGES.indexOf(page), PAGES.length, true);
    if (next === null) return;
    e.preventDefault();
    setPage(PAGES[next]);
    tabsRef.current?.querySelectorAll<HTMLElement>("[role=tab]")[next]?.focus();
  };

  return (
    <>
      <Modal dialog={dialog}>
        <form className="dialog profile-dialog" onSubmit={submit} ref={formRef}>
          <h2>{profile ? t("profile.titleEdit") : t("profile.titleNew")}</h2>
          <div className="segmented profile-pages" role="tablist" ref={tabsRef} onKeyDown={onTabKeyDown}>
            {PAGES.map((p) => (
              <button
                key={p}
                type="button"
                role="tab"
                id={`${pageId}-tab-${p}`}
                aria-controls={`${pageId}-${p}`}
                aria-selected={page === p}
                tabIndex={page === p ? 0 : -1}
                className={page === p ? "on" : undefined}
                onClick={() => setPage(p)}
              >
                {t(`profile.pages.${p}`)}
              </button>
            ))}
          </div>

          {/* All pages share one grid cell, so the dialog keeps the height of the tallest. */}
          <div className="profile-body">
            <div className="profile-stack">
              {PAGES.map((p) => (
                <div
                  key={p}
                  id={`${pageId}-${p}`}
                  className={`profile-page${p === page ? "" : " hidden"}`}
                  role="tabpanel"
                  aria-labelledby={`${pageId}-tab-${p}`}
                  aria-hidden={p !== page}
                >
                  {pages[p]}
                </div>
              ))}
            </div>
          </div>

          {error && <ErrorText>{error}</ErrorText>}

          <footer>
            {profile && (
              <button type="button" className="danger" ref={deleteRef} onClick={remove} onBlur={onDeleteBlur} disabled={saving.busy}>
                {deleteArmed ? t("profile.deleteConfirm") : t("common.delete")}
              </button>
            )}
            <span className="grow" />
            <button type="button" onClick={onClose}>
              {t("common.cancel")}
            </button>
            <button type="submit" className="primary" disabled={saving.busy}>
              {t("common.save")}
            </button>
          </footer>
        </form>
      </Modal>
      {creatingProxy && <ProxyDialog proxy={null} onClose={() => setCreatingProxy(false)} onSaved={(saved) => set({ proxy: saved.id })} />}
      {discarding && (
        <ConfirmDialog
          title={t("profile.discardTitle")}
          message={t("profile.discardMessage")}
          confirmLabel={t("profile.discard")}
          cancelLabel={t("profile.keepEditing")}
          danger
          onConfirm={onClose}
          onCancel={() => setDiscarding(false)}
        />
      )}
    </>
  );
}
