import { useState } from "react";

import { ProfileDialog, type ProfileDefaults } from "../components/ProfileDialog";
import type { Profile } from "../ipc/api";
import { findPane } from "../lib/panes";
import { loginName } from "../lib/sessions";
import type { TabStore } from "../lib/tabs";

interface Options {
  /** The local user's name, for quick SSH connections without one. */
  username: string;
  /** Deletes a session the user has confirmed deleting in its dialog (see `useProfileDeletion`). */
  remove(profile: Profile): Promise<boolean>;
}

/**
 * The session editor: editing a saved session, making a new one, and saving a quick
 * connection as a session, which its pane then connects to. `dialog` is what to render.
 */
export function useProfileEditor(store: TabStore, options: Options) {
  // An existing profile, or a new one (null) with defaults; `paneKey` is the quick connection
  // pane being saved as a session.
  const [editing, setEditing] = useState<{ profile: Profile | null; defaults?: ProfileDefaults; paneKey?: number } | null>(null);

  const edit = (profile: Profile) => setEditing({ profile });

  /** A new session, in `folder` if one is given. */
  const create = (folder: string | undefined) => setEditing({ profile: null, defaults: { folder } });

  const saveAsSession = (key: number) => {
    const pane = findPane(store.get().tabs, key)?.pane;
    if (pane?.target.kind !== "quick") return;
    const { protocol, host, port } = pane.target;
    setEditing({ profile: null, defaults: { protocol, host, port, username: loginName(pane.target, options.username) }, paneKey: key });
  };

  // A quick connection saved as a session becomes that session's pane; the connection stays.
  const onSaved = (profile: Profile) => {
    const key = editing?.paneKey;
    if (key === undefined) return;
    store.dispatch({ type: "updatePane", key, patch: { target: { kind: "profile", profileId: profile.id }, title: profile.name } });
  };

  const dialog = editing && (
    <ProfileDialog
      profile={editing.profile}
      defaults={editing.defaults}
      onClose={() => setEditing(null)}
      onSaved={onSaved}
      onDelete={options.remove}
    />
  );

  return { edit, create, saveAsSession, dialog };
}
