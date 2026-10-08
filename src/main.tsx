import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { setupI18n } from "./i18n";
import { applyTheme, SettingsProvider } from "./lib/settings";
import { suppressWebViewMenu } from "./lib/window";

// Until the stored settings load, follow the system appearance to avoid a flash of the
// wrong theme.
applyTheme(window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");

suppressWebViewMenu();

void setupI18n().then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <SettingsProvider>
        <App />
      </SettingsProvider>
    </React.StrictMode>,
  );
});
