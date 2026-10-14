// What Perora has to say when no other window of its own is the place to say it: one line or a few,
// in a window in the corner that leaves on its own. A click on it sends it away at once.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
// (As a file of its own, never written into the page: the page may show pictures from files only.)
import icon from "../src-tauri/icons/128x128.png?no-inline";

interface View {
  language: string;
  /** What is said; nothing, while the window is out of sight. */
  message: string | null;
  /** Whether a click on it does more than send it away: it shows the file it is about. */
  opens: boolean;
  /** Which notice this is, of all that were said. */
  turn: number;
  /** The colour the user chose for their system, as `#rrggbb`. */
  accent: string | null;
}

export function Notice() {
  const [view, setView] = useState<View | null>(null);

  useEffect(() => {
    let stopped = false;
    const load = () =>
      invoke<View>("notice_state").then((next) => {
        if (!stopped) setView(next);
      });

    void load();
    const listening = listen("notice-changed", () => void load());
    // (Once more when it listens: what was said between the asking and the listening is not missed.)
    void listening.then(() => load());
    return () => {
      stopped = true;
      void listening.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (!view) return;
    document.documentElement.lang = view.language;
    if (view.accent) document.documentElement.style.setProperty("--system-accent", view.accent);
  }, [view?.language, view?.accent]);

  // Always there, with or without something said: a window left on screen with nothing in it can
  // still be clicked away, and what is said is said into a place that was there before it.
  return (
    <main
      className={view?.opens ? "said opens" : "said"}
      role="status"
      // (Which notice the click was on: another may have taken its place before the click is heard.)
      onClick={() => void invoke("notice_dismiss", { turn: view?.turn ?? null })}
    >
      {view?.message && (
        <>
          <img src={icon} alt="" draggable={false} />
          <p title={view.message}>{view.message}</p>
        </>
      )}
    </main>
  );
}
