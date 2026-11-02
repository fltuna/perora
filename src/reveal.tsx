// The screen on which the part of the monitor that brings the selection screen's mode bar back is
// set: over the whole monitor, as the selection screen is, with a still of it behind and the bar drawn
// where it is (owner, 2026-11-02: 「設定ボタン押したらキャプチャ画面みたいなのが出てくるイメージ！」).
// The part is drawn and adjusted as the editor's crop is, on a 16:9 screen: other monitors have it
// stretched (「比率は16:9基準で引き伸ばされるよって記載しておいてほしい」).
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { translator } from "./backend";
import { BAR_REVEAL, type Zone, ZoneEditor } from "./zone";

interface View {
  language: string;
  texts: Record<string, string>;
  zone: Zone;
  modes: string[];
  /** Which still is behind it; none while the screen is not up. */
  still: number | null;
}

export function RevealScreen() {
  const [view, setView] = useState<View | null>(null);
  // What is drawn so far: set only when it is saved.
  const [zone, setZone] = useState<Zone>(BAR_REVEAL);

  useEffect(() => {
    let stopped = false;
    const load = () =>
      invoke<View>("reveal_state").then((next) => {
        if (stopped) return;
        setView(next);
        setZone(next.zone);
      });
    void load();
    // Up again: what is set now, over a new still.
    const listening = listen("reveal-changed", () => void load());
    return () => {
      stopped = true;
      void listening.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (view) document.documentElement.lang = view.language;
  }, [view?.language]);

  const save = () => void invoke("reveal_done", { zone });
  const cancel = () => void invoke("reveal_done", { zone: null });

  // Enter saves, Escape gives up; the arrows are the part's (the stage has the keyboard).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.repeat) return;
      if (event.key === "Enter") save();
      if (event.key === "Escape") cancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!view || view.still === null) return <div className="overlay area-screen" />;
  const t = translator(view.texts);
  const still = view.still;
  // What is pressed on the bar of buttons is not a press on the screen.
  const own = (event: React.PointerEvent) => event.stopPropagation();

  return (
    <div className="overlay area-screen">
      <ZoneEditor
        label={t("reveal.area")}
        value={zone}
        onChange={setZone}
        near={10}
        className="stage"
      >
        <img
          key={still}
          className="still"
          src={convertFileSrc(`reveal/${still}.bmp`, "perora")}
          alt=""
          draggable={false}
          onLoad={() => {
            void invoke("reveal_ready", { still });
            // (The keyboard to the part: its arrows.)
            document.querySelector<HTMLElement>(".area-screen .stage")?.focus();
          }}
          onError={() => void invoke("reveal_ready", { still })}
        />
        <div className="dim" />
        {/* The bar as the selection screen has it, where it has it: to set the part by. */}
        <div className="bar" aria-hidden="true">
          <div className="modes">
            {view.modes.map((mode, index) => (
              <button key={mode} tabIndex={-1} className={index === 0 ? "current" : undefined}>
                <kbd>{index + 1}</kbd>
                {t(`overlay.mode.${mode}`)}
              </button>
            ))}
            <button tabIndex={-1} className="cancel">
              {t("overlay.cancel")}
            </button>
          </div>
        </div>
      </ZoneEditor>
      <div className="area-screen-tools" onPointerDown={own} onPointerMove={own} onPointerUp={own}>
        <p>{t("reveal.hint")}</p>
        <div className="modes">
          <button className="current" onClick={save}>
            {t("reveal.save")}
          </button>
          <button onClick={() => setZone(BAR_REVEAL)}>{t("reveal.reset")}</button>
          <button className="cancel" onClick={cancel}>
            {t("reveal.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
