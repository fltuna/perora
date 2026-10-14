// What the tours of the two preview windows say and show. Where it is about what the mouse does on
// the picture, the window is had to do it, as the mouse would, with the pointer and the mouse drawn
// where it is done; and once the step is over, all of it is undone.
import { useEffect, useLayoutEffect, useState } from "react";
import type { Way, translator } from "./backend";
import type { EditorShow, Part, Sketch } from "./editor";
import { Hand, type Held, type Step } from "./tour";

type Translate = ReturnType<typeof translator>;
type Shown = React.MutableRefObject<EditorShow | null>;
type Choose = (way: Way | null) => void;

/** The tour of the large window: the tools, one by one (selecting last: it is about what the
    others drew), what the mouse does on the picture beyond drawing, looking closer, what is done
    with the capture and how it leaves. */
export const editorSteps = (offersUpload: boolean, show: Shown, choose: Choose, t: Translate): Step[] => [
  { at: "tools", text: t("tour.editor.tools") },
  ...(["pen", "marker", "shapes", "text", "stamp", "hide", "crop", "select"] as const).map((tool) => ({
    at: `tool-${tool}`,
    text: t(tool === "select" ? "tour.editor.select" : `tour.tool.${tool}`),
    look: "ring" as const,
    art: <Sketching key={tool} show={show} play={PLAYS[tool]} />,
  })),
  { at: "stage", text: t("tour.editor.wheel"), art: <Turning show={show} /> },
  { at: "stage", text: t("tour.editor.right"), art: <Asking show={show} /> },
  { at: "stage", text: t("tour.editor.zoom"), art: <Looking show={show} /> },
  { at: "actions", text: t(offersUpload ? "tour.editor.actions" : "tour.editor.actionsWithoutUpload") },
  { at: "as", text: t("tour.preview.as") },
  { at: "menu", text: t("tour.choice"), art: <Choosing choose={choose} way={offersUpload ? "upload" : "save"} /> },
  { at: "status", text: t("tour.editor.status") },
  { at: "guide", text: t("tour.again") },
];

/** The tour of the small window: the picture, what is done with the capture, and how it leaves. */
export const previewSteps = (offersUpload: boolean, choose: Choose, t: Translate): Step[] => [
  { at: "picture", text: t("tour.preview.picture") },
  { at: "actions", text: t(offersUpload ? "tour.preview.actions" : "tour.preview.actionsWithoutUpload") },
  { at: "as", text: t("tour.preview.as") },
  { at: "menu", text: t("tour.choice"), art: <Choosing choose={choose} way={offersUpload ? "upload" : "save"} /> },
  { at: "guide", text: t("tour.again") },
];

/** A place on the picture, in pixels of the page: so far across it and down it, as it is shown
    (again when the window changes size). */
