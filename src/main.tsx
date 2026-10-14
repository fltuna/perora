import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MainWindow } from "./main_window";
import { Notice } from "./notice";
import { Overlay } from "./overlay";
import { Preview } from "./preview";
import "./style.css";

// These windows are parts of an application, not pages: no browser menu, no developer tools, no
// reloading or printing or searching from the keyboard.
window.addEventListener("contextmenu", (event) => event.preventDefault());
window.addEventListener(
  "keydown",
  (event) => {
    const key = event.key.toLowerCase();
    const browserKey =
      /^f([1-9]|1[0-2])$/.test(key) ||
      (event.ctrlKey && event.shiftKey && ["i", "j", "c"].includes(key)) ||
      (event.ctrlKey && ["r", "p", "f", "g", "u", "s", "+", "-", "=", "0"].includes(key));
    if (browserKey) event.preventDefault();
  },
  { capture: true },
);

// One page for every window; which window this is decides what it shows.
const label = getCurrentWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {label === "main" ? (
      <MainWindow />
    ) : label === "notice" ? (
      <Notice />
    ) : label.startsWith("overlay-") ? (
      <Overlay />
    ) : (
      <Preview />
    )}
  </React.StrictMode>,
);
