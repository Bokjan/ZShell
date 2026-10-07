import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { setupI18n } from "./i18n";

void setupI18n().then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
