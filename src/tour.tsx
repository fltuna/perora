// A tour: what one of the windows shows of itself, a step at a time. The window is dimmed but for
// what a step is about, and beside that is what there is to say of it. It comes once on its own,
// the first time the window is seen, and again whenever its button (an "i") is pressed. Seen to
// the end or put away, it does not come on its own again; a window that goes out of sight while it
// is on leaves it unseen.
import { invoke } from "@tauri-apps/api/core";
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { translator } from "./backend";

type Translate = ReturnType<typeof translator>;

export type TourName = "capture" | "preview" | "editor";

/** A place in the window, in pixels of the page. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Step {
  /** What it is about: the `data-tour` name of something in the window, or a place in it. None for
      a step about nothing in particular, said in the middle of the window. */
  at?: string | Box;
  text: string;
  /** How what it is about is shown: as a frame (the default), as a frame being dragged open, or
      as a ring alone, nothing dimmed (for what is shown on the rest of the window meanwhile). */
  look?: "frame" | "drag" | "ring";
  /** Drawn where it is about, under the dimming and what is said: what it is about, when that is
      not in the window (a window drawn for the purpose), and a pointer showing what to do there. */
  art?: React.ReactNode;
}

/** Whether a tour is on: whoever listens for keys or the pointer leaves them alone meanwhile
    (asked of the page, so that whoever listens need not be told). */
export const touring = () => document.querySelector(".tour") !== null;

/** The tours seen in this window since it was opened: as said, before the application says so. */
const seenHere = new Set<TourName>();

/** A tour of this window: `on` while it is shown, `start` to show it, `end` once it is over (seen
    to the end or put away). It starts on its own, a moment after `ready` and the window is in sight,
    unless the application says it was seen. */
export function useTour(name: TourName, seen: string[] | undefined, ready: boolean) {
  const [on, setOn] = useState(false);
  const known = seen !== undefined;
  const already = seenHere.has(name) || (seen ?? []).includes(name);

  useEffect(() => {
    if (!ready || !known || already || on) return;
    let waiting: number | undefined;
    const begin = () => {
      window.clearTimeout(waiting);
      if (document.hidden) return;
      // (A moment after the window came, so that it is seen to come first.)
      waiting = window.setTimeout(() => setOn(true), 400);
    };
    begin();
    document.addEventListener("visibilitychange", begin);
    return () => {
      window.clearTimeout(waiting);
      document.removeEventListener("visibilitychange", begin);
    };
  }, [ready, known, already, on]);

  // Out of sight while it is on: it is over, and not seen.
  useEffect(() => {
    if (!on) return;
    const gone = () => {
      if (document.hidden) setOn(false);
    };
    document.addEventListener("visibilitychange", gone);
    return () => document.removeEventListener("visibilitychange", gone);
  }, [on]);

  return {
    on,
    start: () => setOn(true),
    end: () => {
      setOn(false);
      if (seenHere.has(name)) return;
      seenHere.add(name);
      void invoke("tour_seen", { tour: name }).catch(() => {});
    },
  };
}

/** The button a tour is shown again by: an "i" in a ring. */
export function TourButton({ onClick, t, className }: { onClick: () => void; t: Translate; className?: string }) {
  return (
    <button
      className={className ? `guide ${className}` : "guide"}
      data-tour="guide"
      title={t("tour.open")}
      aria-label={t("tour.open")}
      onClick={onClick}
    >
      <span className="icon" aria-hidden="true">
        {""}
      </span>
    </button>
  );
}

// Around what a step is about, in pixels of the page; and between that and what is said of it, and
// what is said and the edge of the window.
const AROUND = 6;
const GAP = 10;
const EDGE = 8;

/** Where a step is about, in the window (with room around it, when it is something in the window):
    none for a step about nothing. */
function placeOf(step: Step): Box | null {
  if (step.at === undefined) return null;
  // (A place given is where it is, to the pixel: a window on the screen, a frame being dragged.)
  if (typeof step.at !== "string") return step.at;
  const element = document.querySelector(`[data-tour="${step.at}"]`);
  if (!element) return null;
  const box = element.getBoundingClientRect();
  if (box.width === 0 && box.height === 0) return null;
  return {
    left: box.left - AROUND,
    top: box.top - AROUND,
    width: box.width + 2 * AROUND,
    height: box.height + 2 * AROUND,
  };
}

const same = (one: Box | null, other: Box | null) =>
  one === other ||
  (one !== null &&
    other !== null &&
    one.left === other.left &&
    one.top === other.top &&
    one.width === other.width &&
    one.height === other.height);