function usePlace(across: number, down: number) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  useLayoutEffect(() => {
    const measure = () => {
      const sheet = document.querySelector(".picture.stage .sheet")?.getBoundingClientRect();
      if (!sheet || sheet.width === 0) return;
      const [x, y] = [Math.round(sheet.left + sheet.width * across), Math.round(sheet.top + sheet.height * down)];
      setAt((was) => (was && was.x === x && was.y === y ? was : { x, y }));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);
  return at;
}

/** Whether the system asks for less motion: then what the window is had to do is done once, and
    left so. */
const still = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Has the window do something for as long as the step is on, starting from how it is, and goes back
    to that once it is over. `start` returns what stops what it set going. Nothing is done there
    while something is under way that it would end (see `EditorShow.now`): `start` is told so
    (`acting` false), and the mouse alone shows it then. What is undone is undone in the window it
    was done in, while that is still the one. */
function useShowing(
  show: Shown,
  at: { x: number; y: number } | null,
  start: (at: { x: number; y: number }, editor: EditorShow, acting: boolean) => () => void,
) {
  useEffect(() => {
    const editor = show.current;
    if (!editor || !at) return;
    const was = editor.now();
    const stop = start(at, editor, !was.busy);
    return () => {
      stop();
      if (!was.busy && show.current === editor) editor.restore(was);
    };
  }, [at]);
}

/** The other button held, the wheel turned: three tools on, three back, and again. */
function Turning({ show }: { show: Shown }) {
  const at = usePlace(0.35, 0.4);
  const [wheel, setWheel] = useState<"up" | "down">("down");
  useShowing(show, at, (at, editor, acting) => {
    const ways: (1 | -1)[] = [1, 1, 1, -1, -1, -1];
    let turned = 0;
    const turn = () => {
      const by = ways[turned++ % ways.length];
      // (Towards the hand is on to the next.)
      setWheel(by === 1 ? "down" : "up");
      if (acting) editor.turn(at.x, at.y, by);
    };
    const first = window.setTimeout(turn, 500);
    const every = still() ? undefined : window.setInterval(turn, 750);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  });
  return at && <Hand at={at} held="right" wheel={wheel} />;
}

/** The other button pressed twice: the settings of the pen, there; a while later put away, and
    again. */
function Asking({ show }: { show: Shown }) {
  // (High on the picture: the settings open down from there, clear of what is said under it.)
  const at = usePlace(0.22, 0.1);
  // (Each time the presses are shown anew.)
  const [round, setRound] = useState(0);
  useShowing(show, at, (at, editor, acting) => {
    if (!acting) return () => {};
    editor.take("pen");
    let waiting = 0;
    const again = (first: boolean) => {
      if (!first) {
        editor.shut();
        setRound((round) => round + 1);
      }
      // (Once the presses are seen; and the pen in hand by then.)
      waiting = window.setTimeout(() => {
        editor.settingsAt(at.x, at.y);
        if (!still()) waiting = window.setTimeout(() => again(false), 2600);
      }, 1100);
    };
    again(true);
    return () => window.clearTimeout(waiting);
  });
  return at && <Hand key={round} at={at} held="twice" />;
}

/** The wheel turned away from the hand, the picture looked at closer; then back. */
function Looking({ show }: { show: Shown }) {
  const at = usePlace(0.45, 0.45);
  const [wheel, setWheel] = useState<"up" | "down" | undefined>(undefined);
  useShowing(show, at, (at, editor, acting) => {
    if (!acting) return () => {};
    if (still()) {
      // (Closer once, as ten notches would have it, and left so.)
      editor.zoom(at.x, at.y, -800);
      return () => {};
    }
    // Ten notches in, a rest, ten out, a rest: a notch every 90 ms.
    const plan = [...Array(10).fill(-80), ...Array(8).fill(0), ...Array(10).fill(80), ...Array(8).fill(0)] as number[];
    let tick = 0;
    const every = window.setInterval(() => {
      const deltaY = plan[tick++ % plan.length];
      setWheel(deltaY < 0 ? "up" : deltaY > 0 ? "down" : undefined);
      if (deltaY !== 0) editor.zoom(at.x, at.y, deltaY);
    }, 90);
    return () => window.clearInterval(every);
  });
  return at && <Hand at={at} wheel={wheel} />;
}

/** What the hand does at a moment of a play: where the pointer is, which button is held. */
type Move = { at: Part; held?: Held };

/** A play of the tour on the picture, as a tool would be used there: what is done, and when (in
    milliseconds from its start). Played again and again, from what the picture was, for as long as
    the step is on. */
type Play = { tool: "pen" | "marker" | "rectangle" | "text" | "stamp" | "mosaic" | "crop" | "select"; beats: { after: number; hand?: Move; act?: (editor: EditorShow, drawn: number[]) => void }[]; length: number };

/** The hand dragged from one place to another over so long, a beat every 40 ms: where it is at each. */
const dragged = (from: Part, to: Part, start: number, lasts: number, act?: (at: Part, editor: EditorShow, drawn: number[]) => void) => {
  const steps = Math.round(lasts / 40);
  return Array.from({ length: steps + 1 }, (_, step) => {
    const at: Part = [from[0] + ((to[0] - from[0]) * step) / steps, from[1] + ((to[1] - from[1]) * step) / steps];
    return { after: start + step * 40, hand: { at, held: "left" as const }, act: act && ((editor: EditorShow, drawn: number[]) => act(at, editor, drawn)) };
  });
};

/** The points of a wave from one place to another, as far as so much of it (0 to 1). */
const wave = (from: Part, to: Part, much: number): Part[] => {
  const count = Math.max(2, Math.round(much * 40));
  return Array.from({ length: count }, (_, step) => {
    const along = (step / (count - 1)) * much;
    return [from[0] + (to[0] - from[0]) * along, from[1] + (to[1] - from[1]) * along + Math.sin(along * Math.PI * 2) * 0.06];
  });
};

/** Draws or changes what the play drew first (the place it has kept in `drawn`). */
const drawn0 = (editor: EditorShow, drawn: number[], what: Sketch) => {
  drawn[0] = editor.sketch(drawn[0] ?? null, what);
};

const PLAYS: Record<"pen" | "marker" | "shapes" | "text" | "stamp" | "hide" | "crop" | "select", Play> = {
  pen: {
    tool: "pen",
    beats: [
      { after: 0, hand: { at: [0.3, 0.6] } },
      // (The hand along the wave, at its end as it is drawn so far.)
      ...Array.from({ length: 31 }, (_, step) => {
        const points = wave([0.3, 0.6], [0.7, 0.6], step / 30);
        return {
          after: 400 + step * 40,
          hand: { at: points[points.length - 1], held: "left" as const },
          act: (editor: EditorShow, drawn: number[]) => drawn0(editor, drawn, { tool: "pen", points }),
        };
      }),
      { after: 1700, hand: { at: [0.7, 0.6] } },
    ],
    length: 3400,
  },
  marker: {
    tool: "marker",
    beats: [
      { after: 0, hand: { at: [0.3, 0.55] } },
      ...dragged([0.3, 0.55], [0.7, 0.55], 400, 1000, (at, editor, drawn) => drawn0(editor, drawn, { tool: "marker", points: [[0.3, 0.55], at] })),
      { after: 1500, hand: { at: [0.7, 0.55] } },
    ],
    length: 3200,
  },
  shapes: {
    tool: "rectangle",
    beats: [
      { after: 0, hand: { at: [0.32, 0.42] } },
      ...dragged([0.32, 0.42], [0.66, 0.78], 400, 1000, (at, editor, drawn) => drawn0(editor, drawn, { tool: "rectangle", from: [0.32, 0.42], to: at })),
      { after: 1500, hand: { at: [0.66, 0.78] } },
    ],
    length: 3200,
  },
  text: {
    tool: "text",
    beats: [
      { after: 0, hand: { at: [0.35, 0.55] } },
      { after: 300, hand: { at: [0.35, 0.55], held: "left" } },
      // (Then out of the way of what is written.)
      { after: 450, hand: { at: [0.35, 0.55] } },
      { after: 550, hand: { at: [0.3, 0.75] } },
      ...Array.from("Perora", (_, at) => ({
        after: 700 + at * 160,
        act: (editor: EditorShow, drawn: number[]) => drawn0(editor, drawn, { tool: "text", at: [0.35, 0.55], text: "Perora".slice(0, at + 1) }),
      })),
    ],
    length: 3400,
  },
  stamp: {
    tool: "stamp",
    beats: ([[0.35, 0.5], [0.5, 0.62], [0.65, 0.5]] as Part[]).flatMap((at, number) => [
      { after: number * 700, hand: { at } },
      { after: number * 700 + 250, hand: { at, held: "left" as const }, act: (editor: EditorShow) => void editor.sketch(null, { tool: "stamp", at, number: number + 1 }) },
      { after: number * 700 + 400, hand: { at } },
    ]),
    length: 3600,
  },
  hide: {
    tool: "mosaic",
    beats: [
      { after: 0, hand: { at: [0.34, 0.45] } },
      ...dragged([0.34, 0.45], [0.66, 0.72], 400, 1000, (at, editor, drawn) => drawn0(editor, drawn, { tool: "mosaic", from: [0.34, 0.45], to: at })),
      { after: 1500, hand: { at: [0.66, 0.72] } },
    ],
    length: 3200,
  },
  crop: {
    tool: "crop",
    beats: [
      { after: 0, hand: { at: [0.02, 0.02] } },
      ...dragged([0.02, 0.02], [0.25, 0.22], 400, 900, (at, editor) => editor.frame(at, [1, 1])),
      { after: 1400, hand: { at: [0.25, 0.22] } },
    ],
    length: 3200,
  },
  select: {
    tool: "select",
    beats: [
      // What there is to select: drawn at once.
      { after: 0, hand: { at: [0.5, 0.75] }, act: (editor, drawn) => drawn0(editor, drawn, { tool: "rectangle", from: [0.3, 0.45], to: [0.5, 0.7] }) },
      { after: 300, hand: { at: [0.5, 0.6] } },
      // Pressed on: selected, and dragged along.
      { after: 800, hand: { at: [0.5, 0.6], held: "left" }, act: (editor, drawn) => editor.select(drawn[0]) },
      ...dragged([0.5, 0.6], [0.62, 0.52], 1000, 700, (at, editor, drawn) => {
        const [x, y] = [at[0] - 0.5, at[1] - 0.6];
        drawn0(editor, drawn, { tool: "rectangle", from: [0.3 + x, 0.45 + y], to: [0.5 + x, 0.7 + y] });
      }),
      { after: 1800, hand: { at: [0.62, 0.52] } },
      // The other button pressed twice on it: its settings, there.
      { after: 2300, hand: { at: [0.62, 0.52], held: "twice" } },
      { after: 3200, hand: { at: [0.62, 0.52] }, act: (editor) => editor.settingsAt(...pageOf([0.62, 0.52])) },
      // (Then out of the way of the settings.)
      { after: 3400, hand: { at: [0.55, 0.85] } },
    ],
    length: 6200,
  },
};

/** A place on the picture as the tour names it, in pixels of the page. */
function pageOf([across, down]: Part): [number, number] {
  const sheet = document.querySelector(".picture.stage .sheet")?.getBoundingClientRect();
  return sheet ? [Math.round(sheet.left + sheet.width * across), Math.round(sheet.top + sheet.height * down)] : [0, 0];
}

/** A tool used on the picture, as `play` has it: the tool in hand, the hand drawn where it is, and
    what it draws drawn; and again, from what the picture was. Gone, with what it drew, once the
    step is over. */
function Sketching({ show, play }: { show: Shown; play: Play }) {
  const [hand, setHand] = useState<{ at: { x: number; y: number }; held?: Held; round: number } | null>(null);
  const [ready, setReady] = useState<{ x: number; y: number } | null>(null);
  useLayoutEffect(() => {
    const [x, y] = pageOf([0.5, 0.5]);
    if (x || y) setReady({ x, y });
  }, []);
  useShowing(show, ready, (_, editor, acting) => {
    const was = editor.now();
    let timers: number[] = [];
    let round = 0;
    const once = () => {
      round += 1;
      const drawn: number[] = [];
      if (acting) {
        if (round > 1) editor.restore(was);
        editor.take(play.tool);
        // (All of the picture in sight: the places the play names are of all of it.)
        editor.whole();
      }
      // (With less motion asked for, all of it at once, and left so: each a moment after the one
      // before, so that each is drawn before the next.)
      timers = play.beats.map(({ after, hand, act }, index) =>
        window.setTimeout(() => {
          if (hand) {
            const [x, y] = pageOf(hand.at);
            setHand({ at: { x, y }, held: hand.held, round });
          }
          if (acting) act?.(editor, drawn);
        }, still() ? index * 30 : after),
      );
      if (!still()) timers.push(window.setTimeout(once, play.length));
    };
    once();
    return () => timers.forEach((one) => window.clearTimeout(one));
  });
  // (A new round starts where it starts, not from where the last one ended; and the presses twice
  // are shown anew each time.)
  return hand && <Hand key={hand.held === "twice" ? `twice ${hand.round}` : hand.round} at={hand.at} held={hand.held} moving />;
}

/** The choice of what the capture leaves as, open beside one of the things to do. */
function Choosing({ choose, way }: { choose: Choose; way: Way }) {
  useEffect(() => {
    choose(way);
    return () => choose(null);
  }, []);
  return null;
}
