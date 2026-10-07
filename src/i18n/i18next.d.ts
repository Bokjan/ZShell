import "i18next";

import type en from "../locales/en.json";

// Type-check translation keys and interpolation variables against the English catalog.
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: { translation: typeof en };
    interpolationPrefix: "{";
    interpolationSuffix: "}";
  }
}