/** Where what is said goes: under what it is about, or over it, or beside it, wherever it fits
    whole; failing that, wherever there is most room. Always in the window. */
function sayingAt(place: Box | null, width: number, height: number): { left: number; top: number } {
  const [roomX, roomY] = [window.innerWidth, window.innerHeight];
  const within = (left: number, top: number) => ({
    left: Math.max(EDGE, Math.min(left, roomX - width - EDGE)),
    top: Math.max(EDGE, Math.min(top, roomY - height - EDGE)),
  });
  if (!place) return within((roomX - width) / 2, (roomY - height) / 2);
  const middle = place.left + place.width / 2 - width / 2;
  const centre = place.top + place.height / 2 - height / 2;
  const below = roomY - (place.top + place.height) - GAP - EDGE;
  const above = place.top - GAP - EDGE;
  const right = roomX - (place.left + place.width) - GAP - EDGE;
  const left = place.left - GAP - EDGE;
  if (below >= height) return within(middle, place.top + place.height + GAP);
  if (above >= height) return within(middle, place.top - GAP - height);
  if (right >= width) return within(place.left + place.width + GAP, centre);
  if (left >= width) return within(place.left - GAP - width, centre);
  // (Nowhere whole: over the edge of what it is about, on the side with most room.)
  return below >= above
    ? within(middle, roomY - height - EDGE)
    : within(middle, EDGE);
}

