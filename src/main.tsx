import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { DebugLogWindow } from "./DebugLogWindow";
import { AppLockGate } from "./components/AppLockGate";
import { VaultGate } from "./components/VaultGate";
import { setupI18n } from "./i18n/index";
import { recordPageErrors } from "./ipc/debugLog";
import { applyTheme, SettingsProvider } from "./hooks/settings";
import { suppressWebViewMenu } from "./hooks/titleBar";

recordPageErrors();

// Until the stored settings load, follow the system appearance to avoid a flash of the
// wrong theme.
applyTheme(window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");

suppressWebViewMenu();

// The debug log window (see `window/` in the backend) loads the same page; it shows the log
// rather than the app, without the vault (it shows no sessions), but behind the app lock.
const debugLog = getCurrentWindow().label === "debug-log";

void setupI18n().then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <SettingsProvider>
        {debugLog ? (
          <AppLockGate>
            <DebugLogWindow />
          </AppLockGate>
        ) : (
          <VaultGate>
            <AppLockGate>
              <App />
            </AppLockGate>
          </VaultGate>
        )}
      </SettingsProvider>
    </React.StrictMode>,
  );
});
