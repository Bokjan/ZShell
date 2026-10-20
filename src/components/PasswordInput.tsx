import { useState, type ComponentProps } from "react";
import { useTranslation } from "react-i18next";

import { IconButton } from "./IconButton";
import { EyeIcon } from "./icons";

type Props = Omit<ComponentProps<"input">, "type">;

/**
 * A password field with a button at its right end that shows what is typed, and hides it again.
 * The same on every platform: WebView2 adds a reveal button of its own (`::-ms-reveal`, hidden in
 * the stylesheet) that the other web views lack. The button is in the Tab order, after the field,
 * so that it works from the keyboard; a press on it keeps the focus in the field. A disabled
 * field shows dots.
 */
export function PasswordInput(props: Props) {
  const { t } = useTranslation();
  const [show, setShow] = useState(false);
  const shown = show && !props.disabled;
  return (
    <span className="password-input">
      <input {...props} type={shown ? "text" : "password"} autoCapitalize="off" autoCorrect="off" />
      <IconButton
        type="button"
        className="password-toggle"
        label={shown ? t("common.hidePassword") : t("common.showPassword")}
        disabled={props.disabled}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setShow(!shown)}
      >
        <EyeIcon crossed={!shown} />
      </IconButton>
    </span>
  );
}