/** The tour itself, over the whole window. */
export function Tour({ steps, t, onEnd }: { steps: Step[]; t: Translate; onEnd: () => void }) {
  const [at, setAt] = useState(0);
  const step = steps[Math.min(at, steps.length - 1)];
  const last = at >= steps.length - 1;
  const next = () => (last ? onEnd() : setAt(at + 1));
  const back = () => setAt(Math.max(0, at - 1));

  // Where the step is about, as the window is now: looked at again after each drawing (what it is
  // about may have moved) and as the window changes size. And what is said, measured once it is
  // there and placed by that. (Each kept as it was when it is where it was.)
  const [place, setPlace] = useState<Box | null>(null);
  const saying = useRef<HTMLDivElement>(null);
  const [said, setSaid] = useState<{ left: number; top: number } | null>(null);
  const [, setResized] = useState(0);
  useLayoutEffect(() => {
    const now = placeOf(step);
    setPlace((was) => (same(was, now) ? was : now));
    const box = saying.current?.getBoundingClientRect();
    if (!box) return;
    const there = sayingAt(now, box.width, box.height);
    setSaid((was) => (was && was.left === there.left && was.top === there.top ? was : there));
  });
  useEffect(() => {
    const resized = () => setResized((times) => times + 1);
    window.addEventListener("resize", resized);
    return () => window.removeEventListener("resize", resized);
  }, []);
  // (And again a few times as a step starts: what it is about may come, or move into its place,
  // only after it has started.)
  useEffect(() => {
    const again = [60, 200, 400].map((after) => window.setTimeout(() => setResized((times) => times + 1), after));
    return () => again.forEach((one) => window.clearTimeout(one));
  }, [at]);

  // The keys are the tour's while it is on: → or Enter for the next step, ← for the one before,
  // Escape to put it away. No other key does anything meanwhile (Tab goes from button to button of
  // the tour, and Alt is the system's). Escape held down puts it away once, and does nothing more.
  const keys = useRef({ next, back, onEnd });
  keys.current = { next, back, onEnd };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey) return;
      // (Tab goes from one of the tour's buttons to the next, and never to what is under the tour.)
      if (event.key === "Tab") {
        event.preventDefault();
        event.stopImmediatePropagation();
        const buttons = [...document.querySelectorAll<HTMLButtonElement>(".tour .saying button")];
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[(at + (event.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.repeat || event.isComposing) return;
      if (event.key === "Escape") keys.current.onEnd();
      else if (event.key === "ArrowRight" || event.key === "Enter") keys.current.next();
      else if (event.key === "ArrowLeft") keys.current.back();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Nothing pressed on the tour reaches what is under it.
  const own = (event: React.SyntheticEvent) => event.stopPropagation();
  const look = step.look ?? "frame";

  return (
    <div
      className="tour"
      role="dialog"
      aria-modal="true"
      aria-label={t("tour.open")}
      onPointerDown={own}
      onPointerMove={own}
      onPointerUp={own}
      onMouseDown={own}
      onClick={own}
      onDoubleClick={own}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      onWheel={own}
    >
      {/* (Its own for each step: what one step set going is not another's.) */}
      <Fragment key={at}>{step.art}</Fragment>
      <div
        // (Another one for a frame dragged open, so that it starts from nothing each time.)
        key={look === "drag" ? `drag ${at}` : "frame"}
        className={place ? `spot ${look}` : "spot none"}
        style={
          place
            ? ({
                left: place.left,
                top: place.top,
                width: place.width,
                height: place.height,
                "--width": `${place.width}px`,
                "--height": `${place.height}px`,
              } as React.CSSProperties)
            : { left: window.innerWidth / 2, top: window.innerHeight / 2, width: 0, height: 0 }
        }
      />
      <div
        ref={saying}
        className="saying"
        style={said ? { left: said.left, top: said.top } : { visibility: "hidden" }}
      >
        <p key={at}>{step.text}</p>
        <div className="foot">
          <span className="count">{t("tour.count", { at: at + 1, of: steps.length })}</span>
          {!last && (
            <button className="quiet" onClick={onEnd}>
              {t("tour.skip")}
            </button>
          )}
          {at > 0 && <button onClick={back}>{t("tour.back")}</button>}
          <button
            className="primary"
            // (The keyboard is the tour's from the start: the next step is a key away.)
            autoFocus
            onClick={next}
          >
            {t(last ? "tour.done" : "tour.next")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** The pointer, as the system draws it: its tip at the corner it is put at. */
export function Pointer() {
  return (
    <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
      <path d="M2 1.5v15.2l4.1-3.9 2.6 5.9 2.6-1.1-2.6-5.8h5.8z" fill="#fff" stroke="#000" strokeWidth="1.2" strokeLinejoin="round" />
    </svg>
  );
}

/** A mouse, beside the pointer: what is done with it. Its other (right) button held down, or pressed
    twice; its wheel turning away from the hand (`up`) or towards it (`down`). */
export function Mouse({ held, wheel }: { held?: Held; wheel?: "up" | "down" }) {
  return (
    <span className={`mouse${held ? ` ${held}` : ""}${wheel ? ` wheel-${wheel}` : ""}`}>
      <svg width="44" height="66" viewBox="0 0 44 66" aria-hidden="true">
        <defs>
          <clipPath id="tour-wheel">
            <rect x="17" y="8" width="10" height="20" rx="5" />
          </clipPath>
        </defs>
        <rect x="1" y="1" width="42" height="64" rx="21" fill="#f4f4f4" stroke="#222" strokeWidth="1.6" />
        <path className={held === "left" ? "button left" : "button"} d={held === "left" ? "M22 1.8A20.2 20.2 0 0 0 1.8 22v12H22z" : "M22 1.8a20.2 20.2 0 0 1 20.2 20.2v12H22z"} />
        <path d="M1 34h42M22 1v33" stroke="#222" strokeWidth="1.4" fill="none" />
        <rect className="wheel" x="17" y="8" width="10" height="20" rx="5" />
        <g clipPath="url(#tour-wheel)">
          <g className="ridges">
            {[-8, -4, 0, 4, 8, 12, 16, 20, 24].map((at) => (
              <rect key={at} x="17" y={8 + at} width="10" height="2" fill="#fff" />
            ))}
          </g>
        </g>
        {/* Which way the wheel turns, in the round of the mouse under the buttons: arrows going
            that way. */}
        {wheel && (
          <g className="way">
            {[41, 48, 55].map((middle) => (
              <path key={middle} className="chevron" d={wheel === "up" ? `M16 ${middle + 3}l6-6 6 6` : `M16 ${middle - 3}l6 6 6-6`} />
            ))}
          </g>
        )}
      </svg>
    </span>
  );
}

/** What is done with a button of the mouse: the left one held, the other held, the other pressed
    twice. */
export type Held = "left" | "right" | "twice";

/** The pointer at (x, y) of the page, and the mouse beside it. With `moving`, it goes from where it
    was to there (as the hand moves it), not at once. */
export function Hand({ at, held, wheel, moving }: { at: { x: number; y: number }; held?: Held; wheel?: "up" | "down"; moving?: boolean }) {
  return (
    <>
      <span className={moving ? "cursor moving" : "cursor"} style={{ left: at.x, top: at.y }}>
        <Pointer />
      </span>
      <span className={moving ? "hand moving" : "hand"} style={{ left: at.x + 22, top: at.y + 16 }}>
        <Mouse held={held} wheel={wheel} />
      </span>
    </>
  );
}
