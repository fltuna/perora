// One monitor's part of the selection screen: the capture of that monitor, dimmed, with what would be
// chosen left bright. It draws and reports; where the pointer is and what a press means is decided by
// the application, which knows the whole virtual screen. This window only knows its own monitor.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { translator } from "./backend";
import { type Box, Pointer, type Step, Tour, TourButton, touring, useTour } from "./tour";

interface Area {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The part that changes while the screen is up; also what `selection-changed` carries. */
interface Now {
  id: number;
  mode: string;
  highlight: Area | null;
  dragging: boolean;
  /** Whether the tour is on, on the monitor with the bar. */
  touring: boolean;
}

interface Selection extends Now {
  /** Which monitor this window covers, where that is on the virtual screen (physical pixels). */
  monitor: number;
  area: Area;
  scale: number;
  /** Whether this is the window with the mode bar. */
  home: boolean;
  modes: string[];
  /** The windows that can be chosen, front to back, and where each is on the virtual screen. */
  windows: { index: number; title: string; width: number; height: number; area: Area }[];
}

interface View {
  language: string;
  texts: Record<string, string>;
  selection: Selection | null;
  /** The tours that were seen, by their names. */
  toursSeen: string[];
}

type Pointer = "down" | "move" | "up" | "leave";

function useSelection(): View | null {
  const [view, setView] = useState<View | null>(null);
  const known = useRef<number | null>(null);

  useEffect(() => {
    let stopped = false;
    const load = () =>
      invoke<View>("selection_state").then((next) => {
        if (stopped) return;
        known.current = next.selection?.id ?? null;
        setView(next);
      });

    void load();
    const listening = listen<Now | null>("selection-changed", ({ payload }) => {
      // The highlight of the selection on screen comes with the event; anything else (a new
      // selection, none, another language) is asked for whole.
      if (payload && payload.id === known.current) {
        setView((current) =>
          current?.selection ? { ...current, selection: { ...current.selection, ...payload } } : current,
        );
      } else {
        void load();
      }
    });
    return () => {
      stopped = true;
      void listening.then((stop) => stop());
    };
  }, []);

  return view;
}

export function Overlay() {
  const view = useSelection();
  const selection = view?.selection ?? null;
  const id = selection?.id ?? null;

  useEffect(() => {
    if (view) document.documentElement.lang = view.language;
  }, [view?.language]);

  // Escape gives up, 1 to 5 pick a mode. Only the window with the keyboard hears them.
  useEffect(() => {
    if (!selection) return;
    const onKey = (event: KeyboardEvent) => {
      // (While the tour is on, the keys are the tour's: on the monitor it is on, and nothing on the
      // others, should one of them have the keyboard.)
      if (event.repeat || touring() || selection.touring) return;
      if (event.key === "Escape") void invoke("selection_cancel", { id: selection.id });
      const mode = selection.modes[Number(event.key) - 1];
      if (mode && !event.ctrlKey && !event.altKey) {
        void invoke("selection_mode", { id: selection.id, mode });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [id, selection?.modes.join(), selection?.touring]);

  // Pointer moves come faster than they can be drawn: one report a frame is enough.
  const pending = useRef(false);
  const report = (what: Pointer) => {
    if (id === null) return;
    if (what !== "move") return void invoke("selection_pointer", { id, what });
    if (pending.current) return;
    pending.current = true;
    requestAnimationFrame(() => {
      pending.current = false;
      void invoke("selection_pointer", { id, what });
    });
  };

  // The tour of the screen, on the monitor with the mode bar, once its capture is drawn. While it is
  // on, the application is told: nothing is chosen on any monitor meanwhile.
  const [drawnFor, setDrawnFor] = useState<number | null>(null);
  const tour = useTour("capture", view?.toursSeen, !!selection?.home && drawnFor === id);
  useEffect(() => {
    if (!tour.on || id === null) return;
    void invoke("selection_touring", { id, on: true });
    return () => void invoke("selection_touring", { id, on: false });
  }, [tour.on, id]);

  if (!view || !selection) return <div className="overlay" />;
  const t = translator(view.texts);
  const { area, scale } = selection;

  // From the virtual screen to this window: its monitor's corner is its origin, and a pixel of the
  // page is `scale` pixels of the screen.
  const local = (rect: Area) => ({
    left: (rect.x - area.x) / scale,
    top: (rect.y - area.y) / scale,
    width: rect.width / scale,
    height: rect.height / scale,
  });
  const highlight = selection.highlight;

  return (
    <div
      className={`overlay mode-${selection.mode}`}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        // The press belongs to this window until it ends, wherever the pointer goes (another monitor).
        event.currentTarget.setPointerCapture(event.pointerId);
        report("down");
      }}
      onPointerMove={() => report("move")}
      onPointerUp={(event) => {
        if (event.button === 0) report("up");
      }}
      onPointerCancel={() => report("leave")}
    >
      <img
        key={selection.id}
        className="still"
        src={convertFileSrc(`selection/${selection.id}/monitor/${selection.monitor}.bmp`, "perora")}
        alt=""
        draggable={false}
        // Shown only once the capture is drawn: until then the window stays hidden.
        onLoad={() => {
          setDrawnFor(selection.id);
          void invoke("selection_ready", { id: selection.id });
        }}
        onError={() => void invoke("selection_ready", { id: selection.id })}
      />
      {tour.on ? null : highlight ? (
        <div className={selection.dragging ? "chosen dragging" : "chosen"} style={local(highlight)}>
          {selection.dragging && (
            <span className="size">
              {highlight.width} × {highlight.height}
            </span>
          )}
        </div>
      ) : (
        <div className="dim" />
      )}
      {selection.home && selection.mode === "window" && !tour.on && <WindowList selection={selection} t={t} />}
      {selection.home && (
        <ModeBar
          selection={selection}
          t={t}
          onMode={(mode) => void invoke("selection_mode", { id: selection.id, mode })}
          onCancel={() => void invoke("selection_cancel", { id: selection.id })}
          onTour={tour.start}
        />
      )}
      {tour.on && <Tour steps={captureSteps(selection, local, t)} t={t} onEnd={tour.end} />}
    </div>
  );
}

/** The tour of the screen: the modes, what a click on a window and a drag do in the smart mode,
    and how to give up. The click is shown on a window that is on the screen, when there is one fit
    to show it on; else on a window drawn for the purpose. */
function captureSteps(selection: Selection, local: (rect: Area) => Box, t: ReturnType<typeof translator>): Step[] {
  const [wide, high] = [window.innerWidth, window.innerHeight];
  const shown = example(selection);
  const frame = shown
    ? local(shown)
    : { left: Math.round(wide * 0.28), top: Math.round(high * 0.3), width: Math.round(wide * 0.44), height: Math.round(high * 0.44) };
  const dragged = {
    left: Math.round(wide * 0.32),
    top: Math.round(high * 0.36),
    width: Math.round(wide * 0.34),
    height: Math.round(high * 0.3),
  };
  return [
    { at: "modes", text: t("tour.capture.modes") },
    {
      at: frame,
      text: t("tour.capture.window"),
      art: (
        <>
          {!shown && <SampleWindow box={frame} t={t} />}
          <span
            className="cursor click"
            style={{ left: frame.left + frame.width / 2, top: frame.top + frame.height / 2 }}
          >
            <Pointer />
          </span>
        </>
      ),
    },
    {
      at: dragged,
      look: "drag",
      text: t("tour.capture.drag"),
      art: (
        <span
          className="cursor drag"
          style={
            {
              left: dragged.left,
              top: dragged.top,
              "--width": `${dragged.width}px`,
              "--height": `${dragged.height}px`,
            } as React.CSSProperties
          }
        >
          <Pointer />
          {/* (The size, as the screen says it while a frame is dragged: in pixels of the screen.) */}
          <span className="size">
            {Math.round(dragged.width * selection.scale)} × {Math.round(dragged.height * selection.scale)}
          </span>
        </span>
      ),
    },
    { at: "cancel", text: t("tour.capture.cancel") },
    { at: "guide", text: t("tour.again") },
  ];
}

/** The window the tour shows a click on: the first, front to back, that is on this monitor (most of
    it), neither small nor as large as the monitor, and in sight where it is clicked (no other in
    front of it there: a click there would be on that one). */
function example(selection: Selection): Area | null {
  const { area } = selection;
  const inside = (one: Area, x: number, y: number) => x >= one.x && x < one.x + one.width && y >= one.y && y < one.y + one.height;
  for (const { area: one } of selection.windows) {
    const [left, top] = [Math.max(one.x, area.x), Math.max(one.y, area.y)];
    const right = Math.min(one.x + one.width, area.x + area.width);
    const bottom = Math.min(one.y + one.height, area.y + area.height);
    if (right <= left || bottom <= top) continue;
    if ((right - left) * (bottom - top) < 0.8 * one.width * one.height) continue;
    const [across, down] = [one.width / area.width, one.height / area.height];
    if (across < 0.15 || down < 0.15 || (across > 0.92 && down > 0.92)) continue;
    const [x, y] = [one.x + one.width / 2, one.y + one.height / 2];
    if (selection.windows.find((window) => inside(window.area, x, y))?.area !== one) continue;
    return one;
  }
  return null;
}

/** A window drawn for the tour, where there is none on the screen to show it on. */
function SampleWindow({ box, t }: { box: Box; t: ReturnType<typeof translator> }) {
  return (
    <div className="sample-window" style={box}>
      <div className="title">
        <span>{t("tour.capture.sample")}</span>
        <span className="icon" aria-hidden="true">
          {"\uE921  \uE922  \uE8BB"}
        </span>
      </div>
      <div className="body">
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

/** The windows, each with its name above its picture, the way the system's own task view lays them
    out. The one way to choose in the window mode: nothing on the screen behind it reacts. */
function WindowList({ selection, t }: { selection: Selection; t: ReturnType<typeof translator> }) {
  const { id, windows } = selection;
  // What happens on a card is the card's: not a press on the screen behind it.
  const own = (event: React.PointerEvent) => event.stopPropagation();

  // As large as the monitor allows: the pictures share one height, the largest at which every
  // window still has its place.
  const list = useRef<HTMLUListElement>(null);
  const [height, setHeight] = useState(0);
  const shapes = windows.map((window) => `${window.width}x${window.height}`).join();
  useLayoutEffect(() => {
    const fit = () => {
      const room = list.current?.getBoundingClientRect();
      if (room) setHeight(pictureHeight(windows.map(aspect), room.width, room.height));
    };
    fit();
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [shapes]);

  if (windows.length === 0) return <p className="windows empty">{t("overlay.windows.none")}</p>;
  return (
    <ul
      ref={list}
      className="windows"
      // Scrolling the list, by its bar too, is the list's as well.
      onPointerDown={own}
      onPointerMove={own}
      onPointerUp={own}
      style={{ "--picture-height": `${height}px`, visibility: height ? "visible" : "hidden" } as React.CSSProperties}
    >
      {windows.map((window) => (
        <li key={window.index}>
          <button
            onPointerDown={own}
            onPointerMove={own}
            onPointerUp={own}
            onClick={() => void invoke("selection_window", { id, window: window.index })}
          >
            <span className="title">{window.title || t("overlay.windows.untitled")}</span>
            <WindowPicture
              // Its own for every selection: nothing of the one before is kept.
              key={`${id}-${window.index}`}
              src={convertFileSrc(`selection/${id}/window/${window.index}.png`, "perora")}
              aspect={aspect(window)}
            />
          </button>
        </li>
      ))}
    </ul>
  );
}

// A picture that is not made yet is asked for again after this many milliseconds, this many times
// at most: one that never comes is not waited for forever.
const ASK_AGAIN = { after: 200, times: 100 };

/** A window's picture in its place in the list. The place has the picture's shape from the start, so
    that the list does not jump as pictures arrive; until one has, a ring turns there. The pictures
    are made while the screen is up: one that is not there yet is asked for again. */
function WindowPicture({ src, aspect }: { src: string; aspect: number }) {
  const [loaded, setLoaded] = useState(false);
  const [asked, setAsked] = useState(0);
  const waiting = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(waiting.current), []);

  return (
    <span className={loaded ? "picture loaded" : "picture"} style={{ aspectRatio: `${aspect}` }}>
      {!loaded && <span className="ring" role="progressbar" />}
      <img
        // Another address each time, or the answer of the time before is given again.
        src={asked ? `${src}?again=${asked}` : src}
        alt=""
        draggable={false}
        onLoad={() => setLoaded(true)}
        onError={() => {
          if (asked >= ASK_AGAIN.times) return setLoaded(true);
          waiting.current = window.setTimeout(() => setAsked(asked + 1), ASK_AGAIN.after);
        }}
      />
    </span>
  );
}

// What a card adds around its picture, and what lies between cards, in pixels of the page. These
// mirror the style sheet (`.overlay .windows`): padding and border of a card, the name above the
// picture, the gap between cards.
const CARD = { sides: 24, above: 46, below: 12, gap: 20 };

/** No picture wider or taller than this many times its other side: a very wide window (a bar across
    the monitor) or a very tall one would otherwise set the height for all. */
const MOST_OBLONG = 2.4;

function aspect(window: { width: number; height: number }) {
  const ratio = window.width / Math.max(window.height, 1);
  return Math.min(Math.max(ratio, 1 / MOST_OBLONG), MOST_OBLONG);
}

/** The largest height the pictures can share so that the cards, wrapped into rows, fit the room. */
function pictureHeight(aspects: number[], roomWidth: number, roomHeight: number) {
  const fits = (height: number) => {
    let rows = 1;
    let used = 0;
    for (const ratio of aspects) {
      const card = height * ratio + CARD.sides;
      if (card > roomWidth) return false;
      if (used > 0 && used + CARD.gap + card > roomWidth) {
        rows += 1;
        used = 0;
      }
      used += (used > 0 ? CARD.gap : 0) + card;
    }
    return rows * (height + CARD.above + CARD.below) + (rows - 1) * CARD.gap <= roomHeight;
  };

  // Fitting gets no easier as the pictures grow: the largest height that fits is found by halving.
  let [low, high] = [24, Math.max(24, roomHeight)];
  while (high - low > 1) {
    const middle = (low + high) / 2;
    if (fits(middle)) low = middle;
    else high = middle;
  }
  return Math.floor(low);
}

function ModeBar({
  selection,
  t,
  onMode,
  onCancel,
  onTour,
}: {
  selection: Selection;
  t: ReturnType<typeof translator>;
  onMode: (mode: string) => void;
  onCancel: () => void;
  onTour: () => void;
}) {
  const hint = `overlay.hint.${selection.mode}`;
  // What happens on the bar is the bar's: not a press on the screen behind it.
  const own = (event: React.PointerEvent) => event.stopPropagation();

  return (
    <div className="bar" data-tour="modes" onPointerDown={own} onPointerMove={own} onPointerUp={own}>
      <div className="modes" role="radiogroup">
        {selection.modes.map((mode, index) => (
          <button
            key={mode}
            role="radio"
            aria-checked={mode === selection.mode}
            className={mode === selection.mode ? "current" : undefined}
            onClick={() => onMode(mode)}
          >
            <kbd>{index + 1}</kbd>
            {t(`overlay.mode.${mode}`)}
          </button>
        ))}
        <button className="cancel" data-tour="cancel" onClick={onCancel}>
          {t("overlay.cancel")}
        </button>
        <TourButton onClick={onTour} t={t} />
      </div>
      <p className="hint">{t(hint) === hint ? "" : t(hint)}</p>
    </div>
  );
}
