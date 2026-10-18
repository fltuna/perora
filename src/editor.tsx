// Drawing on the capture in the large preview window: a bar of tools above the picture, and the
// picture with what was drawn over it. The bar is laid out the way the Snipping Tool of Windows lays
// out its own: a tool in hand, pressed again, opens a panel of its colours and its size; the shapes
// have a second bar under the first, with what to draw and how its inside and its outline look. So
// have the two ways of making a part unreadable, and cutting the picture down.
//
// What is drawn is a list of edits, each in the pixels of the image, whatever size the picture is
// shown at. The list is drawn here on a canvas, over the picture drawn on it first; and when the
// capture is to leave, the same list is drawn by the same means on a canvas as large as the image,
// and what that canvas holds is the image that leaves. The application draws nothing: it is told
// the list (to give it back when asked anew) and handed the image.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { type Capture, type Edit, type Leaving, type Place, captureFile, drawn, fontFamilies, imageUrl, tellEdits, type translator } from "./backend";
import { touring } from "./tour";

type Translate = ReturnType<typeof translator>;

/** A place on the picture as the tour names it: so far across it and down it, from 0 to 1. */
export type Part = [number, number];

/** Something the tour draws on the picture, with the settings of its tool as they are. */
export type Sketch =
  | { tool: "pen"; points: Part[] }
  | { tool: "marker"; points: Part[] }
  | { tool: "rectangle"; from: Part; to: Part }
  | { tool: "mosaic"; from: Part; to: Part }
  | { tool: "text"; at: Part; text: string }
  | { tool: "stamp"; at: Part; number: number };

/** What the tour of the window has the window do, as the mouse would: for it to be seen. */
export interface EditorShow {
  /** The tool in hand, how closely the picture is looked at and what is drawn: what `restore` goes
      back to. And whether something is under way that taking another tool would end (something
      selected, being written, a frame being set to cut the picture to, a press): then nothing is
      done here. */
  now: () => {
    tool: Tool;
    looking: { times: number; x?: number; y?: number };
    drawing: Drawing;
    selected: number | null;
    several: number[];
    figure: Tool;
    hider: "blur" | "mosaic";
    shut: boolean;
    justDrawn: boolean;
    busy: boolean;
  };
  /** Something drawn for the tour: on top of the rest (`index` none), or in the place of what the
      tour drew before at `index`. Its index, or -1 when it could not be drawn. The application is
      not told of it, and it is no step of the drawing to be undone: it is gone with `restore`. */
  sketch: (index: number | null, what: Sketch) => number;
  /** One thing drawn selected, or nothing. */
  select: (index: number | null) => void;
  /** The frame the picture is to be cut to, while cutting it down: between two places. */
  frame: (from: Part, to: Part) => void;
  /** All of the picture in sight, as it is when nothing is looked at more closely: where the tour
      names places on it. */
  whole: () => void;
  /** A place on the picture as the tour names it, in pixels of the page: where it is with all of
      the picture in sight (as `whole` has it, at once: before the window is drawn so). [0, 0] while
      there is no picture shown. */
  pageOf: (part: Part) => [number, number];
  /** The wheel turned a notch with the other button held, the pointer at (x, y) of the page: the
      next tool (`by` 1) or the one before (-1) in hand, and shown by the pointer. */
  turn: (x: number, y: number, by: 1 | -1) => void;
  /** The tool taken in hand, as by its key. */
  take: (one: Tool) => void;
  /** The settings of the tool in hand, where (x, y) of the page is: as the other button pressed
      twice there. */
  settingsAt: (x: number, y: number) => void;
  /** Those settings put away again. */
  shut: () => void;
  /** The wheel turned at (x, y) of the page: the picture looked at more closely, or less. */
  zoom: (x: number, y: number, deltaY: number) => void;
  /** All of that undone: the tool and the look as they were, nothing shown by the pointer, nothing
      open. */
  restore: (was: ReturnType<EditorShow["now"]>) => void;
}

export type Tool =
  | "select"
  | "pen"
  | "marker"
  | "line"
  | "arrow"
  | "rectangle"
  | "ellipse"
  | "text"
  | "stamp"
  | "blur"
  | "mosaic"
  | "crop";

/** An edit that is drawn with a colour and a width; one that makes a part unreadable; one that has
    letters in it. */
type Drawn = Exclude<Edit, { kind: "blur" | "mosaic" | "crop" | "text" | "stamp" }>;
type Hidden = Extract<Edit, { kind: "blur" | "mosaic" }>;
type Written = Extract<Edit, { kind: "text" }>;
type Stamped = Extract<Edit, { kind: "stamp" }>;
const isHidden = (edit: Edit): edit is Hidden => edit.kind === "blur" || edit.kind === "mosaic";
const isLettered = (edit: Edit): edit is Written | Stamped => edit.kind === "text" || edit.kind === "stamp";
const isDrawn = (edit: Edit): edit is Drawn => !isHidden(edit) && !isLettered(edit) && edit.kind !== "crop";

const TOOLS: { tool: Tool; key: string }[] = [
  { tool: "select", key: "V" },
  { tool: "pen", key: "P" },
  { tool: "marker", key: "M" },
  { tool: "line", key: "L" },
  { tool: "arrow", key: "A" },
  { tool: "rectangle", key: "R" },
  { tool: "ellipse", key: "E" },
  { tool: "text", key: "T" },
  { tool: "stamp", key: "N" },
  { tool: "blur", key: "B" },
  { tool: "mosaic", key: "X" },
  { tool: "crop", key: "C" },
];

/** What the second bar offers to draw: the shapes, and the lines that go with them. */
const FIGURES: Tool[] = ["rectangle", "ellipse", "line", "arrow"];
const isFigure = (tool: string) => FIGURES.includes(tool as Tool);

/** The two ways of making a part unreadable, which share a button of the first bar. */
const HIDERS: Tool[] = ["blur", "mosaic"];
const isHider = (tool: string) => HIDERS.includes(tool as Tool);

/** How strongly a part is made unreadable, in steps, and the step each of the two ways starts with;
    and what a step is in pixels of the image, for each of them (times `unit`, as with widths), up to
    what the application takes. */
const STRENGTHS = { least: 1, most: 10, blur: 3, mosaic: 5 };
// The weakest of each is a light one, which softens more than it hides: how much is the user's to
// choose, and a weak one has to be there to be chosen. (It is also the least the application does,
// whatever it is asked.)
// (`least` and `most` are those of the first and the last step, in pixels on an image a thousand
// wide; the steps between are evenly apart.)
const BLUR = { least: 1, most: 7 };
const MOSAIC = { least: 2, most: 10 };
/** What a step of strength is, for one of the two ways. */
const perStep = (way: { least: number; most: number }) => (way.most - way.least) / (STRENGTHS.most - STRENGTHS.least);

/** How strongly each of the two ways was last used: kept from one capture to the next, and from one
    run to the next. Where nothing can be kept, or what is kept is no strength, the one to start
    with. */
const KEPT = "perora.editor.strengths";
function keptStrengths() {
  const first = { blur: STRENGTHS.blur, mosaic: STRENGTHS.mosaic };
  try {
    const kept: unknown = JSON.parse(localStorage.getItem(KEPT) ?? "{}");
    const step = (value: unknown, otherwise: number) =>
      Number.isInteger(value) && (value as number) >= STRENGTHS.least && (value as number) <= STRENGTHS.most
        ? (value as number)
        : otherwise;
    const { blur, mosaic } = (kept ?? {}) as { blur?: unknown; mosaic?: unknown };
    return { blur: step(blur, first.blur), mosaic: step(mosaic, first.mosaic) };
  } catch {
    return first;
  }
}
function keepStrengths(strengths: { blur: number; mosaic: number }) {
  try {
    localStorage.setItem(KEPT, JSON.stringify(strengths));
  } catch {
    // Not kept, then: they are what they are until the window is gone.
  }
}

/** Which of the two ways was in hand last: kept from one capture to the next, and from one run to the
    next, for the editor to start with it when the user has it so (owner, 2026-10-19). Blurring,
    where nothing is kept. */
const KEPT_HIDER = "perora.editor.hider";
function firstHider(chosen: "last" | "mosaic" | "blur"): "blur" | "mosaic" {
  if (chosen !== "last") return chosen;
  try {
    return localStorage.getItem(KEPT_HIDER) === "mosaic" ? "mosaic" : "blur";
  } catch {
    return "blur";
  }
}
function keepHider(hider: "blur" | "mosaic") {
  try {
    localStorage.setItem(KEPT_HIDER, hider);
  } catch {
    // Not kept, then: the next capture starts as the first one did.
  }
}

/** The shapes the image can be cut down to: as wide to as tall as this, or any (`null`). */
const RATIOS: { name: string; ratio: number | null }[] = [
  { name: "free", ratio: null },
  { name: "1:1", ratio: 1 },
  { name: "4:3", ratio: 4 / 3 },
  { name: "3:2", ratio: 3 / 2 },
  { name: "16:9", ratio: 16 / 9 },
  { name: "3:4", ratio: 3 / 4 },
  { name: "2:3", ratio: 2 / 3 },
  { name: "9:16", ratio: 9 / 16 },
];

/** The colours offered, six to a row: greys, strong colours, and pale ones. */
const PALETTE = [
  ["#000000", "#ffffff", "#d1d3d4", "#a7a9ac", "#808285", "#58595b"],
  ["#b31564", "#e61b1b", "#ff5500", "#ffaa00", "#ffce00", "#ffe600"],
  ["#a2e61b", "#26e600", "#008055", "#00aacc", "#004de6", "#3d00b8"],
  ["#6600cc", "#600080", "#f7d7c4", "#bb9167", "#8e562e", "#613d30"],
  ["#ff80ff", "#ffc680", "#ffff80", "#80ff9e", "#80d6ff", "#bcb3ff"],
].flat();

/** The colours of a marker. */
const MARKERS = ["#ffe600", "#26e600", "#44c8f5", "#ec008c", "#ff5500", "#6600cc"];

/** The colour things are drawn in until another is chosen. */
const RED = "#e61b1b";

// As the application draws them: how much shows through a marker, how much wider it is than a pen,
// and the head of an arrow in widths of its line (`perora_core::edit`).
const MARKER = { opacity: 0.4, width: 4 };
const ARROW = { length: 4.5, halfWidth: 2.2, smallest: 10 };

/** The widths offered, in steps; a step is more pixels on a larger image (see `unit`). */
const WIDTHS = { least: 1, most: 20, first: 4 };

/** What is written: the size of its letters to start with, in steps (a step is so many pixels, times
    `unit`), the room a line takes in sizes of its letters, the room around all of it, and what it is
    written in. Its size is set in pixels of the image, within `SIZES`. */
const LETTERS = { least: 1, most: 20, first: 4, step: 6, line: 1.3, around: 0.2 };
const SIZES = { least: 4, most: 2000 };
const FONT = '"Segoe UI Variable Text", "Segoe UI", "Yu Gothic UI", "Meiryo", sans-serif';
/** The name of a font's family as it can stand in a style: nothing in it that ends the name there. */
const family = (font: string) =>
  Array.from(font.replace(/["'\\;{}<>]/g, ""))
    .slice(0, 64)
    .join("")
    .trim();
/** The fonts something is written in: the one chosen, when one is, and then those Perora writes in
    (a font that the system does not have, or that lacks a letter, gives way to them). */
const fontsOf = (font = "") => (family(font) ? `"${family(font)}", ${FONT}` : FONT);
const fontOf = (size: number, weight: number, font = "", italic = false) =>
  `${italic ? "italic " : ""}${weight} ${size}px ${fontsOf(font)}`;
/** How heavy letters are: bold, and not. (Two weights that nearly every font has, or that are made
    of the one it has: between others, most fonts show no difference.) */
const WEIGHTS = { plain: 400, bold: 700 };
const weightOf = (how: { bold: boolean }) => (how.bold ? WEIGHTS.bold : WEIGHTS.plain);
/** The line through letters that are struck through: how thick it is, and how far under the middle
    of their line, in sizes of the letters. */
const STRUCK = { thick: 1 / 14, under: 0.05 };
/** Families to choose from until the system has said which it has (and should it never say). */
const FONTS = [
  "Segoe UI",
  "Yu Gothic UI",
  "Meiryo",
  "BIZ UDPGothic",
  "BIZ UDPMincho",
  "Yu Mincho",
  "MS Gothic",
  "Consolas",
  "Cascadia Code",
  "Arial",
  "Times New Roman",
  "Georgia",
  "Impact",
  "Comic Sans MS",
];

/** The font last written in: kept from one capture to the next, and from one run to the next. */
const KEPT_FONT = "perora.editor.font";
function keptFont() {
  try {
    return family(localStorage.getItem(KEPT_FONT) ?? "");
  } catch {
    return "";
  }
}
function keepFont(font: string) {
  try {
    localStorage.setItem(KEPT_FONT, family(font));
  } catch {
    // (Nowhere to keep it: the next capture starts with the font Perora writes in.)
  }
}

/** A number in a disc: how far across the disc is, in steps as above. */
const STAMPS = { least: 1, most: 20, first: 4, step: 8 };

/** The closest the picture is looked at: this many pixels of the screen to one of the image. */
const CLOSEST = 8;

/** The furthest it is looked at from: this part of the size it has fitted to its place. Further than
    fitted, there is room around it, to begin drawing from (owner, 2026-10-14). */
const FARTHEST = 0.25;

/** How far from a line a press still means that line, in pixels of the screen. */
const NEAR = 6;

/** A drawing is no longer than this: so many things drawn, so many places in all, no line wider
    than this many pixels of the image. As the application has it (`perora_core::edit`): what is
    shown here is never more than what it takes. */
const MOST = {
  edits: 2000,
  points: 200_000,
  width: 400,
  hidden: 64,
  // What is written: no more letters in all than this, and nothing of it larger than this (a side,
  // and its pixels in all).
  written: 20_000,
  side: 8192,
  pixels: 8_000_000,
};

/** How many things a drawing can be made of: for whoever tells the user how far along one is. */
export const MOST_EDITS = MOST.edits;

/** Changes of the same setting this close together are one step to undo: a slider being dragged. */
const ONE_STEP = 800;

const held = (width: number) => Math.min(Math.max(width, 1), MOST.width);

const fits = (edits: Edit[]) =>
  edits.length <= MOST.edits &&
  edits.reduce((count, edit) => count + ("points" in edit ? edit.points.length : 2), 0) <= MOST.points &&
  edits.filter(isHidden).length <= MOST.hidden &&
  edits.reduce((length, edit) => length + (edit.kind === "text" ? edit.text.length : 0), 0) <= MOST.written;

/** A step of width in pixels of the image: one on an image a monitor wide, more on a larger one, so
    that a line looks about as wide on screen whatever was captured. */
const unit = (capture: Capture) => Math.max(1, Math.round(Math.max(capture.width, capture.height) / 1000));

const tenth = (value: number) => Math.round(value * 10) / 10;

// ---------------------------------------------------------------------------------------------
// Drawing

function arrowHead(from: Place, to: Place, width: number) {
  const [across, down] = [to[0] - from[0], to[1] - from[1]];
  const length = Math.hypot(across, down);
  if (length < 0.5) return null;
  const along: Place = [across / length, down / length];
  const head = Math.min(Math.max(width * ARROW.length, ARROW.smallest), length);
  const half = (head * ARROW.halfWidth) / ARROW.length;
  const base: Place = [to[0] - along[0] * head, to[1] - along[1] * head];
  return {
    base,
    shaft: length > head,
    left: [base[0] - along[1] * half, base[1] + along[0] * half] as Place,
    right: [base[0] + along[1] * half, base[1] - along[0] * half] as Place,
  };
}

function strokeThrough(ctx: CanvasRenderingContext2D, points: Place[], width: number) {
  const [first, ...rest] = points;
  if (!first) return;
  if (rest.every((place) => place[0] === first[0] && place[1] === first[1])) {
    ctx.beginPath();
    ctx.arc(first[0], first[1], width / 2, 0, Math.PI * 2);
    ctx.fill();
    return;
  }
  ctx.lineWidth = width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(first[0], first[1]);
  for (const place of rest) ctx.lineTo(place[0], place[1]);
  ctx.stroke();
}

function paint(ctx: CanvasRenderingContext2D, edit: Drawn) {
  ctx.save();
  ctx.strokeStyle = edit.color;
  ctx.fillStyle = edit.color;
  switch (edit.kind) {
    case "pen":
      strokeThrough(ctx, edit.points, held(edit.width));
      break;
    case "marker":
      ctx.globalAlpha = MARKER.opacity;
      strokeThrough(ctx, edit.points, held(edit.width * MARKER.width));
      break;
    case "line":
      strokeThrough(ctx, [edit.from, edit.to], held(edit.width));
      break;
    case "arrow": {
      const width = held(edit.width);
      const head = arrowHead(edit.from, edit.to, width);
      if (!head) {
        strokeThrough(ctx, [edit.from], width);
        break;
      }
      if (head.shaft) strokeThrough(ctx, [edit.from, head.base], width);
      ctx.beginPath();
      ctx.moveTo(edit.to[0], edit.to[1]);
      ctx.lineTo(head.left[0], head.left[1]);
      ctx.lineTo(head.right[0], head.right[1]);
      ctx.closePath();
      ctx.fill();
      break;
    }
    case "rectangle":
    case "ellipse": {
      const box = boxOf(edit);
      if (box.width <= 0 || box.height <= 0) break;
      ctx.beginPath();
      if (edit.kind === "rectangle") ctx.rect(box.x, box.y, box.width, box.height);
      else {
        ctx.ellipse(box.x + box.width / 2, box.y + box.height / 2, box.width / 2, box.height / 2, 0, 0, Math.PI * 2);
      }
      if (edit.fill > 0) {
        ctx.globalAlpha = edit.fill;
        ctx.fillStyle = edit.fillColor ?? edit.color;
        ctx.fill();
        ctx.globalAlpha = 1;
      }
      ctx.lineWidth = held(edit.width);
      ctx.lineJoin = edit.kind === "rectangle" ? "miter" : "round";
      ctx.stroke();
      break;
    }
  }
  ctx.restore();
}

// Letters.

let measuring: CanvasRenderingContext2D | null = null;

/** How something is written: the colour of the inside of its letters and how solid that is (0 for
    none), the size of the letters in pixels of the image, the font (none for Perora's own), and the
    line around the letters, when they have one: its colour, and how far it reaches out of them.
    And whether the letters are bold, slanted, struck through. */
interface Writing {
  color: string;
  size: number;
  font: string;
  fill: number;
  outline: string | null;
  edge: number;
  bold: boolean;
  italic: boolean;
  strike: boolean;
}
const howOf = (edit: Written): Writing => {
  // (Within bounds, whatever the edit says; and never without both an inside and a line: letters
  // that had neither would be there and not be seen.)
  const within = (value: number, most: number) => (Number.isFinite(value) ? Math.max(0, Math.min(most, value)) : 0);
  const edge = edit.outline ? within(edit.outlineWidth ?? 0, MOST.width) : 0;
  const outline = edge > 0 ? (edit.outline ?? null) : null;
  const fill = within(edit.fill ?? 1, 1);
  return {
    color: edit.color,
    size: edit.size,
    font: edit.font ?? "",
    fill: fill === 0 && !outline ? 1 : fill,
    outline,
    edge,
    bold: edit.bold === true,
    italic: edit.italic === true,
    strike: edit.strike === true,
  };
};
/** How far the line around letters of a size reaches out of them to start with: a twelfth of
    their size, within what its slider offers. */
const firstEdge = (size: number) => Math.max(1, Math.min(EDGES.most, Math.round(size / 12)));
const EDGES = { least: 1, most: 40 };
/** A sheet to draw the line around letters on, apart from the picture: one, used again. */
let apart: OffscreenCanvas | null = null;
/** How far the line around the letters reaches out of them, in whole pixels: so much more room is
    around what is written. */
const reachOf = (how: { outline: string | null; edge: number }) => (how.outline ? Math.ceil(how.edge) : 0);
/** A colour, as solid as that: `#rrggbbaa`. */
const faded = (colour: string, solid: number) =>
  `${colour}${Math.round(Math.max(0, Math.min(1, solid)) * 255).toString(16).padStart(2, "0")}`;

/** How what is written is laid out, in pixels of the image: its lines, the room each takes, the room
    around all of it (that of the line around its letters too), how wide each line is, and how
    large that makes it. */
function laidOut(text: string, how: Writing) {
  measuring ??= document.createElement("canvas").getContext("2d");
  const lines = text.split("\n");
  const [line, around] = [Math.round(how.size * LETTERS.line), Math.ceil(how.size * LETTERS.around) + reachOf(how)];
  let widths = lines.map(() => 0);
  if (measuring) {
    const measure = measuring;
    measure.font = fontOf(how.size, weightOf(how), how.font, how.italic);
    widths = lines.map((one) => measure.measureText(one).width);
  }
  const widest = widths.reduce((most, one) => Math.max(most, one), 0);
  return { lines, widths, line, around, width: Math.ceil(widest) + around * 2, height: lines.length * line + around * 2 };
}

/** Writes at a place: `at` is the top-left corner of the room around what is written. The line
    around the letters first, and the inside of the letters over it. Letters that are struck
    through have a line through them that is part of their shape: with their inside, and with the
    line around them around it too.

    The line around is drawn on the edge of the letters, as wide again inwards as it reaches
    outwards. Under letters that are solid, its inner half is not seen. Under letters that are not,
    it would be seen through them, where what is behind the letters is to be seen: so there the
    line is drawn apart, the letters are taken out of it, and what is left (what is outside them)
    is laid on. The inside of letters that are struck through and not solid is drawn apart as well:
    drawn here, it would be twice as solid where the line goes through a letter. */
function write(ctx: CanvasRenderingContext2D, at: Place, text: string, how: Writing) {
  const { lines, widths, line, around, width, height } = laidOut(text, how);
  type Pen = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
  const set = (pen: Pen) => {
    pen.font = fontOf(how.size, weightOf(how), how.font, how.italic);
    pen.textBaseline = "middle";
    pen.textAlign = "left";
  };
  // Each line of it, and the line through each when it is struck through.
  const thick = Math.max(1, how.size * STRUCK.thick);
  const each = (letters: (one: string, x: number, y: number) => void, through: (x: number, y: number, width: number, height: number) => void) =>
    lines.forEach((one, index) => {
      const [x, y] = [at[0] + around, at[1] + around + line * (index + 0.5)];
      letters(one, x, y);
      if (how.strike && widths[index] > 0) through(x, y + how.size * STRUCK.under - thick / 2, widths[index], thick);
    });
  const edge = (pen: Pen) => {
    if (!how.outline) return;
    pen.strokeStyle = how.outline;
    pen.lineWidth = how.edge * 2;
    pen.lineJoin = "round";
    each(
      (one, x, y) => pen.strokeText(one, x, y),
      (x, y, width, height) => pen.strokeRect(x, y, width, height),
    );
  };
  const inside = (pen: Pen, colour: string) => {
    pen.fillStyle = colour;
    each(
      (one, x, y) => pen.fillText(one, x, y),
      (x, y, width, height) => pen.fillRect(x, y, width, height),
    );
  };
  // Drawn apart exactly as it would be drawn here (the same pixels, so that what is drawn apart
  // meets what is drawn here to the pixel), and only as much of it as is on this drawing: looked
  // at closely, what is written is many times larger than what is seen of it. Then laid on, as
  // solid as that.
  const apartly = (draw: (pen: OffscreenCanvasRenderingContext2D) => void, solid = 1) => {
    const here = ctx.getTransform();
    const [x, y] = [here.a * at[0] + here.e, here.d * at[1] + here.f];
    const [left, top] = [Math.max(0, Math.floor(Math.min(x, x + here.a * width))), Math.max(0, Math.floor(Math.min(y, y + here.d * height)))];
    const right = Math.min(ctx.canvas.width, Math.ceil(Math.max(x, x + here.a * width)));
    const bottom = Math.min(ctx.canvas.height, Math.ceil(Math.max(y, y + here.d * height)));
    if (!(right > left && bottom > top)) return;
    apart ??= new OffscreenCanvas(1, 1);
    // (Sized anew, it is empty again.)
    apart.width = right - left;
    apart.height = bottom - top;
    const pen = apart.getContext("2d");
    if (!pen) return;
    pen.setTransform(here.a, here.b, here.c, here.d, here.e - left, here.f - top);
    set(pen);
    draw(pen);
    ctx.save();
    try {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalAlpha *= solid;
      ctx.drawImage(apart, left, top);
    } finally {
      ctx.restore();
    }
  };
  ctx.save();
  try {
    set(ctx);
    if (how.outline && how.edge > 0) {
      if (how.fill >= 1) edge(ctx);
      else {
        apartly((pen) => {
          edge(pen);
          pen.globalCompositeOperation = "destination-out";
          inside(pen, "#000000");
          pen.globalCompositeOperation = "source-over";
        });
      }
    }
    if (how.fill > 0) {
      const solid = Math.min(1, how.fill);
      if (how.strike && solid < 1) apartly((pen) => inside(pen, how.color), solid);
      else {
        ctx.globalAlpha *= solid;
        inside(ctx, how.color);
      }
    }
  } finally {
    ctx.restore();
  }
}

/** Whether a colour is a light one: what is written on it is black then, white otherwise. */
function isLight(colour: string) {
  const [red, green, blue] = [1, 3, 5].map((at) => parseInt(colour.slice(at, at + 2), 16));
  return red * 0.299 + green * 0.587 + blue * 0.114 > 160;
}

/** A number in a disc that fills a box: the disc in the colour, the number and a thin ring inside
    its edge in black or white, whichever shows on it (the ring sets it off from a ground of its own
    colour). */
function stamp(ctx: CanvasRenderingContext2D, box: Box, number: number, colour: string) {
  const across = Math.min(box.width, box.height);
  if (across <= 0) return;
  const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
  const on = isLight(colour) ? "#000000" : "#ffffff";
  const digits = String(number);
  ctx.save();
  ctx.fillStyle = colour;
  ctx.beginPath();
  ctx.arc(x, y, across / 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = on;
  ctx.lineWidth = across * 0.05;
  ctx.beginPath();
  ctx.arc(x, y, across * 0.43, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = on;
  ctx.font = fontOf(across * (digits.length <= 1 ? 0.56 : digits.length === 2 ? 0.46 : 0.34), 700);
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  // (A little lower than the middle of the line: digits have nothing below their foot.)
  ctx.fillText(digits, x, y + across * 0.035);
  ctx.restore();
}

/** Draws what has letters in it, as it is drawn over the picture. */
function letter(ctx: CanvasRenderingContext2D, edit: Written | Stamped) {
  if (edit.kind === "stamp") return stamp(ctx, boxOf(edit), edit.number, edit.color);
  // Written at its size where its place is as large as that makes it; and stretched to its place
  // while it is being pulled to another size.
  const box = boxOf(edit);
  const how = howOf(edit);
  const own = laidOut(edit.text, how);
  if (box.width <= 0 || box.height <= 0) return;
  ctx.save();
  ctx.translate(box.x, box.y);
  ctx.scale(box.width / own.width, box.height / own.height);
  // (Nothing of it outside its place: its picture has nothing there either.)
  ctx.beginPath();
  ctx.rect(0, 0, own.width, own.height);
  ctx.clip();
  write(ctx, [0, 0], edit.text, how);
  ctx.restore();
}

/** Something written with its top-left corner at a place, at whole pixels (where its letters are
    sharpest). Nothing when it would be larger than what is written gets. */
function written(at: Place, text: string, how: Writing): Written | null {
  const from: Place = [Math.round(at[0]), Math.round(at[1])];
  const { width, height } = laidOut(text, how);
  if (width > MOST.side || height > MOST.side || width * height > MOST.pixels) return null;
  // (Each thing is said only when it is not what goes without saying: Perora's own font, letters
  // that are solid, no line around them, letters that are not bold, upright, not struck through.)
  return {
    kind: "text",
    from,
    to: [from[0] + width, from[1] + height],
    text,
    color: how.color,
    size: how.size,
    ...(family(how.font) && { font: family(how.font) }),
    ...(how.fill !== 1 && { fill: how.fill }),
    ...(how.outline && how.edge > 0 && { outline: how.outline, outlineWidth: how.edge }),
    ...(how.bold && { bold: true }),
    ...(how.italic && { italic: true }),
    ...(how.strike && { strike: true }),
  };
}

/** A number in a disc around a place, so far across, at whole pixels. */
function stamped(middle: Place, across: number, number: number, colour: string): Stamped {
  const side = Math.min(MOST.side, Math.max(4, Math.round(across)));
  const from: Place = [Math.round(middle[0] - side / 2), Math.round(middle[1] - side / 2)];
  return { kind: "stamp", from, to: [from[0] + side, from[1] + side], number, color: colour };
}

/** A number in a disc as it is after being pulled to another size: square, at whole pixels. */
function squared(edit: Stamped): Stamped {
  const box = boxOf(edit);
  const side = Math.max(4, Math.round(Math.min(box.width, box.height)));
  const from: Place = [Math.round(box.x), Math.round(box.y)];
  return { ...edit, from, to: [from[0] + side, from[1] + side] };
}

/** A canvas to work on beside the one that is shown, and one to copy a part of it to as it is. */
let spare: HTMLCanvasElement | null = null;
let copied: HTMLCanvasElement | null = null;

/** Makes a part of the canvas unreadable, as it is drawn so far: blurred, or broken into blocks.
    `zoom` is how many pixels of the canvas one of the image is, `view` the part of the image the
    canvas shows (it may reach beyond the picture), and `picture` where the picture is, in the
    image. Only what is in the area, and of the picture, goes into it, as the application does it. */
function hide(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement, edit: Hidden, zoom: number, view: Box, picture: Box) {
  const box = boxOf(edit);
  const [from, to] = [Math.max(box.x, picture.x), Math.min(box.x + box.width, picture.x + picture.width)];
  const [above, below] = [Math.max(box.y, picture.y), Math.min(box.y + box.height, picture.y + picture.height)];
  const left = Math.max(0, Math.floor((from - view.x) * zoom));
  const top = Math.max(0, Math.floor((above - view.y) * zoom));
  const right = Math.min(canvas.width, Math.ceil((to - view.x) * zoom));
  const bottom = Math.min(canvas.height, Math.ceil((below - view.y) * zoom));
  const [width, height] = [right - left, bottom - top];
  if (width <= 0 || height <= 0) return;
  spare ??= document.createElement("canvas");
  const beside = spare.getContext("2d");
  if (!beside) return;

  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  if (edit.kind === "mosaic") {
    // Made as small as it has blocks, each pixel the blend of a block, and drawn large again with
    // its pixels left square.
    const cell = Math.max(1, edit.cell * zoom);
    const [columns, rows] = [Math.max(1, Math.ceil(width / cell)), Math.max(1, Math.ceil(height / cell))];
    // The area copied out as it is first, and made smaller from there: how the browser makes a part
    // of a canvas smaller depends on the canvas around it, and the window's canvas is not the one the
    // image that leaves is made on (it is larger, the picture elsewhere in it). From a canvas of its
    // own, the blocks are the same in both.
    copied ??= document.createElement("canvas");
    copied.width = width;
    copied.height = height;
    copied.getContext("2d")?.drawImage(canvas, left, top, width, height, 0, 0, width, height);
    spare.width = columns;
    spare.height = rows;
    beside.imageSmoothingQuality = "high";
    // First stretched over all of it, so that the blocks at the right and bottom edges, which are
    // only partly in the area, are not partly see-through; then in its true proportions.
    beside.drawImage(copied, 0, 0, width, height, 0, 0, columns, rows);
    beside.drawImage(copied, 0, 0, width, height, 0, 0, width / cell, height / cell);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(spare, 0, 0, columns, rows, left, top, columns * cell, rows * cell);
  } else {
    // Copied out with its edges repeated around it (so that the blur has something to take from
    // beyond them that is not from beside the area), and drawn back blurred.
    const spread = Math.max(0.5, edit.radius * zoom);
    const around = Math.ceil(spread * 3);
    spare.width = width + around * 2;
    spare.height = height + around * 2;
    beside.drawImage(canvas, left, top, width, height, around, around, width, height);
    beside.drawImage(canvas, left, top, width, 1, around, 0, width, around);
    beside.drawImage(canvas, left, bottom - 1, width, 1, around, around + height, width, around);
    beside.drawImage(spare, around, 0, 1, spare.height, 0, 0, around, spare.height);
    beside.drawImage(spare, around + width - 1, 0, 1, spare.height, around + width, 0, around, spare.height);
    ctx.filter = `blur(${spread}px)`;
    ctx.drawImage(spare, left - around, top - around);
  }
  ctx.restore();
}

// ---------------------------------------------------------------------------------------------
// Where an edit is

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

const placesOf = (edit: Edit): Place[] =>
  "points" in edit ? edit.points : [edit.from, edit.to];

function boxOf(edit: Edit): Box {
  const places = placesOf(edit);
  const xs = places.map((place) => place[0]);
  const ys = places.map((place) => place[1]);
  const [left, top] = [Math.min(...xs), Math.min(...ys)];
  return { x: left, y: top, width: Math.max(...xs) - left, height: Math.max(...ys) - top };
}

/** How wide the edit is drawn; nothing, for what is not drawn with a line. */
const reach = (edit: Edit) =>
  isDrawn(edit) ? held(edit.kind === "marker" ? edit.width * MARKER.width : edit.width) : 0;

/** The part of the image the last crop among the edits leaves, in whole pixels; nothing without one
    (or with one that leaves nothing). */
function keptOf(edits: Edit[], whole: Box): Box | null {
  const crop = edits.filter((edit) => edit.kind === "crop").pop();
  if (!crop) return null;
  const box = boxOf(crop);
  const left = Math.max(0, Math.floor(box.x));
  const top = Math.max(0, Math.floor(box.y));
  const right = Math.min(whole.width, Math.ceil(box.x + box.width));
  const bottom = Math.min(whole.height, Math.ceil(box.y + box.height));
  return right > left && bottom > top ? { x: left, y: top, width: right - left, height: bottom - top } : null;
}

/** The largest frame of a shape (as wide to as tall as `ratio`) inside a box, in its middle. */
function fitted(box: Box, ratio: number): Box {
  const width = Math.max(1, Math.round(Math.min(box.width, box.height * ratio)));
  const height = Math.max(1, Math.min(box.height, Math.round(width / ratio)));
  return {
    x: box.x + Math.round((box.width - width) / 2),
    y: box.y + Math.round((box.height - height) / 2),
    width,
    height,
  };
}

function distanceToSegment(at: Place, a: Place, b: Place) {
  const [across, down] = [b[0] - a[0], b[1] - a[1]];
  const length = across * across + down * down;
  const along = length === 0 ? 0 : Math.min(1, Math.max(0, ((at[0] - a[0]) * across + (at[1] - a[1]) * down) / length));
  return Math.hypot(at[0] - (a[0] + along * across), at[1] - (a[1] + along * down));
}

/** Whether a press at `at` means this edit; `near` is how far off still counts, in image pixels. */
function isAt(edit: Edit, at: Place, near: number) {
  // What the image is cut down to is not a thing in it to take hold of.
  if (edit.kind === "crop") return false;
  const slack = reach(edit) / 2 + near;
  if (edit.kind === "rectangle" || edit.kind === "ellipse" || isHidden(edit) || isLettered(edit)) {
    // The whole of its box. But a shape with no inside is its outline alone: a press inside it is
    // for what is seen there, behind it.
    const box = boxOf(edit);
    const within =
      at[0] >= box.x - slack &&
      at[0] <= box.x + box.width + slack &&
      at[1] >= box.y - slack &&
      at[1] <= box.y + box.height + slack;
    if (!within || !(edit.kind === "rectangle" || edit.kind === "ellipse") || edit.fill > 0) return within;
    // (Not a small one, which is little more than its outline: that is taken hold of whole.)
    if (near <= 0 || Math.max(box.width, box.height) < near * 4) return true;
    if (edit.kind === "rectangle") {
      return !(
        at[0] > box.x + slack &&
        at[0] < box.x + box.width - slack &&
        at[1] > box.y + slack &&
        at[1] < box.y + box.height - slack
      );
    }
    // (Of an ellipse: between the one that much larger and the one that much smaller.)
    const [across, down] = [box.width / 2, box.height / 2];
    const [x, y] = [at[0] - (box.x + across), at[1] - (box.y + down)];
    const inside = (by: number) => across + by > 0 && down + by > 0 && (x / (across + by)) ** 2 + (y / (down + by)) ** 2 <= 1;
    return inside(slack) && !inside(-slack);
  }
  const places = placesOf(edit);
  if (places.length === 1) return Math.hypot(at[0] - places[0][0], at[1] - places[0][1]) <= slack;
  return places.slice(1).some((place, index) => distanceToSegment(at, places[index], place) <= slack);
}

function moved(edit: Edit, by: Place): Edit {
  // (What has a picture stays at whole pixels: the picture is laid in pixel for pixel.)
  const whole = isLettered(edit);
  const shift = (place: Place): Place =>
    whole ? [place[0] + Math.round(by[0]), place[1] + Math.round(by[1])] : [tenth(place[0] + by[0]), tenth(place[1] + by[1])];
  return "points" in edit
    ? { ...edit, points: edit.points.map(shift) }
    : { ...edit, from: shift(edit.from), to: shift(edit.to) };
}

/** Where something selected can be taken hold of to change its size: an end of a line; a corner or
    a side of the frame around a shape or a part made unreadable; a corner of the frame around
    something written or a number. (-1 the left or top one, 1 the right or bottom one.) */
type Hold = "from" | "to" | { x: -1 | 0 | 1; y: -1 | 0 | 1 };

/** How far around an edit the frame that shows it selected is, `scale` pixels of the screen to one
    of the image. */
const frameAround = (edit: Edit, scale: number) => reach(edit) / 2 + 4 / scale;

/** The places an edit can be taken hold of to change its size, in pixels of the image. A stroke of
    the pen or the marker has none: it is moved, not made larger. */
function holdsOf(edit: Edit, scale: number): { hold: Hold; at: Place }[] {
  if (edit.kind === "line" || edit.kind === "arrow") {
    return [
      { hold: "from", at: edit.from },
      { hold: "to", at: edit.to },
    ];
  }
  if ("points" in edit || edit.kind === "crop") return [];
  const box = boxOf(edit);
  const margin = frameAround(edit, scale);
  const holds: { hold: Hold; at: Place }[] = [];
  for (const x of [-1, 0, 1] as const) {
    for (const y of [-1, 0, 1] as const) {
      if (x === 0 && y === 0) continue;
      // (What has letters keeps its shape: only its corners are pulled.)
      if (isLettered(edit) && (x === 0 || y === 0)) continue;
      holds.push({
        hold: { x, y },
        at: [box.x + ((x + 1) / 2) * box.width + x * margin, box.y + ((y + 1) / 2) * box.height + y * margin],
      });
    }
  }
  return holds;
}

/** An edit after one of its holds was pulled along by `by`. With Shift held, a line keeps
    to one of eight directions and a shape keeps its proportions. What has letters is made larger or
    smaller as a whole, and is not yet given a new picture (`letteredAnew` does that, at the end). */
function resized(edit: Edit, hold: Hold, by: Place, shift: boolean, unit: number): Edit {
  if (hold === "from" || hold === "to") {
    if (edit.kind !== "line" && edit.kind !== "arrow") return edit;
    const other = hold === "from" ? edit.to : edit.from;
    // (As far as the pointer went, from where the end was: not to the pointer, which took hold of
    // it a little beside it.)
    const pulled: Place = [edit[hold][0] + by[0], edit[hold][1] + by[1]];
    const end = shift ? straightened("line", other, pulled) : pulled;
    return { ...edit, [hold]: [tenth(end[0]), tenth(end[1])] };
  }
  if ("points" in edit || edit.kind === "crop") return edit;
  const box = boxOf(edit);
  if (isLettered(edit)) {
    // From the corner opposite, which stays: as many times larger as the pointer went, the further
    // of across and down; no smaller than can be read, no larger than a picture gets.
    const across = box.width + hold.x * by[0];
    const down = box.height + hold.y * by[1];
    const least = edit.kind === "text" ? (LETTERS.step * unit) / edit.size : (STAMPS.step * unit) / box.width;
    // (A little under what a picture gets: the picture made of it at the end is laid out anew, and
    // may come out a pixel or two larger than this.)
    const most = 0.97 * Math.min(MOST.side / box.width, MOST.side / box.height, Math.sqrt(MOST.pixels / (box.width * box.height)));
    const times = Math.min(most, Math.max(Math.min(least, 1), across / box.width, down / box.height));
    const [width, height] = [Math.round(box.width * times), Math.round(box.height * times)];
    const from: Place = [hold.x === 1 ? box.x : box.x + box.width - width, hold.y === 1 ? box.y : box.y + box.height - height];
    const to: Place = [from[0] + width, from[1] + height];
    // (The line around letters grows with them.)
    return edit.kind === "text"
      ? {
          ...edit,
          from,
          to,
          size: tenth(edit.size * times),
          // (The one it is drawn with: what the edit says of a line it does not have is left be.)
          ...(howOf(edit).edge > 0 && { outlineWidth: Math.max(0.5, tenth(howOf(edit).edge * times)) }),
        }
      : { ...edit, from, to };
  }
  let [left, top, right, bottom] = [box.x, box.y, box.x + box.width, box.y + box.height];
  if (hold.x === -1) left += by[0];
  if (hold.x === 1) right += by[0];
  if (hold.y === -1) top += by[1];
  if (hold.y === 1) bottom += by[1];
  if (shift && hold.x !== 0 && hold.y !== 0 && box.width > 0 && box.height > 0) {
    // As many times larger both ways, from the corner opposite.
    const times = Math.max(Math.abs(right - left) / box.width, Math.abs(bottom - top) / box.height);
    const [wide, tall] = [box.width * times * Math.sign(right - left || 1), box.height * times * Math.sign(bottom - top || 1)];
    if (hold.x === 1) right = left + wide;
    else left = right - wide;
    if (hold.y === 1) bottom = top + tall;
    else top = bottom - tall;
  }
  // (A side pulled onto the one opposite: it still has an inside, a pixel of it.)
  if (Math.abs(right - left) < 1) [left, right] = hold.x === -1 ? [right - 1, right] : [left, left + 1];
  if (Math.abs(bottom - top) < 1) [top, bottom] = hold.y === -1 ? [bottom - 1, bottom] : [top, top + 1];
  return { ...edit, from: [tenth(left), tenth(top)], to: [tenth(right), tenth(bottom)] } as Edit;
}

/** Draws the capture and what is drawn on it, each edit in its turn, within the picture (`picture`,
    the part of the image there is: all of it, or what it is cut down to). `zoom` is how many pixels
    of the canvas one of the image is, and `view` the part of the image the canvas shows (in the
    window it reaches beyond the picture); the canvas is set to that before. Shown in the window and
    made into the image that leaves by this one function: what leaves is what is shown. Without
    `skip`, the edit at that place in the list. */
function drawAll(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  capture: ImageBitmap | null,
  list: Edit[],
  zoom: number,
  view: Box,
  picture: Box,
  skip: number | null = null,
) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(picture.x, picture.y, picture.width, picture.height);
  ctx.clip();
  if (capture) ctx.drawImage(capture, 0, 0);
  list.forEach((edit, index) => {
    if (index === skip) return;
    if (isHidden(edit)) hide(ctx, canvas, edit, zoom, view, picture);
    else if (isLettered(edit)) letter(ctx, edit);
    else if (isDrawn(edit)) paint(ctx, edit);
  });
  ctx.restore();
}

/** The image that leaves: the capture with the list drawn into it, cut down to what the last crop
    in the list leaves, a pixel of the canvas to one of the image. Nothing when it cannot be made. */
function flattened(capture: ImageBitmap, list: Edit[]): Promise<Blob | null> {
  const whole: Box = { x: 0, y: 0, width: capture.width, height: capture.height };
  const kept = keptOf(list, whole) ?? whole;
  const canvas = document.createElement("canvas");
  canvas.width = kept.width;
  canvas.height = kept.height;
  const ctx = canvas.getContext("2d");
  // (Not as large as asked: the canvas could not be made, and what it would give is not the image.)
  if (!ctx || canvas.width !== kept.width || canvas.height !== kept.height) return Promise.resolve(null);
  ctx.setTransform(1, 0, 0, 1, -kept.x, -kept.y);
  drawAll(ctx, canvas, capture, list, 1, kept, kept);
  return new Promise((made) => {
    try {
      canvas.toBlob(made, "image/png");
    } catch {
      made(null);
    }
  });
}

/** With Shift held: a line along one of eight directions, a shape as wide as it is tall. */
function straightened(tool: Tool, from: Place, to: Place): Place {
  const [across, down] = [to[0] - from[0], to[1] - from[1]];
  if (tool === "rectangle" || tool === "ellipse" || isHider(tool)) {
    const side = Math.max(Math.abs(across), Math.abs(down));
    return [from[0] + Math.sign(across || 1) * side, from[1] + Math.sign(down || 1) * side];
  }
  const eighth = Math.PI / 4;
  const angle = Math.round(Math.atan2(down, across) / eighth) * eighth;
  const length = Math.hypot(across, down);
  return [from[0] + Math.cos(angle) * length, from[1] + Math.sin(angle) * length];
}

// ---------------------------------------------------------------------------------------------
// The drawing and its history

interface Drawing {
  edits: Edit[];
  /** The drawings before this one, oldest first, and the ones undone, latest first. */
  past: Edit[][];
  future: Edit[][];
}

/** Which edges of the frame the image is cut down to are being pulled (-1 the left or top one, 1 the
    right or bottom one), or the whole frame moved, or a new frame drawn. */
type Grip = { x: -1 | 0 | 1; y: -1 | 0 | 1 } | "move" | "new";

/** What is being done with the pointer down: a new edit taking shape, an edit being moved, or the
    frame the image is cut down to being set (from where, and what it was). */
type Gesture =
  | { kind: "draw"; edit: Edit }
  // Some of the edits being moved, together: which, from where, and by how much so far.
  | { kind: "move"; indices: number[]; from: Place; by: Place }
  // A part of the picture being marked out with the tool for selecting: what is in it is selected.
  | { kind: "marquee"; from: Place; to: Place; with?: number[] }
  // One of the edits being made larger or smaller by a hold of it: from where, what it was, and
  // what it is so far.
  | { kind: "resize"; index: number; hold: Hold; from: Place; was: Edit; now: Edit }
  | { kind: "crop"; grip: Grip; from: Place; was: Box }
  // The part of the picture that is shown being dragged along: from where on the screen, and where
  // it was.
  | { kind: "pan"; from: Place; was: Place };

export function Editor({
  capture,
  settle,
  show,
  beside,
  frozen,
  defaultHider,
  t,
  onEdits,
  onCount,
  onFull,
  onUnmade,
}: {
  capture: Capture;
  /** Which way of making a part unreadable is in hand to start with: the one used last, or always
      the same. */
  defaultHider: "last" | "mosaic" | "blur";
  /** A place beside the status under the picture: for how closely the picture is looked at. */
  beside: HTMLElement | null;
  /** The capture is on its way out: nothing is drawn on it meanwhile. */
  frozen: boolean;
  /** Filled in here: finishes what is half done and returns the drawing as it is then, with the
      image made of it; or "stay" when that could not be done, and the capture is not to leave. */
  settle: React.MutableRefObject<(() => Promise<Leaving | "stay">) | null>;
  /** Filled in here: what the tour of the window has it do. */
  show: React.MutableRefObject<EditorShow | null>;
  t: Translate;
  /** What is drawn changed: the window uploads and saves what it was last told here. */
  onEdits: (edits: Edit[]) => void;
  /** How many things are shown drawn changed, and nothing else did: as the tour draws, for a while. */
  onCount: (count: number) => void;
  /** Something was not drawn: the drawing is as long as one gets. */
  onFull: () => void;
  /** The image of the capture as it is drawn on could not be made: it did not leave. */
  onUnmade: () => void;
}) {
  const [drawing, setDrawing] = useState<Drawing>({ edits: capture.edits, past: [], future: [] });
  const [tool, setTool] = useState<Tool>("select");
  // What is drawn next is drawn with these. The pen and the marker each have a colour and a size of
  // their own; shapes and lines share an outline (a colour and a size), and a shape has an inside.
  const [pen, setPen] = useState({ color: RED, step: WIDTHS.first });
  const [marker, setMarker] = useState({ color: MARKERS[0], step: WIDTHS.first });
  const [shapes, setShapes] = useState({ color: RED, step: WIDTHS.first, fillColor: RED, fill: 0 });
  // How solid an inside was the last time it was any: what it is again when it is given a colour.
  const solid = useRef(1);
  // Which of the shapes and lines was in hand last: what the button for them picks up again.
  const [figure, setFigure] = useState<Tool>("rectangle");
  // How strongly a part is made unreadable, by each of the two ways, and which of them was in hand
  // last.
  const [strengths, setStrengths] = useState(keptStrengths);
  const [hider, setHider] = useState<"blur" | "mosaic">(() => firstHider(defaultHider));
  // (Kept as it is in hand, whatever put it there: what the tour has in hand for a while, it puts
  // back.)
  useEffect(() => keepHider(hider), [hider]);
  // The frame the image is being cut down to, while the tool for that is in hand; and the shape it
  // is held to, if any.
  const [frame, setFrame] = useState<Box | null>(null);
  const [ratio, setRatio] = useState<number | null>(null);
  // What is written next is written in this colour, this size (in pixels of the image) and this
  // font; and the numbers have their colour and size.
  const [letters, setLetters] = useState<Writing>(() => ({
    color: RED,
    size: LETTERS.first * LETTERS.step * unit(capture),
    font: keptFont(),
    fill: 1,
    outline: null,
    edge: 0,
    // (Bold to start with: what is written on a picture is to be read at a glance.)
    bold: true,
    italic: false,
    strike: false,
  }));
  const [stamps, setStamps] = useState({ color: RED, step: STAMPS.first });
  // What is being written: where, what so far, and which of the edits it is when it is one that
  // was written before and is written anew.
  const [typing, setTyping] = useState<{ index: number | null; at: Place; text: string } | null>(null);
  // Whether the last of the edits was drawn with the tool in hand, since it was taken in hand.
  const justDrawn = useRef(false);
  // The panel asked for under a button of the bars, if one was.
  const [asked, setPanel] = useState<PanelName | null>(null);
  // What is selected: one of the edits, which has its settings and its holds; or several of them
  // (marked out together), which are moved and taken away together and have neither. Never both.
  const [selected, selectOne] = useState<number | null>(null);
  const [several, setSeveral] = useState<number[]>([]);
  const setSelected = (one: number | null) => {
    selectOne(one);
    setSeveral((now) => (now.length === 0 ? now : []));
  };
  /** These are selected: one as one is, more than one together. */
  const setSelection = (those: number[]) => {
    if (those.length < 2) return setSelected(those[0] ?? null);
    selectOne(null);
    setSeveral(those);
  };
  // What the pointer is doing, for drawing it (the state) and for the next thing the pointer does
  // (the ref): two moves can come before the page is drawn again, and each is to see the other.
  const [gesture, showGesture] = useState<Gesture | null>(null);
  const hand = useRef<Gesture | null>(null);
  const setGesture = (next: Gesture | null) => {
    hand.current = next;
    showGesture(next);
  };

  // Each capture starts with what the application has of it: nothing, usually.
  useEffect(() => {
    setDrawing({ edits: capture.edits, past: [], future: [] });
    setSelected(null);
    setGesture(null);
    setTyping(null);
    setTool("select");
    // (What is written starts as large as suits this capture, not as the one before was written on.)
    setLetters((now) => ({ ...now, size: LETTERS.first * LETTERS.step * unit(capture) }));
  }, [capture.id]);

  const { edits } = drawing;
  // The drawing as it is now, for what was started a moment ago and finds out only later.
  const latest = useRef(edits);
  latest.current = edits;
  /** The drawing is `next` from now on, here and for the application. */
  const tell = (next: Edit[]) => {
    onEdits(next);
    tellEdits(capture, next).catch(onFull);
  };
  // The setting last changed and when: more of the same right after is the same step.
  const lastSet = useRef({ what: "", at: 0 });
  const change = (next: Edit[], setting = "") => {
    if (!fits(next)) return onFull();
    latest.current = next;
    const now = Date.now();
    const same = setting !== "" && lastSet.current.what === setting && now - lastSet.current.at < ONE_STEP;
    lastSet.current = { what: setting, at: now };
    setDrawing((now) => ({ edits: next, past: same ? now.past : [...now.past, now.edits], future: [] }));
    tell(next);
  };
  const undo = () => {
    const before = drawing.past[drawing.past.length - 1];
    // (While something is being written, undoing is about what is written.)
    if (!before || typing) return;
    justDrawn.current = false;
    setTaking(false);
    lastSet.current = { what: "", at: 0 };
    setDrawing({ edits: before, past: drawing.past.slice(0, -1), future: [edits, ...drawing.future] });
    setSelected(null);
    tell(before);
  };
  const redo = () => {
    const [after, ...rest] = drawing.future;
    if (!after || typing) return;
    justDrawn.current = false;
    setTaking(false);
    lastSet.current = { what: "", at: 0 };
    setDrawing({ edits: after, past: [...drawing.past, edits], future: rest });
    setSelected(null);
    tell(after);
  };
  const remove = () => {
    if (selected === null && several.length === 0) return;
    change(edits.filter((_, index) => index !== selected && !several.includes(index)));
    setSelected(null);
  };

  // What is selected is copied, and what was copied is put on the picture again: beside where it
  // was, a little further with each time, on top of everything, and selected (to be moved to its
  // place). Kept here and not on the clipboard, which has the capture; and for this capture only.
  const copied = useRef<{ edits: Edit[]; times: number } | null>(null);
  useEffect(() => {
    copied.current = null;
  }, [capture.id]);
  const picked = () => (selected !== null ? [selected] : several).flatMap((index) => (edits[index] ? [edits[index]] : []));
  /** Whether they were put there: not when the drawing is as long as one gets. */
  const put = (those: Edit[], times: number) => {
    if (those.length === 0) return false;
    const by = Math.max(1, Math.round(16 / (scale || 1))) * times;
    // A number that is on the picture already is not there twice: one put beside it is the next
    // number, as if it had been set down by hand. (One taken out and put back is the number it was.)
    const numbers = new Set(edits.flatMap((edit) => (edit.kind === "stamp" ? [edit.number] : [])));
    const numbered = (edit: Edit): Edit => {
      if (edit.kind !== "stamp") return edit;
      const number = numbers.has(edit.number) ? 1 + Math.max(0, ...numbers) : edit.number;
      numbers.add(number);
      return { ...edit, number };
    };
    const next = [...edits, ...those.map((edit) => numbered(moved(edit, [by, by])))];
    if (!fits(next)) {
      onFull();
      return false;
    }
    if (tool !== "select") hold("select");
    change(next);
    setSelection(those.map((_, index) => edits.length + index));
    return true;
  };
  const copy = () => {
    const those = picked();
    if (those.length > 0) copied.current = { edits: those, times: 1 };
  };
  /** Copied and taken away: put back, the first time, where it was. */
  const takeOut = () => {
    const those = picked();
    if (those.length === 0) return;
    copied.current = { edits: those, times: 0 };
    remove();
  };
  const paste = () => {
    const { edits: those, times } = copied.current ?? { edits: [], times: 0 };
    // (Where it was only if it is no longer there: taken out and brought back by undoing, it
    // would lie exactly on itself, and nothing would be seen of it.)
    const there = (one: Edit) => edits.some((edit) => JSON.stringify(edit) === JSON.stringify(one));
    const from = times === 0 && those.some(there) ? 1 : times;
    if (copied.current && put(those, from)) copied.current.times = from + 1;
  };
  const duplicate = () => void put(picked(), 1);

  const chosen = selected !== null ? (edits[selected] ?? null) : null;
  // What the settings are about: what is selected; or, with nothing selected, what was just drawn
  // while the tool that drew it is still in hand. That one can be set right at a look (another
  // colour, wider, stronger) without being selected first. Otherwise they are only for what is
  // drawn next; and whatever they are set to, they are that for what is drawn next as well.
  const lastIndex = edits.length - 1;
  const aboutIndex =
    selected ?? (justDrawn.current && lastIndex >= 0 && edits[lastIndex].kind === tool ? lastIndex : null);
  const about = aboutIndex !== null ? (edits[aboutIndex] ?? null) : null;
  const alter = (next: Edit, setting: string) => {
    // Set to what it is already (the hue of a grey, the colour in use pressed again): nothing to undo.
    if (JSON.stringify(next) === JSON.stringify(about)) return;
    change(
      edits.map((edit, index) => (index === aboutIndex ? next : edit)),
      setting === "" ? "" : `${aboutIndex}:${setting}`,
    );
  };
  /** The name under which changes of a setting are gathered into one step to undo: for what is
      dragged (a slider, the field of shades). A press is a step of its own. */
  const gathered = (what: object, dragged: boolean) => (dragged ? Object.keys(what).join() : "");
  // Of each kind: the one selected (which the bars show the settings of), and the one the settings
  // are about (which they change).
  const strokeOf = (edit: Edit | null) => (edit && "points" in edit ? edit : null);
  const figureOf = (edit: Edit | null) => (edit && isDrawn(edit) && !("points" in edit) ? edit : null);
  // (By what it is, not by its having an inside: what is written has one too.)
  const shapeOf = (edit: Edit | null) => (edit && (edit.kind === "rectangle" || edit.kind === "ellipse") ? edit : null);
  const hiddenOf = (edit: Edit | null) => (edit && isHidden(edit) ? edit : null);
  const writtenOf = (edit: Edit | null) => (edit?.kind === "text" ? edit : null);
  const stampedOf = (edit: Edit | null) => (edit?.kind === "stamp" ? edit : null);
  const [chosenStroke, chosenFigure, chosenShape, chosenHidden, chosenWritten, chosenStamped] = [
    strokeOf(chosen),
    figureOf(chosen),
    shapeOf(chosen),
    hiddenOf(chosen),
    writtenOf(chosen),
    stampedOf(chosen),
  ];
  const stepOf = (edit: Drawn) => Math.round(edit.width / unit(capture));
  const widthOf = (what: { step?: number }) => what.step !== undefined && { width: what.step * unit(capture) };

  const outline = chosenFigure ? { color: chosenFigure.color, step: stepOf(chosenFigure) } : shapes;
  const setOutline = (what: { color?: string; step?: number }, dragged = false) => {
    setShapes({ ...shapes, ...what });
    const figure = figureOf(about);
    if (!figure) return;
    const next = { ...figure, ...(what.color !== undefined && { color: what.color }), ...widthOf(what) };
    alter(next as Edit, gathered(what, dragged));
  };
  const inside = chosenShape
    ? { fillColor: chosenShape.fillColor ?? chosenShape.color, fill: chosenShape.fill }
    : shapes;
  const setInside = (what: { fillColor?: string; fill?: number }, dragged = false) => {
    // An inside that had no colour and is given one is as solid as it last was.
    const next = what.fillColor !== undefined && inside.fill === 0 ? { ...what, fill: solid.current } : what;
    if (next.fill) solid.current = next.fill;
    // What is drawn next has the inside this one has now, all of it.
    setShapes({ ...shapes, fillColor: inside.fillColor, fill: inside.fill, ...next });
    const shape = shapeOf(about);
    if (shape) alter({ ...shape, ...next }, gathered(what, dragged));
  };
  const setStroke = (kind: "pen" | "marker", what: { color?: string; step?: number }, dragged = false) => {
    if (kind === "pen") setPen({ ...pen, ...what });
    else setMarker({ ...marker, ...what });
    const stroke = strokeOf(about);
    if (!stroke || stroke.kind !== kind) return;
    const next = { ...stroke, ...(what.color !== undefined && { color: what.color }), ...widthOf(what) };
    alter(next, gathered(what, dragged));
  };

  // What is written: its colour, the size of its letters in pixels of the image, and its font.
  // Changed, the one the setting is about (what is being written; or what is selected, whose
  // picture is made anew) is written so. The numbers: their colour, and their size in steps here.
  const lettersNow: Writing = chosenWritten ? howOf(chosenWritten) : letters;
  const setLettering = (what: Partial<Writing>, dragged = false) => {
    let next: Writing = { ...lettersNow, ...what };
    // A line around the letters has a width from the start: a twelfth of their size.
    if (next.outline && !(next.edge > 0)) next = { ...next, edge: firstEdge(next.size) };
    // Letters are never without both: with no inside they have a line around them (in the colour
    // their inside had), and with no line around them an inside.
    if (next.fill === 0 && !next.outline) {
      next = what.outline === null ? { ...next, fill: 1 } : { ...next, outline: next.color, edge: next.edge || firstEdge(next.size) };
    }
    // The letters stay where they are: with more or less room around them for the line, the
    // corner of what is written is that much further out or in.
    const nearer = reachOf(lettersNow) - reachOf(next);
    const one = typing ? null : writtenOf(about);
    const anew = one && written([one.from[0] + nearer, one.from[1] + nearer], one.text, next);
    // (What is selected cannot be written so: too large to make a picture of. Nothing is changed,
    // and the next thing written is not written so either.)
    if (one && !anew) return onFull();
    setLetters(next);
    if (typing && nearer !== 0) setTyping({ ...typing, at: [typing.at[0] + nearer, typing.at[1] + nearer] });
    if (what.font !== undefined) keepFont(next.font);
    if (anew) alter(anew, gathered(what, dragged));
  };
  const stampsNow = chosenStamped
    ? {
        color: chosenStamped.color,
        step: Math.round(boxOf(chosenStamped).width / (STAMPS.step * unit(capture))),
      }
    : stamps;
  const setStamping = (what: { color?: string; step?: number }, dragged = false) => {
    const next = { ...stampsNow, ...what };
    setStamps(next);
    const one = stampedOf(about);
    if (!one) return;
    const box = boxOf(one);
    const middle: Place = [box.x + box.width / 2, box.y + box.height / 2];
    const across = what.step !== undefined ? next.step * STAMPS.step * unit(capture) : box.width;
    alter(stamped(middle, across, one.number, next.color), gathered(what, dragged));
  };
  /** How large the letters of what is being written are, in pixels of the image. */
  const typingSize = letters.size;
  /** What is being written is done: it is one of the edits now (or, with nothing written, none of
      them any more). Returns the drawing as it is then; nothing when it could not be done, and is
      still being written. */
  const finish = (): Edit[] | null => {
    if (!typing) return edits;
    const blank = typing.text.trim() === "";
    const one = blank ? null : written(typing.at, typing.text, letters);
    const next =
      typing.index === null
        ? one
          ? [...edits, one]
          : edits
        : one
          ? edits.map((edit, index) => (index === typing.index ? one : edit))
          : edits.filter((_, index) => index !== typing.index);
    // No picture could be made of it, or the drawing is as long as one gets: it stays as it is,
    // still being written.
    if ((!blank && !one) || !fits(next)) {
      onFull();
      return null;
    }
    setTyping(null);
    if (JSON.stringify(next) === JSON.stringify(edits)) return edits;
    change(next);
    justDrawn.current = typing.index === null;
    return next;
  };
  /** One of the edits, written before, is written anew: in its place, as it is written. */
  const rewrite = (index: number) => {
    const one = writtenOf(edits[index] ?? null);
    if (!one) return;
    setLetters(howOf(one));
    setSelected(null);
    setTyping({ index, at: one.from, text: one.text });
  };

  // How strongly: in steps here, in pixels of the image in an edit.
  const hiddenBy = (kind: "blur" | "mosaic", step: number) =>
    kind === "blur"
      ? { radius: tenth((BLUR.least + (step - 1) * perStep(BLUR)) * unit(capture)) }
      : { cell: tenth((MOSAIC.least + (step - 1) * perStep(MOSAIC)) * unit(capture)) };
  const strengthNow = chosenHidden
    ? 1 +
      Math.round(
        chosenHidden.kind === "blur"
          ? (chosenHidden.radius / unit(capture) - BLUR.least) / perStep(BLUR)
          : (chosenHidden.cell / unit(capture) - MOSAIC.least) / perStep(MOSAIC),
      )
    : strengths[tool === "blur" || tool === "mosaic" ? tool : hider];
  const setHiding = (step: number) => {
    const hidden = hiddenOf(about);
    // Each way has a strength of its own: that of the part the setting is about, or of the tool.
    const way = hidden?.kind ?? (tool === "blur" || tool === "mosaic" ? tool : hider);
    const next = { ...strengths, [way]: step };
    setStrengths(next);
    keepStrengths(next);
    if (hidden) alter({ ...hidden, ...hiddenBy(hidden.kind, step) } as Edit, "strength");
  };

  // The image as it is captured, and the part of it that is shown: what it is cut down to, except
  // while the frame for that is being set.
  const whole: Box = { x: 0, y: 0, width: capture.width, height: capture.height };
  const view = tool === "crop" ? whole : (keptOf(edits, whole) ?? whole);
  /** The image is cut down to the frame as it is set, and the tool put down. Returns the drawing as
      it is then. */
  const cut = (): Edit[] => {
    const kept = frame ?? whole;
    const rest = edits.filter((edit) => edit.kind !== "crop");
    const all = kept.width >= whole.width && kept.height >= whole.height;
    const next: Edit[] = all
      ? rest
      : [...rest, { kind: "crop", from: [kept.x, kept.y], to: [kept.x + kept.width, kept.y + kept.height] }];
    const cutNow = JSON.stringify(next) !== JSON.stringify(edits) && fits(next);
    if (cutNow) change(next);
    setSelected(null);
    setTool("select");
    return cutNow ? next : edits;
  };
  // Asked for before the capture leaves: a frame that is set but not yet cut to is cut to. What was
  // put outside it was meant to stay behind.
  // And what is being written is done.
  // What is drawn then is made into the image that leaves: here, where it is shown. Never without
  // the image when something is drawn: the capture does not leave bare.
  settle.current = async () => {
    const list = tool === "crop" ? cut() : finish();
    if (list === null) return "stay";
    if (list.length === 0) return { edits: list, picture: null };
    const picture = bitmap ? await flattened(bitmap, list) : null;
    // (Drawn on while the image was being made: the image is of a drawing that is no longer.)
    if (!picture || latest.current !== list) {
      onUnmade();
      return "stay";
    }
    return { edits: list, picture };
  };

  // The second bar: for the shapes and lines, for the ways of making a part unreadable and for
  // cutting the image down when one of their tools is in hand, or for what is selected.
  //
  // The bar of a tool is there when the tool is taken by its button, until the picture is pressed
  // on: for as long as the tool is being chosen, not while it is drawn with. It can be put away,
  // and brought back, by a press on the tool that is in hand already. A tool taken by its key or
  // with the wheel has no bar, and nor does what is selected until it is asked for: nothing comes
  // down over the picture that was not asked for by a press. The settings are had where the
  // pointer is (see `afloat`), and the top of the picture is free.
  const [shut, setShut] = useState(true);
  // (The tool whose button was pressed, until it is in hand.)
  const byButton = useRef<Tool | null>(null);
  const afloatNow = useRef(false);
  useEffect(() => {
    // (Nor a tool picked in the bar that is where the pointer is: its bar is there.)
    setShut(byButton.current !== tool || afloatNow.current);
    byButton.current = null;
  }, [tool, chosen === null]);
  // The settings of the tool in hand (or of what is selected), asked for where the pointer is, by
  // pressing the other button of the mouse twice: its bar there, or the panel of the pen and the
  // marker, which have no bar. A press elsewhere, Escape, another tool, and they are gone again.
  const [afloat, setAfloat] = useState<{ x: number; y: number } | null>(null);
  afloatNow.current = afloat !== null;
  // (Another shape picked in the bar that is there, or the other way to make a part unreadable,
  // and it is still the bar of that: it stays.)
  const kind = isFigure(tool) ? "figure" : isHider(tool) ? "hider" : tool;
  useEffect(() => setAfloat(null), [kind, selected]);
  useEffect(() => {
    if (!afloat) return;
    const away = (event: PointerEvent) => {
      // (Opened by the tour, they are the tour's to put away.)
      if (touring()) return;
      if (!(event.target instanceof Element && event.target.closest(".afloat"))) setAfloat(null);
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  }, [afloat]);
  const offered = chosenStroke
    ? "stroke"
    : chosenShape
      ? "shape"
      : chosenFigure
        ? "line"
        : chosenHidden
          ? "hidden"
          : chosenStamped || (tool === "stamp" && !chosen)
              ? "stamps"
          : isFigure(tool)
            ? "figures"
            : isHider(tool)
              ? "hiding"
              : tool === "crop"
                ? "crop"
                : null;
  // (Of what is selected, too: selecting something brings no bar down by itself. It is asked for
  // by a press on the tool for selecting, or where the pointer is.)
  const second = shut && !afloat ? null : offered;
  // (No room is made for the second bar: it lies over the top of the picture for as long as it is
  // there, and the picture stays where it is whatever is taken in hand.)
  const fills = second === "shape" || (second === "figures" && (tool === "rectangle" || tool === "ellipse"));
  // A panel is open only where its button is: asked for, and still belonging to what is in hand.
  const panel =
    asked === "pen" || asked === "marker"
      ? tool === asked && !chosen
        ? asked
        : null
      : asked === "stroke"
        ? second === "stroke"
          ? asked
          : null
        : asked === "fill"
          ? fills
            ? asked
            : null
          : asked === "ratio" || asked === "letters" || asked === "stamps"
            ? second === (asked === "ratio" ? "crop" : asked)
              ? asked
              : null
            : asked === "outline" && (second === "figures" || second === "shape" || second === "line")
              ? asked
              : null;
  useEffect(() => setPanel(null), [tool, selected]);
  // A press anywhere but in the panel and on what opened it puts it away.
  useEffect(() => {
    if (!panel) return;
    const onPress = (event: PointerEvent) => {
      if (touring()) return;
      if (!(event.target instanceof Element && event.target.closest(".anchor"))) setPanel(null);
    };
    window.addEventListener("pointerdown", onPress, true);
    return () => window.removeEventListener("pointerdown", onPress, true);
  }, [panel]);
  // What is open under a button stays inside the window when the window changes.
  useEffect(() => {
    const again = () => document.querySelectorAll<HTMLElement>(".tools .panel").forEach(inView);
    window.addEventListener("resize", again);
    return () => window.removeEventListener("resize", again);
  }, []);
  /** A tool is taken in hand. */
  const hold = (one: Tool) => {
    // (What is being written is done first: it is not given up for another tool. Where it cannot be
    // done, it stays in hand.)
    if (finish() === null) {
      byButton.current = null;
      return;
    }
    justDrawn.current = false;
    setTaking(false);
    setPointing(null);
    setTool(one);
    if (isFigure(one)) setFigure(one);
    if (one === "blur" || one === "mosaic") setHider(one);
    // Cutting down starts from what the image is cut down to already, or from all of it, in any
    // shape.
    setFrame(one === "crop" ? (keptOf(edits, whole) ?? whole) : null);
    setRatio(null);
    if (one !== "select") setSelected(null);
    setPanel(null);
  };
  /** A tool is pressed: taken in hand; the one in hand already, its panel opened or put away. */
  const pick = (one: Tool) => {
    if (one === tool && !chosen && (one === "pen" || one === "marker")) return setPanel(panel === one ? null : one);
    // The tool in hand pressed again: its bar goes, or comes back.
    // (Not in the bar that is where the pointer is: that one is there, and the other stays away.)
    if (one === tool && offered !== null) {
      setPanel(null);
      if (!afloat) setShut(!shut);
      return;
    }
    // (Remembered only when it is not in hand yet: it is forgotten when it is.)
    if (one !== tool) byButton.current = one;
    hold(one);
  };

  // The picture as large as its place allows, never larger than it is.
  const stage = useRef<HTMLDivElement>(null);
  const [room, setRoom] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const element = stage.current;
    if (!element) return;
    const measure = () => setRoom({ width: element.clientWidth, height: element.clientHeight });
    measure();
    const watching = new ResizeObserver(measure);
    watching.observe(element);
    return () => watching.disconnect();
  }, []);
  // How closely it is looked at: once is all of it (of what it is cut down to), fitted to its place;
  // more is a part of it, as much larger; less is all of it, smaller. And which place of the picture
  // is in the middle of the place it is shown in (none: the middle of the picture).
  const [looking, setLooking] = useState<{ times: number; x?: number; y?: number }>({ times: 1 });
  // (Not when the tour puts the window back from cutting down, which shows all of the picture: the
  // tour gives the look back as it was.)
  const lookKept = useRef(false);
  useEffect(() => {
    if (lookKept.current) {
      lookKept.current = false;
      return;
    }
    setLooking({ times: 1 });
  }, [capture.id, view.x, view.y, view.width, view.height]);
  const fit = Math.min(1, room.width / view.width, room.height / view.height) || 0;
  const closest = fit > 0 ? Math.max(1, CLOSEST / fit) : 1;
  const times = Math.min(closest, Math.max(FARTHEST, looking.times));
  /** How many pixels of the screen one of the image is. */
  const scale = fit * times;
  /** What is shown, the whole place, and the part of the image that is (reaching beyond the picture
      where it does not fill the place), looked at so closely with that place of the picture in the
      middle; and that place. The middle is always somewhere on the picture: it is never out of
      sight, however far it is moved (owner, 2026-10-14). */
  const lookAt = (times: number, x?: number, y?: number) => {
    const by = fit * times;
    const shown = { width: room.width, height: room.height };
    const middle: Place = [
      Math.min(view.x + view.width, Math.max(view.x, x ?? view.x + view.width / 2)),
      Math.min(view.y + view.height, Math.max(view.y, y ?? view.y + view.height / 2)),
    ];
    if (by === 0) return { shown, part: view, middle };
    const [width, height] = [room.width / by, room.height / by];
    // (The corner of the picture on a whole pixel of the screen: its pixels are not smudged across
    // two, and what is shown at a pixel to a pixel is the image.)
    // (As the canvas draws it: so many pixels of it as fill the place, which need not be the place
    // times the pixels of the screen to one of the page.)
    const pixel = room.width > 0 ? (by * Math.max(1, Math.round(room.width * (window.devicePixelRatio || 1)))) / room.width : by;
    const [x0, y0] = [middle[0] - width / 2, middle[1] - height / 2];
    return {
      shown,
      part: { x: view.x - Math.round((view.x - x0) * pixel) / pixel, y: view.y - Math.round((view.y - y0) * pixel) / pixel, width, height },
      middle,
    };
  };
  const { shown: shownSize, part: look, middle } = lookAt(times, looking.x, looking.y);
  /** Whether some of a part of the image is in sight: in what is shown, and of the picture (not
      past where the image was cut down to, which the place may show around it now). */
  const inSight = (box: Box) =>
    box.x < Math.min(look.x + look.width, view.x + view.width) &&
    box.x + box.width > Math.max(look.x, view.x) &&
    box.y < Math.min(look.y + look.height, view.y + view.height) &&
    box.y + box.height > Math.max(look.y, view.y);
  /** Whether a press at a place is on a thing drawn, as `isAt`: where the picture is, that is.
      Beside it, what is drawn is cut off, out of sight, and no press there is on it. */
  const seenAt = (edit: Edit, at: Place, near: number) =>
    at[0] >= view.x - near && at[0] <= view.x + view.width + near && at[1] >= view.y - near && at[1] <= view.y + view.height + near && isAt(edit, at, near);
  // Space held down: the pointer takes hold of the picture, not of a tool.
  const [grabbing, setGrabbing] = useState(false);

  // What is on the canvas: the picture, every edit in its turn (the one being moved, where it is
  // now), the one taking shape, a frame around the selected one, and while the image is being cut
  // down, the frame for that.
  const canvas = useRef<HTMLCanvasElement>(null);
  // The capture's picture, to draw on the canvas: asked of the application as a file, so that what
  // is drawn over it can be read back (see `captureFile`). With its colours as they are in the file,
  // to the last bit: what is not drawn on leaves as it was captured.
  const [bitmap, setBitmap] = useState<ImageBitmap | null>(null);
  useEffect(() => {
    let gone = false;
    setBitmap(null);
    captureFile(capture)
      .then((file) => createImageBitmap(file, { premultiplyAlpha: "none", colorSpaceConversion: "none" }))
      .then(
        (picture) => (gone ? picture.close() : setBitmap(picture)),
        // (Shown all the same, from its address, under the canvas; it cannot leave drawn on.)
        () => {},
      )
      // The application is told when the capture is drawn (a moment after there is something to
      // draw, or nothing more to wait for): a window that opens with a capture opens then.
      .finally(() => {
        if (!gone) window.setTimeout(() => void drawn(capture).catch(() => {}), 40);
      });
    return () => {
      gone = true;
    };
  }, [capture.id]);
  useEffect(() => () => bitmap?.close(), [bitmap]);
  const visible = edits.map((edit, index) =>
    gesture?.kind === "move" && gesture.indices.includes(index)
      ? moved(edit, gesture.by)
      : gesture?.kind === "resize" && gesture.index === index
        ? gesture.now
        : edit,
  );
  useEffect(() => {
    const element = canvas.current;
    const ctx = element?.getContext("2d");
    if (!element || !ctx || scale === 0) return;
    const dense = window.devicePixelRatio || 1;
    // (Made anew only when its size changes: the canvas is the whole place, and is drawn again with
    // each move of the pointer. Otherwise cleared, and its settings as they were made.)
    const [width, height] = [Math.max(1, Math.round(shownSize.width * dense)), Math.max(1, Math.round(shownSize.height * dense))];
    if (element.width !== width || element.height !== height) {
      element.width = width;
      element.height = height;
    } else ctx.reset();
    const zoom = element.width / look.width;
    ctx.setTransform(zoom, 0, 0, zoom, -look.x * zoom, -look.y * zoom);
    // Looked at closely, the pixels of the picture are shown as the squares they are.
    ctx.imageSmoothingEnabled = zoom < 3;
    // The picture itself is drawn here too: what makes a part unreadable works on what is drawn.
    // (What is being written anew is in the field over the picture, not also under it.)
    drawAll(ctx, element, bitmap, gesture?.kind === "draw" ? [...visible, gesture.edit] : visible, zoom, look, view, typing?.index ?? null);

    // Those selected together: a frame around each, and nothing to take hold of them by but that.
    for (const index of several) {
      const one = visible[index];
      if (!one) continue;
      const box = boxOf(one);
      const margin = frameAround(one, scale);
      ctx.save();
      ctx.lineWidth = 1.5 / scale;
      ctx.setLineDash([5 / scale, 4 / scale]);
      ctx.strokeStyle = "#ffffff";
      ctx.strokeRect(box.x - margin, box.y - margin, box.width + margin * 2, box.height + margin * 2);
      ctx.lineDashOffset = 5 / scale;
      ctx.strokeStyle = "#000000";
      ctx.strokeRect(box.x - margin, box.y - margin, box.width + margin * 2, box.height + margin * 2);
      ctx.restore();
    }
    // The part being marked out: what is wholly in it will be selected.
    if (gesture?.kind === "marquee") {
      const [x, y] = [Math.min(gesture.from[0], gesture.to[0]), Math.min(gesture.from[1], gesture.to[1])];
      const [width, height] = [Math.abs(gesture.to[0] - gesture.from[0]), Math.abs(gesture.to[1] - gesture.from[1])];
      ctx.save();
      ctx.fillStyle = "rgb(79 140 255 / 14%)";
      ctx.fillRect(x, y, width, height);
      ctx.lineWidth = 1 / scale;
      ctx.strokeStyle = "#ffffff";
      ctx.strokeRect(x, y, width, height);
      ctx.setLineDash([4 / scale, 3 / scale]);
      ctx.strokeStyle = "#2f6fe0";
      ctx.strokeRect(x, y, width, height);
      ctx.restore();
    }

    const framed = selected !== null ? visible[selected] : undefined;
    if (framed) {
      const box = boxOf(framed);
      const margin = frameAround(framed, scale);
      ctx.save();
      ctx.lineWidth = 1.5 / scale;
      ctx.setLineDash([5 / scale, 4 / scale]);
      ctx.strokeStyle = "#ffffff";
      ctx.strokeRect(box.x - margin, box.y - margin, box.width + margin * 2, box.height + margin * 2);
      ctx.lineDashOffset = 5 / scale;
      ctx.strokeStyle = "#000000";
      ctx.strokeRect(box.x - margin, box.y - margin, box.width + margin * 2, box.height + margin * 2);
      // Where it can be taken hold of to change its size: squares on the frame, discs on the ends
      // of a line.
      ctx.setLineDash([]);
      ctx.fillStyle = "#ffffff";
      ctx.strokeStyle = "rgb(0 0 0 / 70%)";
      ctx.lineWidth = 1 / scale;
      const grip = 9 / scale;
      for (const { hold, at } of holdsOf(framed, scale)) {
        ctx.beginPath();
        if (typeof hold === "string") ctx.arc(at[0], at[1], grip / 2, 0, Math.PI * 2);
        else ctx.rect(at[0] - grip / 2, at[1] - grip / 2, grip, grip);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
    }

    if (tool === "crop" && frame) {
      // What would be cut away is dimmed; the frame has a grip at each corner and each side.
      const [right, bottom] = [frame.x + frame.width, frame.y + frame.height];
      ctx.save();
      ctx.fillStyle = "rgb(0 0 0 / 60%)";
      ctx.fillRect(0, 0, whole.width, frame.y);
      ctx.fillRect(0, bottom, whole.width, whole.height - bottom);
      ctx.fillRect(0, frame.y, frame.x, frame.height);
      ctx.fillRect(right, frame.y, whole.width - right, frame.height);
      ctx.lineWidth = 1.5 / scale;
      ctx.strokeStyle = "#ffffff";
      ctx.strokeRect(frame.x, frame.y, frame.width, frame.height);
      const grip = 9 / scale;
      ctx.fillStyle = "#ffffff";
      ctx.strokeStyle = "rgb(0 0 0 / 70%)";
      ctx.lineWidth = 1 / scale;
      for (const x of [frame.x, frame.x + frame.width / 2, right]) {
        for (const y of [frame.y, frame.y + frame.height / 2, bottom]) {
          if (x === frame.x + frame.width / 2 && y === frame.y + frame.height / 2) continue;
          ctx.fillRect(x - grip / 2, y - grip / 2, grip, grip);
          ctx.strokeRect(x - grip / 2, y - grip / 2, grip, grip);
        }
      }
      ctx.restore();
    }
  });

  /** Where a pointer is, in pixels of the image. */
  // (Measured from the canvas wherever the pointer is: the room around the picture is pressed on too.)
  const placeOf = (event: React.PointerEvent): Place => {
    const box = (canvas.current ?? event.currentTarget).getBoundingClientRect();
    return [
      tenth(look.x + ((event.clientX - box.left) / box.width) * look.width),
      tenth(look.y + ((event.clientY - box.top) / box.height) * look.height),
    ];
  };

  /** The wheel looks closer or from further away, at the place under the pointer. */
  const onWheel = (event: React.WheelEvent) => {
    // (Not over the field something is written in, nor while it is carried by its band: it would
    // jump, carried at another size.)
    if (carrying.current || (event.target instanceof Element && event.target.closest(".holding"))) return;
    zoomAt(event.clientX, event.clientY, event.deltaY);
  };
  const zoomAt = (x: number, y: number, deltaY: number) => {
    const box = canvas.current?.getBoundingClientRect();
    if (scale === 0 || hand.current || !box) return;
    const [across, down] = [(x - box.left) / box.width, (y - box.top) / box.height];
    const under: Place = [look.x + across * look.width, look.y + down * look.height];
    let next = Math.min(closest, Math.max(FARTHEST, times * Math.exp(-deltaY * 0.0015)));
    // (Passing fitted, it stops there once: the size all of it fits in is easy to come back to.
    // Where it was moved to stays; looking at all of it again puts it in the middle.)
    if ((times > 1 && next < 1) || (times < 1 && next > 1)) next = 1;
    if (next === times) return;
    // That place stays under the pointer: as far across what is shown then as it is now.
    const then = lookAt(next).part;
    setLooking({ times: next, x: under[0] - (across - 0.5) * then.width, y: under[1] - (down - 0.5) * then.height });
  };

  /** The frame the image is cut down to, after the pointer went from where it took hold to `at`. */
  const pulled = (grip: Grip, was: Box, from: Place, at: Place): Box => {
    const within = (value: number, least: number, most: number) => Math.round(Math.min(most, Math.max(least, value)));
    const [across, down] = [at[0] - from[0], at[1] - from[1]];
    if (grip === "move") {
      return {
        ...was,
        x: within(was.x + across, 0, whole.width - was.width),
        y: within(was.y + down, 0, whole.height - was.height),
      };
    }
    if (ratio !== null) {
      // Held to a shape. A corner (or a new frame) is pulled away from the corner opposite, which
      // stays; a side from the side opposite, the frame staying in the middle the other way. It is
      // as large as the pointer asks, and no larger than the image has room for from there. (From
      // an edge of the image there is room only inwards: that way it goes, wherever the pointer is.)
      const [right, bottom] = [was.x + was.width, was.y + was.height];
      const inwards = (held: number, most: number, asked: boolean) => (held <= 0 ? true : held >= most ? false : asked);
      if (grip === "new" || (grip.x !== 0 && grip.y !== 0)) {
        const held: Place =
          grip === "new"
            ? [within(from[0], 0, whole.width), within(from[1], 0, whole.height)]
            : [grip.x === 1 ? was.x : right, grip.y === 1 ? was.y : bottom];
        const to: Place =
          grip === "new" ? at : [(grip.x === 1 ? right : was.x) + across, (grip.y === 1 ? bottom : was.y) + down];
        const [wide, tall] = [to[0] - held[0], to[1] - held[1]];
        const toRight = inwards(held[0], whole.width, wide === 0 && grip !== "new" ? grip.x === 1 : wide >= 0);
        const toBottom = inwards(held[1], whole.height, tall === 0 && grip !== "new" ? grip.y === 1 : tall >= 0);
        const roomAcross = toRight ? whole.width - held[0] : held[0];
        const roomDown = toBottom ? whole.height - held[1] : held[1];
        const asked = Math.max(Math.abs(wide), Math.abs(tall) * ratio);
        const width = Math.max(1, Math.round(Math.min(asked, roomAcross, roomDown * ratio)));
        const height = Math.max(1, Math.min(roomDown, Math.round(width / ratio)));
        return {
          x: within(toRight ? held[0] : held[0] - width, 0, whole.width - width),
          y: within(toBottom ? held[1] : held[1] - height, 0, whole.height - height),
          width,
          height,
        };
      }
      if (grip.x !== 0) {
        const held = grip.x === 1 ? was.x : right;
        const wide = (grip.x === 1 ? right : was.x) + across - held;
        const toRight = inwards(held, whole.width, wide === 0 ? grip.x === 1 : wide > 0);
        // (Grown past the room above or below its middle, it moves over to where there is room.)
        const middle = was.y + was.height / 2;
        const width = Math.max(1, Math.round(Math.min(Math.abs(wide), toRight ? whole.width - held : held, whole.height * ratio)));
        const height = Math.max(1, Math.min(whole.height, Math.round(width / ratio)));
        return {
          x: within(toRight ? held : held - width, 0, whole.width - width),
          y: within(middle - height / 2, 0, whole.height - height),
          width,
          height,
        };
      }
      const held = grip.y === 1 ? was.y : bottom;
      const tall = (grip.y === 1 ? bottom : was.y) + down - held;
      const toBottom = inwards(held, whole.height, tall === 0 ? grip.y === 1 : tall > 0);
      const middle = was.x + was.width / 2;
      const height = Math.max(1, Math.round(Math.min(Math.abs(tall), toBottom ? whole.height - held : held, whole.width / ratio)));
      const width = Math.max(1, Math.min(whole.width, Math.round(height * ratio)));
      return {
        x: within(middle - width / 2, 0, whole.width - width),
        y: within(toBottom ? held : held - height, 0, whole.height - height),
        width,
        height,
      };
    }
    let [left, top, right, bottom] = [was.x, was.y, was.x + was.width, was.y + was.height];
    if (grip === "new") [left, top, right, bottom] = [from[0], from[1], at[0], at[1]];
    else {
      if (grip.x === -1) left += across;
      if (grip.x === 1) right += across;
      if (grip.y === -1) top += down;
      if (grip.y === 1) bottom += down;
    }
    // Pulled past the opposite edge, the frame turns over rather than vanish.
    const x = within(Math.min(left, right), 0, whole.width - 1);
    const y = within(Math.min(top, bottom), 0, whole.height - 1);
    return {
      x,
      y,
      width: Math.max(1, within(Math.max(left, right), 0, whole.width) - x),
      height: Math.max(1, within(Math.max(top, bottom), 0, whole.height) - y),
    };
  };

  const onDown = (event: React.PointerEvent) => {
    if (scale === 0 || frozen) return;
    // (The field something is written in, and its band, have presses of their own.)
    if (event.target instanceof Element && event.target.closest(".holding")) return;
    // The middle button, or the main one with Space held: the picture is dragged along under the
    // pointer, whatever tool is in hand.
    if (event.button === 1 || (event.button === 0 && grabbing)) {
      (canvas.current ?? event.currentTarget).setPointerCapture(event.pointerId);
      return setGesture({ kind: "pan", from: [event.clientX, event.clientY], was: middle });
    }
    if (event.button !== 0) return;
    // (The picture is pressed on: the bar of the tool, there since its button was pressed, goes.)
    if (!shut) setShut(true);
    const at = placeOf(event);
    // Pressed beside the picture: drawn from there, as much of it as falls on the picture. Not
    // what would be put there whole, and so not be seen: something written, a number.
    const offPicture = at[0] < view.x || at[1] < view.y || at[0] > view.x + view.width || at[1] > view.y + view.height;
    if (tool === "text") {
      // A press while something is being written: that is done. Otherwise writing starts there,
      // or, on something written before, that is written anew.
      if (typing) return void finish();
      if (offPicture) return;
      let index = edits.length - 1;
      while (index >= 0 && !(edits[index].kind === "text" && isAt(edits[index], at, 0))) index -= 1;
      if (index !== -1) return rewrite(index);
      // (The middle of the first line is where the press was.)
      const { line, around } = laidOut("", letters);
      setSelected(null);
      return setTyping({ index: null, at: [Math.round(at[0] - around), Math.round(at[1] - around - line / 2)], text: "" });
    }
    // (Held by the canvas, wherever the press was: what it says of the pointer, the pointer says
    // while the drag goes on.)
    (canvas.current ?? event.currentTarget).setPointerCapture(event.pointerId);
    if (tool === "select") {
      // With Ctrl held: the one on top of those under the pointer is selected along with what is
      // selected already, or no longer if it was. Where there is none, what is selected stays, and
      // what the pointer goes on to mark out is selected along with it.
      if (event.ctrlKey) {
        const those = selected !== null ? [selected] : several;
        let index = edits.length - 1;
        while (index >= 0 && !seenAt(edits[index], at, NEAR / scale)) index -= 1;
        if (index === -1) return setGesture({ kind: "marquee", from: at, to: at, with: those });
        return setSelection(those.includes(index) ? those.filter((one) => one !== index) : [...those, index].sort((a, b) => a - b));
      }
      // On one of those selected together, or anywhere in the frame of one: they are moved, all
      // of them.
      if (several.length > 0) {
        const framed = several.some((index) => {
          const one = edits[index];
          if (!one) return false;
          const [box, around] = [boxOf(one), frameAround(one, scale)];
          return at[0] >= box.x - around && at[0] <= box.x + box.width + around && at[1] >= box.y - around && at[1] <= box.y + box.height + around;
        });
        // (As with one: what else is seen in their frames is still there to be pressed on.)
        const on = framed && (several.some((index) => edits[index] && seenAt(edits[index], at, NEAR / scale)) || !edits.some((edit) => seenAt(edit, at, NEAR / scale)));
        if (on) return setGesture({ kind: "move", indices: several, from: at, by: [0, 0] });
      }
      // On a hold of the one selected: that is pulled, to change its size.
      const held = selected !== null ? edits[selected] : undefined;
      if (held && selected !== null) {
        const box = boxOf(held);
        const on = holdAt(at);
        if (on) return setGesture({ kind: "resize", index: selected, hold: on.hold, from: at, was: held, now: held });
        // Anywhere in the frame around the one selected, where nothing else is: that is moved.
        // (Its frame is what there is to take hold of it by, once it is selected: a stroke is
        // thin, and a shape with no inside is its outline alone. But what else is seen in its
        // frame is still there to be pressed on.)
        const around = frameAround(held, scale);
        const framed = at[0] >= box.x - around && at[0] <= box.x + box.width + around && at[1] >= box.y - around && at[1] <= box.y + box.height + around;
        if (framed && (seenAt(held, at, NEAR / scale) || !edits.some((edit) => seenAt(edit, at, NEAR / scale)))) {
          return setGesture({ kind: "move", indices: [selected], from: at, by: [0, 0] });
        }
      }
      // The one on top of those under the pointer: selected, and moved if the pointer goes on.
      // Where there is none: nothing is selected any more, and what the pointer goes on to mark
      // out will be.
      let index = edits.length - 1;
      while (index >= 0 && !seenAt(edits[index], at, NEAR / scale)) index -= 1;
      setSelected(index === -1 ? null : index);
      setGesture(index !== -1 ? { kind: "move", indices: [index], from: at, by: [0, 0] } : { kind: "marquee", from: at, to: at });
      return;
    }
    if (tool === "crop") {
      return setGesture({ kind: "crop", grip: gripAt(at), from: at, was: frame ?? whole });
    }
    // On the shape that was just drawn, with its tool still in hand: that is moved, to be put
    // right, without the tool being put down for it. (A shape: a stroke of the pen is begun on
    // the one before it all the time. And only where a press means it: inside a shape with no
    // inside, the next one is drawn.)
    if (takesLast(at)) return setGesture({ kind: "move", indices: [edits.length - 1], from: at, by: [0, 0] });
    setSelected(null);
    const u = unit(capture);
    if (tool === "stamp") {
      if (offPicture) return;
      // The next number: one more than the highest there is.
      const number = 1 + Math.max(0, ...edits.map((edit) => (edit.kind === "stamp" ? edit.number : 0)));
      return setGesture({ kind: "draw", edit: stamped(at, stamps.step * STAMPS.step * u, number, stamps.color) });
    }
    if (tool === "blur" || tool === "mosaic") {
      return setGesture({ kind: "draw", edit: { kind: tool, from: at, to: at, ...hiddenBy(tool, strengths[tool]) } as Edit });
    }
    const edit: Edit =
      tool === "pen"
        ? { kind: tool, points: [at], color: pen.color, width: pen.step * u }
        : tool === "marker"
          ? { kind: tool, points: [at], color: marker.color, width: marker.step * u }
          : tool === "rectangle" || tool === "ellipse"
            ? { kind: tool, from: at, to: at, color: shapes.color, width: shapes.step * u, fill: shapes.fill, fillColor: shapes.fillColor }
            : { kind: tool, from: at, to: at, color: shapes.color, width: shapes.step * u };
    setGesture({ kind: "draw", edit });
  };

  /** What of the frame for cutting down is at a place. On an edge or a corner of it: that is
      pulled. Inside it: the frame is moved. Outside: a new frame is drawn. */
  const gripAt = (at: Place): Grip => {
    const was = frame ?? whole;
    const near = (NEAR * 1.5) / scale;
    const [right, bottom] = [was.x + was.width, was.y + was.height];
    const across = at[0] >= was.x - near && at[0] <= right + near;
    const along = at[1] >= was.y - near && at[1] <= bottom + near;
    const x = along && Math.abs(at[0] - was.x) <= near ? -1 : along && Math.abs(at[0] - right) <= near ? 1 : 0;
    const y = across && Math.abs(at[1] - was.y) <= near ? -1 : across && Math.abs(at[1] - bottom) <= near ? 1 : 0;
    const inside = at[0] > was.x && at[0] < right && at[1] > was.y && at[1] < bottom;
    return x !== 0 || y !== 0 ? { x, y } : inside ? "move" : "new";
  };
  /** The pointer that says what a press there would do to the frame: the arrows of the way an edge
      or a corner is pulled, the four arrows of moving it, the cross of drawing a new one. */
  /** The hold of the one selected that a press at a place takes: the nearest of them, when the
      press is on one. Not on a thing so small that its holds lie on it: pressed on, that is taken
      hold of whole, to be moved. */
  const holdAt = (at: Place) => {
    const held = selected !== null ? edits[selected] : undefined;
    if (!held) return undefined;
    const box = boxOf(held);
    if (Math.max(box.width, box.height) * scale < 24 && isAt(held, at, 0)) return undefined;
    const far = (one: { at: Place }) => Math.hypot(one.at[0] - at[0], one.at[1] - at[1]);
    return holdsOf(held, scale)
      .filter((one) => far(one) <= NEAR / scale)
      .sort((a, b) => far(a) - far(b))[0];
  };
  const pointerOf = (grip: Grip) =>
    grip === "move"
      ? "move"
      : grip === "new"
        ? "crosshair"
        : grip.x !== 0 && grip.y !== 0
          ? grip.x === grip.y
            ? "nwse-resize"
            : "nesw-resize"
          : grip.x !== 0
            ? "ew-resize"
            : "ns-resize";
  // The pointer over the picture while the image is cut down: what is under it, or what is in hand.
  const [pointing, setPointing] = useState<string | null>(null);
  /** Whether a press at a place takes hold of the shape that was just drawn, its tool still in
      hand. */
  const takesLast = (at: Place) => {
    const last = edits[edits.length - 1];
    return (
      selected === null &&
      justDrawn.current &&
      last !== undefined &&
      last.kind === tool &&
      (last.kind === "rectangle" || last.kind === "ellipse" || last.kind === "line" || last.kind === "arrow") &&
      seenAt(last, at, NEAR / scale) &&
      // (Not at an end of a line: the next line is begun there, as often as not.)
      !((last.kind === "line" || last.kind === "arrow") && [last.from, last.to].some((end) => Math.hypot(at[0] - end[0], at[1] - end[1]) <= (NEAR * 2) / scale))
    );
  };
  // Whether the pointer is where a press would do that: it says so, by being the pointer of
  // moving something.
  const [taking, setTaking] = useState(false);
  const onMove = (event: React.PointerEvent) => {
    const gesture = hand.current;
    if (!gesture && (tool === "rectangle" || tool === "ellipse" || tool === "line" || tool === "arrow")) {
      const next = takesLast(placeOf(event));
      if (next !== taking) setTaking(next);
    } else if (!gesture && taking) setTaking(false);
    if (!gesture && tool === "crop") {
      const next = pointerOf(gripAt(placeOf(event)));
      if (next !== pointing) setPointing(next);
    }
    // Over a hold of the one selected: the pointer of the way it is pulled there (of an end of a
    // line, which goes any way, that of moving).
    if (!gesture && tool === "select") {
      const on = holdAt(placeOf(event));
      const next = !on ? null : typeof on.hold === "string" ? "move" : pointerOf(on.hold);
      if (next !== pointing) setPointing(next);
    }
    if (!gesture) return;
    if (gesture.kind === "pan") {
      // The picture goes along under the pointer, as far as its middle can go. (Held there: back
      // from past that, it comes along at once, not once the pointer is back where it stopped.)
      const [x, y] = [gesture.was[0] - (event.clientX - gesture.from[0]) / scale, gesture.was[1] - (event.clientY - gesture.from[1]) / scale];
      const [left, top] = [Math.min(view.x + view.width, Math.max(view.x, x)), Math.min(view.y + view.height, Math.max(view.y, y))];
      if (left !== x || top !== y) setGesture({ ...gesture, was: [left, top], from: [event.clientX, event.clientY] });
      return setLooking({ times, x: left, y: top });
    }
    const at = placeOf(event);
    if (gesture.kind === "move") {
      return setGesture({ ...gesture, by: [at[0] - gesture.from[0], at[1] - gesture.from[1]] });
    }
    if (gesture.kind === "resize") {
      const by: Place = [at[0] - gesture.from[0], at[1] - gesture.from[1]];
      return setGesture({ ...gesture, now: resized(gesture.was, gesture.hold, by, event.shiftKey, unit(capture)) });
    }
    if (gesture.kind === "crop") return setFrame(pulled(gesture.grip, gesture.was, gesture.from, at));
    if (gesture.kind === "marquee") return setGesture({ ...gesture, to: at });
    const { edit } = gesture;
    if (edit.kind === "stamp") {
      // The number goes along with the pointer until it is let go.
      return setGesture({ kind: "draw", edit: stamped(at, boxOf(edit).width, edit.number, edit.color) });
    }
    if ("points" in edit) {
      // Not every twitch of the pointer: a place a pixel of the screen from the last is one.
      const last = edit.points[edit.points.length - 1];
      if (Math.hypot(at[0] - last[0], at[1] - last[1]) * scale < 1) return;
      return setGesture({ kind: "draw", edit: { ...edit, points: [...edit.points, at] } });
    }
    const to = event.shiftKey ? straightened(tool, edit.from, at) : at;
    setGesture({ kind: "draw", edit: { ...edit, to: [tenth(to[0]), tenth(to[1])] } });
  };

  const onUp = () => {
    const gesture = hand.current;
    if (!gesture) return;
    setGesture(null);
    if (gesture.kind === "move") {
      // (Not for a move that leaves them where they were: what has letters goes by whole pixels.)
      const next = edits.map((edit, index) => (gesture.indices.includes(index) ? moved(edit, gesture.by) : edit));
      if (JSON.stringify(next) !== JSON.stringify(edits)) change(next);
      return;
    }
    if (gesture.kind === "marquee") {
      // What is wholly in the part marked out is selected: one as one is, more than one together.
      // (Wholly: a shape that is only crossed, or that the part lies inside of, is not meant. And
      // a part too small to have been meant is a press, which selected nothing.)
      const [left, top] = [Math.min(gesture.from[0], gesture.to[0]), Math.min(gesture.from[1], gesture.to[1])];
      const [right, bottom] = [Math.max(gesture.from[0], gesture.to[0]), Math.max(gesture.from[1], gesture.to[1])];
      if (Math.hypot(right - left, bottom - top) * scale < 4) return;
      // (Of what is seen: not what the image was cut down past, nor what is out of sight where
      // the picture is looked at closely. The pointer goes on past the edge of the picture.)
      const within = edits.flatMap((edit, index) => {
        if (edit.kind === "crop") return [];
        const box = boxOf(edit);
        return inSight(box) && box.x >= left && box.y >= top && box.x + box.width <= right && box.y + box.height <= bottom ? [index] : [];
      });
      // (Along with what was selected, where it was marked out with Ctrl held.)
      const those = [...new Set([...(gesture.with ?? []), ...within])].sort((a, b) => a - b);
      if (those.length > 0) setSelection(those);
      return;
    }
    if (gesture.kind === "resize") {
      // (Only what it was when it was taken hold of is made larger: not whatever is in its place.)
      if (edits[gesture.index] !== gesture.was || JSON.stringify(gesture.now) === JSON.stringify(gesture.was)) return;
      // What has letters gets a picture of what it is now, the corner that was not pulled staying
      // where it is.
      let now: Edit | null = gesture.now;
      if (now.kind === "stamp") now = squared(now);
      else if (now.kind === "text" && typeof gesture.hold !== "string") {
        const box = boxOf(now);
        const anew = written([box.x, box.y], now.text, howOf(now));
        const [wide, tall] = anew ? [anew.to[0] - anew.from[0], anew.to[1] - anew.from[1]] : [0, 0];
        const from: Place = [
          gesture.hold.x === 1 ? box.x : box.x + box.width - wide,
          gesture.hold.y === 1 ? box.y : box.y + box.height - tall,
        ];
        now = anew && { ...anew, from, to: [from[0] + wide, from[1] + tall] };
      }
      if (!now) return onFull();
      const next = now;
      return change(edits.map((edit, index) => (index === gesture.index ? next : edit)));
    }
    // The frame is where it was left; cutting down is done by the key or the button for it. So is
    // the part that is shown.
    if (gesture.kind === "crop" || gesture.kind === "pan") return;
    const { edit } = gesture;
    // A press without a drag is a dot of the pen; of anything else it is nothing.
    if (!("points" in edit) && edit.from[0] === edit.to[0] && edit.from[1] === edit.to[1]) return;
    // Drawn wholly beside the picture, it would never be seen: it is not kept.
    const [box, around] = [boxOf(edit), reach(edit) / 2];
    if (box.x + box.width + around < view.x || box.y + box.height + around < view.y || box.x - around > view.x + view.width || box.y - around > view.y + view.height) return;
    // (Just drawn only when it is: not what a drawing that is full did not take.)
    if (!fits([...edits, edit])) return onFull();
    change([...edits, edit]);
    justDrawn.current = true;
  };

  // The keyboard: tools by their letters, undo and redo, Delete, and Escape for "never mind" as far
  // as there is something to give up. Heard before the window's own keys: an Escape that puts down
  // what is in hand is not also the one that discards the capture.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // (A key that only goes into letters being put together is not a key to act on.)
      if (event.isComposing) return;
      // (A choice open beside the things to do with the capture has the keys: Escape puts that
      // away, and is not about the drawing. What is being written keeps its other keys.)
      if (document.querySelector(".actions .menu") !== null && (!typing || event.key === "Escape")) return;
      // (Nor is any key about the drawing while the tour of the window is on.)
      if (touring()) return;
      // (Escape held down is one Escape: what follows the first gives up nothing more.)
      if (event.repeat && event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      // On its way out, the capture is not drawn on, nor asked to leave once more.
      if (frozen) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      // (A field something is typed into: of text, or of a number. Not a slider, which has no
      // letters of its own.)
      const onText =
        (event.target instanceof HTMLInputElement && ["text", "number", "search"].includes(event.target.type)) ||
        event.target instanceof HTMLTextAreaElement;
      const key = event.key.toLowerCase();
      const taken = () => {
        event.preventDefault();
        event.stopPropagation();
      };
      // (What is typed where the fonts are looked through is that list's: it sees to its keys.)
      if (event.target instanceof HTMLElement && event.target.closest("[data-own-keys]") && !event.ctrlKey) return;
      // In a field of the box by what is written, Enter and Escape are "done here", and nothing
      // else: back to what is being written, when something is; otherwise the field is left.
      if (event.target instanceof HTMLElement && event.target.closest(".lettering") && (key === "escape" || (onText && key === "enter" && !event.ctrlKey))) {
        taken();
        if (typing) field.current?.focus({ preventScroll: true });
        else event.target.blur();
        return;
      }
      // While something is being written: Escape gives it up (what was written before stays as it
      // was), Enter is done with it, and so is Ctrl+Enter (which leaves nothing while something is
      // written). Shift+Enter is a new line, and every other key a letter.
      // (Escape in another field, where a colour is written, is that field's.)
      if (typing && key === "escape" && !(onText && event.target !== field.current)) {
        taken();
        return setTyping(null);
      }
      if (typing && key === "enter" && event.ctrlKey) {
        taken();
        return void finish();
      }
      // (Enter alone: in the field, or with the keyboard nowhere in particular. On a button it is
      // that button's, and a slider keeps its keys.)
      if (
        typing &&
        key === "enter" &&
        !event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        (event.target === field.current ||
          (!onText && !(event.target instanceof HTMLInputElement) && !(event.target instanceof HTMLElement && event.target.closest("button"))))
      ) {
        taken();
        return void finish();
      }
      // What is being written is held as a thing (by the band around its field): Delete takes it
      // away, all of it; what was written before and is written anew is gone from the drawing.
      // Any other key in its field is typed, and it is being written again. (Not a key held for
      // another key, which is no key yet.)
      if (typing && held && (event.target === field.current || (!onText && !(event.target instanceof HTMLInputElement)))) {
        const alone = !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
        if ((key === "delete" || key === "backspace") && alone && !event.isComposing) {
          taken();
          if (typing.index !== null) change(edits.filter((_, index) => index !== typing.index));
          justDrawn.current = false;
          return setTyping(null);
        }
        if (event.target === field.current && !["shift", "control", "alt", "meta", "capslock"].includes(key)) setHeld(false);
      }
      // The field lost the keyboard (a slider was moved, something beside the picture pressed)
      // while something is still being written: a key is still for what is written, not for a
      // tool, and not for uploading. The field has the keyboard again from this key on. (A slider
      // keeps its own keys.)
      if (typing && !onText && !event.ctrlKey && !event.altKey && !event.metaKey) {
        // (Tab goes where it goes, and Enter on a button is that button's.)
        const onButton = event.target instanceof HTMLElement && event.target.closest("button") !== null;
        if (key === "tab" || (onButton && (key === "enter" || key === " "))) return;
        // (A key that is held for another key is not yet a key for what is written.)
        if (["shift", "control", "alt", "meta", "capslock"].includes(key)) return;
        if (!(event.target instanceof HTMLInputElement)) field.current?.focus({ preventScroll: true });
        if (key === "enter") taken();
        return;
      }
      // With the pointer down, something is half drawn or half moved: nothing is uploaded, saved or
      // copied in that state. The keys wait for the pointer.
      // Nor is anything else done to the drawing or to how it is looked at meanwhile (what is in
      // hand is about the drawing as it was when the pointer went down). Escape gives it up.
      if (gesture && key !== "escape") return taken();
      // In a field of text, undoing and redoing are about the text.
      if (onText && event.ctrlKey) return;
      if (event.ctrlKey && !event.altKey && key === "z") {
        taken();
        return event.shiftKey ? redo() : undo();
      }
      if (event.ctrlKey && !event.altKey && key === "y") {
        taken();
        return redo();
      }
      // What is selected: copied, taken out, put back, or made once more beside itself.
      // (Not while something is being written: that is not done yet, and nothing is selected.)
      if (!typing && event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey && ["c", "x", "v", "d"].includes(key)) {
        taken();
        // (Once for each press: a key held down does not fill the picture.)
        if (event.repeat) return;
        if (key === "c") copy();
        else if (key === "x") takeOut();
        else if (key === "v") paste();
        else duplicate();
        return;
      }
      // (Escape gives up what the pointer is doing with Ctrl still held, too.)
      if ((event.ctrlKey || event.altKey || event.metaKey) && !(gesture && key === "escape")) return;
      // Space takes hold of the picture, for as long as it is held (unless it is for a field or a
      // button that has the keyboard); 0 looks at all of it again.
      const onControl = event.target instanceof HTMLElement && event.target.closest("button, input, textarea") !== null;
      if (key === " " && !onControl) {
        taken();
        return setGrabbing(true);
      }
      if (key === "0" && !onText) {
        taken();
        return setLooking({ times: 1 });
      }
      // While the image is being cut down, Enter cuts it; it is not yet time for the capture to leave.
      // (On a button that has the keyboard, Enter is that button's.)
      const onButton = event.target instanceof HTMLElement && event.target.closest("button") !== null;
      if (key === "enter" && tool === "crop" && !onText && !onButton) {
        taken();
        cut();
        return;
      }
      if (key === "escape") {
        if (gesture) setGesture(null);
        else if (panel) setPanel(null);
        else if (afloat) setAfloat(null);
        else if (selected !== null || several.length > 0) setSelected(null);
        else if (tool !== "select") hold("select");
        // A drawing is not thrown away by a key: that takes the button.
        else if (edits.length === 0) return;
        return taken();
      }
      if (onText) return;
      // (Not while a slider has the keyboard: there these keys are not about the drawing.)
      const onInput = event.target instanceof HTMLInputElement;
      if ((key === "delete" || key === "backspace") && (selected !== null || several.length > 0) && !onInput) {
        taken();
        return remove();
      }
      const picked = TOOLS.find((entry) => entry.key.toLowerCase() === key);
      if (picked && !event.repeat) {
        taken();
        if (picked.tool === tool && !chosen) return;
        // A key takes the tool in hand and nothing else: no panel opens for it.
        hold(picked.tool);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  // The wheel, with the other button of the mouse held down: the tools, one after another, in the
  // order of their bar, as far as its first and its last. Of the shapes and of the two ways to
  // make a part unreadable, the one that was in hand last. While that is done, the tool in hand is
  // shown by the pointer, up and to the right of it; and a moment longer once the button is let
  // go, going along with the pointer for as long as it is shown.
  // (It comes up into its place, and goes back down out of it: `going` while it does that.)
  const [turned, setTurned] = useState<{ x: number; y: number; going?: boolean } | null>(null);
  const turning = useRef({ by: 0, gone: 0, down: false, last: { at: 0, x: 0, y: 0 } });
  useEffect(() => {
    const round: Tool[] = ["select", "pen", "marker", figure, "text", "stamp", hider, "crop"];
    // (Whether that button is down: as the wheel says, or as its own press said.)
    // (Whether that button is down is what each event of the pointer says of it: one button
    // pressed or let go while another is held is said by a move, not by a press or a letting go.)
    const pressed = (event: PointerEvent) => {
      turning.current.down = (event.buttons & 2) !== 0;
      if (touring()) return;
      if (event.button !== 2) return;
      // Twice in the same place, over the picture: the settings of what is in hand, there.
      const last = turning.current.last;
      const twice = event.timeStamp - last.at < 450 && Math.hypot(event.clientX - last.x, event.clientY - last.y) < 8;
      turning.current.last = { at: twice ? 0 : event.timeStamp, x: event.clientX, y: event.clientY };
      const over = event.target instanceof Element && event.target.closest(".picture.stage") !== null;
      if (!twice || !over || frozen || hand.current || typing) return;
      if (offered !== null || ((tool === "pen" || tool === "marker") && !chosen)) {
        setPanel(null);
        setAfloat({ x: event.clientX, y: event.clientY });
      }
    };
    const onTurn = (event: WheelEvent) => {
      if (!(event.buttons & 2) && !turning.current.down) return;
      if (touring()) return;
      // (The wheel is not for looking closer meanwhile, nor for what is under the pointer.)
      event.preventDefault();
      event.stopPropagation();
      if (frozen || hand.current) return;
      window.clearTimeout(turning.current.gone);
      setTurned({ x: event.clientX, y: event.clientY });
      // A notch of the wheel is a tool; a wheel that turns smoothly, as far as a notch.
      turning.current.by += event.deltaY;
      if (Math.abs(turning.current.by) < 50) return;
      const step = turning.current.by > 0 ? 1 : -1;
      turning.current.by = 0;
      const at = round.indexOf(isFigure(tool) ? figure : isHider(tool) ? hider : tool);
      // (A press of that button right after this is the wheel again, not one of two presses.)
      turning.current.last.at = 0;
      const next = round[Math.max(0, at) + step];
      if (next === undefined || next === tool) return;
      hold(next);
    };
    // (While the tour of the window is on, the tool shown by the pointer is the tour's: where the
    // mouse really is, and what is pressed and let go there, have nothing to do with it.)
    const along = (event: PointerEvent) => {
      if (touring()) return;
      const was = turning.current.down;
      turning.current.down = (event.buttons & 2) !== 0;
      if (was && !turning.current.down) done(event);
      setTurned((now) => now && { ...now, x: event.clientX, y: event.clientY });
    };
    const done = (event: PointerEvent | FocusEvent) => {
      if (touring()) return;
      turning.current.down = "buttons" in event && event.type !== "pointercancel" && (event.buttons & 2) !== 0;
      if (turning.current.down) return;
      turning.current.by = 0;
      window.clearTimeout(turning.current.gone);
      turning.current.gone = window.setTimeout(() => {
        setTurned((now) => now && { ...now, going: true });
        turning.current.gone = window.setTimeout(() => setTurned(null), GOING);
      }, 450);
    };
    window.addEventListener("pointerdown", pressed, true);
    window.addEventListener("wheel", onTurn, { capture: true, passive: false });
    window.addEventListener("pointermove", along, true);
    window.addEventListener("pointerup", done, true);
    window.addEventListener("pointercancel", done, true);
    window.addEventListener("blur", done);
    return () => {
      window.removeEventListener("pointercancel", done, true);
      window.removeEventListener("pointerdown", pressed, true);
      window.removeEventListener("wheel", onTurn, true);
      window.removeEventListener("pointermove", along, true);
      window.removeEventListener("pointerup", done, true);
      window.removeEventListener("blur", done);
    };
  });
  useEffect(() => () => window.clearTimeout(turning.current.gone), []);

  // Space let go, or the window left with it held: the pointer is the tool's again.
  useEffect(() => {
    if (!grabbing) return;
    const letGo = (event: KeyboardEvent | FocusEvent) => {
      if (!("key" in event) || event.key === " ") setGrabbing(false);
    };
    window.addEventListener("keyup", letGo);
    window.addEventListener("blur", letGo);
    return () => {
      window.removeEventListener("keyup", letGo);
      window.removeEventListener("blur", letGo);
    };
  }, [grabbing]);

  // What the tour of the window has it do, done as the mouse does it (see `EditorShow`). Made anew
  // with each drawing; what the tour holds is a handle that stays this window's (and stops being the
  // one shown, when the window is made anew for another capture).
  const showNow = useRef<EditorShow | null>(null);
  const stopped = () => frozen || hand.current !== null;
  // (Put back by the tour and not yet drawn so: what the window is about to be, for the next step
  // of the tour, which starts before it is drawn again.)
  const puttingBack = useRef<ReturnType<EditorShow["now"]> | null>(null);
  useLayoutEffect(() => {
    puttingBack.current = null;
  });
  /** A place on the picture as the tour names it, in pixels of the image: of what is shown of it. */
  const partOf = ([across, down]: Part, of: Box = view): Place => [tenth(of.x + across * of.width), tenth(of.y + down * of.height)];
  showNow.current = {
    now: () =>
      puttingBack.current ?? {
        tool,
        looking,
        drawing,
        selected,
        several,
        figure,
        hider,
        shut,
        justDrawn: justDrawn.current,
        busy: stopped() || typing !== null || tool === "crop",
      },
    sketch: (index, what) => {
      const u = unit(capture);
      const made: Edit | null =
        what.tool === "pen" || what.tool === "marker"
          ? { kind: what.tool, points: what.points.map((one) => partOf(one)), color: (what.tool === "pen" ? pen : marker).color, width: (what.tool === "pen" ? pen : marker).step * u }
          : what.tool === "rectangle"
            ? { kind: "rectangle", from: partOf(what.from), to: partOf(what.to), color: shapes.color, width: shapes.step * u, fill: shapes.fill, fillColor: shapes.fillColor }
            : what.tool === "mosaic"
              ? ({ kind: "mosaic", from: partOf(what.from), to: partOf(what.to), ...hiddenBy("mosaic", strengths.mosaic) } as Edit)
              : what.tool === "text"
                ? written(partOf(what.at), what.text, letters)
                : stamped(partOf(what.at), stamps.step * STAMPS.step * u, what.number, stamps.color);
      if (!made) return index ?? -1;
      const was = latest.current;
      const next = index === null || index >= was.length ? [...was, made] : was.map((one, at) => (at === index ? made : one));
      latest.current = next;
      setDrawing((now) => ({ ...now, edits: next }));
      onCount(next.length);
      return index === null || index >= was.length ? next.length - 1 : index;
    },
    select: (index) => {
      setSeveral([]);
      setSelected(index);
    },
    // (Taken right after the window was put back: against what it is about to be, not what it was drawn as.)
    whole: () => setLooking({ times: 1 }),
    pageOf: (part) => {
      const box = canvas.current?.getBoundingClientRect();
      if (!box || box.width === 0 || fit === 0) return [0, 0];
      const [[x, y], all, by] = [partOf(part), lookAt(1).part, (fit * box.width) / room.width];
      return [Math.round(box.left + (x - all.x) * by), Math.round(box.top + (y - all.y) * by)];
    },
    frame: (from, to) => {
      const [one, other] = [partOf(from, whole), partOf(to, whole)];
      setFrame({ x: Math.round(one[0]), y: Math.round(one[1]), width: Math.round(other[0] - one[0]), height: Math.round(other[1] - one[1]) });
    },
    turn: (x, y, by) => {
      if (stopped()) return;
      // (Not as far as cutting the picture down: that would set a frame, and look at all of it.)
      const round: Tool[] = ["select", "pen", "marker", figure, "text", "stamp", hider];
      const at = round.indexOf(isFigure(tool) ? figure : isHider(tool) ? hider : tool);
      const next = round[Math.max(0, at) + by];
      window.clearTimeout(turning.current.gone);
      setTurned({ x, y });
      if (next !== undefined && next !== tool) hold(next);
    },
    take: (one) => {
      // (Taken right after the tour put the window back: against what it is about to be, not what it
      // was last drawn as.)
      if (!stopped() && one !== (puttingBack.current?.tool ?? tool)) hold(one);
    },
    settingsAt: (x, y) => {
      if (stopped()) return;
      setPanel(null);
      setAfloat({ x, y });
    },
    shut: () => {
      setPanel(null);
      setAfloat(null);
    },
    zoom: zoomAt,
    restore: (was) => {
      puttingBack.current = { ...was, busy: false };
      window.clearTimeout(turning.current.gone);
      setTurned(null);
      setAfloat(null);
      setPanel(null);
      // (Whatever tool the window is about to have: taken again, so that what taking it set is set.)
      hold(was.tool);
      setFigure(was.figure);
      setHider(was.hider);
      setSelected(was.selected);
      setSeveral(was.several);
      // (A bar that was open, open again: as if its tool had been taken by its button.)
      setShut(was.shut);
      byButton.current = was.shut ? null : was.tool;
      justDrawn.current = was.justDrawn;
      // (Leaving the cutting down shows what the picture is cut to again: the look is this one, not
      // all of that.)
      if (tool === "crop" && was.tool !== "crop" && keptOf(was.drawing.edits, whole)) lookKept.current = true;
      setLooking(was.looking);
      latest.current = was.drawing.edits;
      onCount(was.drawing.edits.length);
      setDrawing(was.drawing);
    },
  };
  useLayoutEffect(() => {
    // (Once the window is gone, made anew for another capture, what is asked of it is done nowhere.)
    let gone = false;
    const now = () => showNow.current as EditorShow;
    const handle: EditorShow = {
      now: () => now().now(),
      sketch: (index, what) => (gone ? -1 : now().sketch(index, what)),
      select: (index) => void (gone || now().select(index)),
      frame: (from, to) => void (gone || now().frame(from, to)),
      whole: () => void (gone || now().whole()),
      pageOf: (part) => (gone ? [0, 0] : now().pageOf(part)),
      turn: (x, y, by) => void (gone || now().turn(x, y, by)),
      take: (one) => void (gone || now().take(one)),
      settingsAt: (x, y) => void (gone || now().settingsAt(x, y)),
      shut: () => void (gone || now().shut()),
      zoom: (x, y, deltaY) => void (gone || now().zoom(x, y, deltaY)),
      restore: (was) => void (gone || now().restore(was)),
    };
    show.current = handle;
    return () => {
      gone = true;
      if (show.current === handle) show.current = null;
    };
  }, []);

  const ratioNamed = (name: string) => (name === "free" ? t("editor.crop.free") : name);
  const named = (one: Tool) => `${t(`editor.tool.${one}`)} (${TOOLS.find((entry) => entry.tool === one)?.key})`;
  /** A tool of the first bar. With a panel of its own it shows, in hand, that pressing it again
      opens one. */
  // What the buttons show of what their tools would draw with.
  const looks: Partial<Record<Tool, Look>> = {
    pen: { colour: pen.color },
    marker: { colour: marker.color },
    rectangle: { colour: shapes.color, inside: { colour: shapes.fillColor, solid: shapes.fill } },
    ellipse: { colour: shapes.color, inside: { colour: shapes.fillColor, solid: shapes.fill } },
    line: { colour: shapes.color },
    arrow: { colour: shapes.color },
    text: { colour: letters.fill > 0 ? letters.color : (letters.outline ?? letters.color) },
    stamp: { colour: stamps.color },
  };
  // A colour chosen from a panel under the bars is what the panel was opened for: it is put away.
  // (Not one mixed: that is on its way to another.)
  const shutPanel = () => {
    setPanel(null);
    if (offered === null) setAfloat(null);
  };
  /** The colour and the size of what is written, or of the numbers: a button of the second bar and
      the panel under it. */
  const lettering = (
    name: "letters" | "stamps",
    now: { color: string; step: number },
    limits: { least: number; most: number },
    set: (what: { color?: string; step?: number }, dragged?: boolean) => void,
  ) => (
    <div className="anchor">
      {opener(name, t("editor.stroke"), now.color)}
      <Lingering>
        {panel === name && (
          <Panel label={t("editor.stroke")}>
            <Colours colours={PALETTE} value={now.color} onPick={(one, dragged) => one !== null && set({ color: one }, dragged)} onChosen={shutPanel} t={t} />
            <label className="setting">
              <span className="heading">{t("editor.size")}</span>
              <span className="slider">
                <input
                  type="range"
                  min={limits.least}
                  max={limits.most}
                  value={Math.min(limits.most, Math.max(limits.least, now.step))}
                  aria-label={t("editor.size")}
                  onChange={(event) => set({ step: Number(event.target.value) }, true)}
                />
                <output>{now.step}</output>
              </span>
            </label>
          </Panel>
        )}
      </Lingering>
    </div>
  );
  // The field something is written in, over the picture: where it will be, as large as it will be.
  const field = useRef<HTMLTextAreaElement>(null);
  // The field being dragged along by the band around it: from where on the screen, and where what
  // is being written was.
  const carrying = useRef<{ from: Place; was: Place; far: boolean } | null>(null);
  // Whether what is being written was pressed on that band (pressed, not carried along by it),
  // and is held as a thing since: Delete takes away all of it then. A press in the field, or a
  // key that is typed, and it is being written again.
  // (The field keeps the keyboard meanwhile, though it shows no caret: a field without it is not
  // typed into in every language at the first key. Where letters are put together from several
  // keys, the first of them would be a letter of its own.)
  const [held, setHeld] = useState(false);
  const heldNow = useRef(held);
  heldNow.current = held;
  useEffect(() => {
    // (Carried along while it is held as a thing, it stays held so.)
    if (!heldNow.current) field.current?.focus({ preventScroll: true });
  }, [typing?.index, typing?.at[0], typing?.at[1]]);
  useEffect(() => {
    if (!typing) setHeld(false);
  }, [typing === null, typing?.index]);
  // (Done with or given up while the band was held: nothing is carried along any more.)
  useEffect(() => {
    if (!typing) carrying.current = null;
  }, [typing === null]);
  const perPixel = look.width > 0 ? shownSize.width / look.width : 0;
  const writing = typing && laidOut(typing.text, letters);
  // Where the settings of what is written are shown: by what is being written, or by what is
  // selected of what was written; in pixels of the screen, from the corner of the picture. Not
  // while that is being moved or pulled.
  const letteredOf =
    perPixel === 0 || frozen || gesture
      ? null
      : typing && writing
        ? { x: typing.at[0], y: typing.at[1], width: writing.width, height: writing.height }
        : chosenWritten
          ? boxOf(chosenWritten)
          : null;
  // (Nor by what is out of sight, where the picture is looked at so closely that it is, or past
  // where it was cut down to.)
  const lettered = letteredOf && inSight(letteredOf) ? letteredOf : null;
  // (By the letters and the room they have without a line around them: the settings stay where
  // they are while such a line is given to the letters, taken from them, or made wider.)
  const reached = reachOf(lettersNow);
  const letteredAt = lettered && {
    x: (lettered.x + reached - look.x) * perPixel,
    y: (lettered.y + reached - look.y) * perPixel,
    width: Math.max(0, lettered.width - reached * 2) * perPixel,
    height: Math.max(0, lettered.height - reached * 2) * perPixel,
  };
  const toolButton = (one: Tool, opens = false) => (
    <button
      role="radio"
      aria-checked={tool === one}
      aria-haspopup={opens ? "dialog" : undefined}
      aria-expanded={opens ? panel === one : undefined}
      className={tool === one ? "current" : undefined}
      title={named(one)}
      aria-label={t(`editor.tool.${one}`)}
      data-tour={`tool-${one}`}
      onClick={() => pick(one)}
    >
      <ToolIcon tool={one} look={looks[one]} />
      {opens && tool === one && <Caret />}
    </button>
  );
  /** A button of the second bar that opens a panel: what it sets, and the colour that is set. */
  const opener = (name: PanelName, label: string, colour: string | null) => (
    <button
      className="choice"
      aria-haspopup="dialog"
      aria-expanded={panel === name}
      onClick={() => setPanel(panel === name ? null : name)}
    >
      <span className={colour === null ? "swatch none" : "swatch"} style={{ "--swatch": colour ?? "transparent" } as React.CSSProperties} />
      <span>{label}</span>
      <Caret />
    </button>
  );
  const strokePanel = (kind: "pen" | "marker", now: { color: string; step: number }) => (
    <Panel label={t(`editor.tool.${kind}`)}>
      <Colours
        colours={kind === "pen" ? PALETTE : MARKERS}
        value={now.color}
        onPick={(one, dragged) => one !== null && setStroke(kind, { color: one }, dragged)}
        onChosen={shutPanel}
        t={t}
      />
      <Size kind={kind} colour={now.color} step={now.step} onSet={(to) => setStroke(kind, { step: to }, true)} t={t} />
    </Panel>
  );

  // What the pointer looks like over the picture and the room around it (and, held by the canvas,
  // while a drag goes on): while cutting down, the pointer of what is in hand, for as long as it is,
  // wherever the pointer has gone meanwhile; otherwise of what is under it.
  const pointer: React.CSSProperties | undefined =
    tool === "crop" && !grabbing
      ? { cursor: gesture?.kind === "crop" ? pointerOf(gesture.grip) : gesture ? undefined : (pointing ?? undefined) }
      : // (Selecting: over a hold of the one selected, and while it is pulled by one.)
        tool === "select" && !grabbing && (gesture ? gesture.kind === "resize" : pointing !== null)
        ? { cursor: gesture?.kind === "resize" ? (typeof gesture.hold === "string" ? "move" : pointerOf(gesture.hold)) : (pointing ?? undefined) }
        : // (And over the shape that was just drawn, where a press moves it; and while it is moved.)
        !grabbing && tool !== "select" && (gesture ? gesture.kind === "move" : taking)
        ? { cursor: "move" }
        : undefined;

  return (
    <>
      <div
        className={frozen ? "tools frozen" : "tools"}
        role="toolbar"
        aria-label={t("editor.tools")}
        // A press on a tool does not take the keyboard from the picture: Enter still means what it meant, and
        // the letters still pick tools. (The sliders do take it, to be moved by the arrow keys.)
        onMouseDown={(event) => {
          if (!(event.target instanceof HTMLInputElement)) event.preventDefault();
        }}
      >
        <div className="bar" data-tour="tools">
          <div className="group" role="radiogroup">
            {toolButton("select")}
            <div className="anchor">
              {toolButton("pen", true)}
              <Lingering>{panel === "pen" && strokePanel("pen", pen)}</Lingering>
            </div>
            <div className="anchor">
              {toolButton("marker", true)}
              <Lingering>{panel === "marker" && strokePanel("marker", marker)}</Lingering>
            </div>
            <button
              role="radio"
              aria-checked={isFigure(tool)}
              className={isFigure(tool) ? "current" : undefined}
              title={t("editor.tool.shapes")}
              aria-label={t("editor.tool.shapes")}
              data-tour="tool-shapes"
              onClick={() => pick(isFigure(tool) ? tool : figure)}
            >
              <ShapesIcon look={looks.rectangle ?? {}} />
            </button>
            {toolButton("text")}
            {toolButton("stamp")}
            <button
              role="radio"
              aria-checked={isHider(tool)}
              className={isHider(tool) ? "current" : undefined}
              title={t("editor.tool.hide")}
              aria-label={t("editor.tool.hide")}
              data-tour="tool-hide"
              onClick={() => pick(isHider(tool) ? tool : hider)}
            >
              <ToolIcon tool={isHider(tool) ? tool : hider} />
            </button>
            {toolButton("crop")}
          </div>
          <span className="rule" />
          <div className="group">
            <button title={`${t("editor.undo")} (Ctrl+Z)`} aria-label={t("editor.undo")} disabled={drawing.past.length === 0 || typing !== null} onClick={undo}>
              <span className="icon" aria-hidden="true">
                {"\uE7A7"}
              </span>
            </button>
            <button title={`${t("editor.redo")} (Ctrl+Y)`} aria-label={t("editor.redo")} disabled={drawing.future.length === 0 || typing !== null} onClick={redo}>
              <span className="icon" aria-hidden="true">
                {"\uE7A6"}
              </span>
            </button>
            <button title={`${t("editor.delete")} (Delete)`} aria-label={t("editor.delete")} disabled={selected === null && several.length === 0} onClick={remove}>
              <span className="icon" aria-hidden="true">
                {"\uE74D"}
              </span>
            </button>
          </div>
        </div>
        {/* The panel of the pen or the marker, where the pointer was when it was asked for. */}
        <Lingering>
          {afloat && offered === null && (tool === "pen" || tool === "marker") && !chosen && (
            <div key={`${afloat.x} ${afloat.y}`} className="anchor afloat" style={{ left: afloat.x, top: afloat.y }}>
              {strokePanel(tool, tool === "pen" ? pen : marker)}
            </div>
          )}
        </Lingering>
        <Lingering>{second && (
          // (Another bar, not this one changed: the one before goes and this one comes down.)
          // Where the pointer was, when it was asked for there: in the window, whatever it is by.
          <div
            key={afloat ? `${second} ${afloat.x} ${afloat.y}` : second}
            ref={afloat ? inWindow : undefined}
            className={afloat ? "bar second afloat" : "bar second"}
            style={afloat ? { left: afloat.x, top: afloat.y + 12 } : undefined}
          >
            {second === "figures" && (
              <>
                <div className="group" role="radiogroup">
                  {FIGURES.map((one) => (
                    <button
                      key={one}
                      role="radio"
                      aria-checked={tool === one}
                      className={tool === one ? "current" : undefined}
                      title={named(one)}
                      aria-label={t(`editor.tool.${one}`)}
                      onClick={() => pick(one)}
                    >
                      <ToolIcon tool={one} look={looks[one]} />
                    </button>
                  ))}
                </div>
                <span className="rule" />
              </>
            )}
            {second === "hiding" && (
              <>
                <div className="group" role="radiogroup">
                  {HIDERS.map((one) => (
                    <button
                      key={one}
                      role="radio"
                      aria-checked={tool === one}
                      className={tool === one ? "current worded" : "worded"}
                      title={named(one)}
                      onClick={() => pick(one)}
                    >
                      <ToolIcon tool={one} />
                      <span>{t(`editor.tool.${one}`)}</span>
                    </button>
                  ))}
                </div>
                <span className="rule" />
              </>
            )}
            {(second === "hiding" || second === "hidden") && (
              <label className="slider strength">
                <span>{t("editor.strength")}</span>
                <input
                  type="range"
                  min={STRENGTHS.least}
                  max={STRENGTHS.most}
                  value={Math.min(STRENGTHS.most, Math.max(STRENGTHS.least, strengthNow))}
                  onChange={(event) => setHiding(Number(event.target.value))}
                />
                <output>{strengthNow}</output>
              </label>
            )}
            {second === "stamps" && lettering("stamps", stampsNow, STAMPS, setStamping)}
            {second === "crop" && (
              <>
                <div className="anchor">
                  <button
                    className="choice"
                    title={t("editor.crop.ratio")}
                    aria-haspopup="dialog"
                    aria-expanded={panel === "ratio"}
                    onClick={() => setPanel(panel === "ratio" ? null : "ratio")}
                  >
                    <span>{ratioNamed(RATIOS.find((one) => one.ratio === ratio)?.name ?? "free")}</span>
                    <Caret />
                  </button>
                  <Lingering>{panel === "ratio" && (
                    <Panel label={t("editor.crop.ratio")} narrow>
                      <div className="list" role="radiogroup" aria-label={t("editor.crop.ratio")}>
                        {RATIOS.map((one) => (
                          <button
                            key={one.name}
                            role="radio"
                            aria-checked={one.ratio === ratio}
                            className={one.ratio === ratio ? "current" : undefined}
                            onClick={() => {
                              // The frame as it is, held to the shape: the largest of it inside.
                              // (Of that shape already, it is left as it is, to the pixel.)
                              if (one.ratio !== null && one.ratio !== ratio) setFrame(fitted(frame ?? whole, one.ratio));
                              setRatio(one.ratio);
                              setPanel(null);
                            }}
                          >
                            {ratioNamed(one.name)}
                          </button>
                        ))}
                      </div>
                    </Panel>
                  )}</Lingering>
                </div>
                <span className="rule" />
                <output className="size">
                  {(frame ?? whole).width} × {(frame ?? whole).height}
                </output>
                <span className="rule" />
                {/* Done, all of it again, never mind: each its icon alone, named in its tooltip. */}
                <button className="accent" title={`${t("editor.crop.apply")} (Enter)`} aria-label={t("editor.crop.apply")} onClick={() => cut()}>
                  <span className="icon" aria-hidden="true">
                    {"\uE73E"}
                  </span>
                </button>
                <button
                  title={t("editor.crop.whole")}
                  aria-label={t("editor.crop.whole")}
                  onClick={() => {
                    // All of it is of the image's own shape: the frame is held to no other.
                    setRatio(null);
                    setFrame(whole);
                  }}
                >
                  <span className="icon" aria-hidden="true">
                    {"\uE72C"}
                  </span>
                </button>
                <button title={`${t("editor.crop.cancel")} (Esc)`} aria-label={t("editor.crop.cancel")} onClick={() => hold("select")}>
                  <span className="icon" aria-hidden="true">
                    {"\uE711"}
                  </span>
                </button>
              </>
            )}
            {second === "stroke" && chosenStroke && (
              <div className="anchor">
                {opener("stroke", t("editor.stroke"), chosenStroke.color)}
                <Lingering>
                  {panel === "stroke" &&
                    strokePanel(chosenStroke.kind, { color: chosenStroke.color, step: stepOf(chosenStroke) })}
                </Lingering>
              </div>
            )}
            {(second === "figures" || second === "shape" || second === "line") && (
              <>
                <div className={fills ? "anchor" : "anchor off"}>
                  {opener("fill", t("editor.fill"), fills && inside.fill > 0 ? inside.fillColor : null)}
                  <Lingering>{panel === "fill" && (
                    <Panel label={t("editor.fill")}>
                      <Colours
                        none
                        // "None" takes the place of the darkest grey: the rows stay six wide.
                        colours={PALETTE.filter((_, index) => index !== 5)}
                        value={inside.fill > 0 ? inside.fillColor : null}
                        fallback={inside.fillColor}
                        onPick={(one, dragged) => setInside(one === null ? { fill: 0 } : { fillColor: one }, dragged)}
                        onChosen={shutPanel}
                        t={t}
                      />
                      <label className={inside.fill > 0 ? "setting" : "setting off"}>
                        <span className="heading">{t("editor.opacity")}</span>
                        <span className="slider">
                          <input
                            type="range"
                            min={5}
                            max={100}
                            step={5}
                            disabled={inside.fill === 0}
                            value={Math.round((inside.fill || solid.current) * 100)}
                            onChange={(event) => setInside({ fill: Number(event.target.value) / 100 }, true)}
                          />
                          <output>{Math.round(inside.fill * 100)}%</output>
                        </span>
                      </label>
                    </Panel>
                  )}</Lingering>
                </div>
                <div className="anchor">
                  {opener("outline", t("editor.outline"), outline.color)}
                  <Lingering>{panel === "outline" && (
                    <Panel label={t("editor.outline")}>
                      <Colours colours={PALETTE} value={outline.color} onPick={(one, dragged) => one !== null && setOutline({ color: one }, dragged)} onChosen={shutPanel} t={t} />
                      <Size kind="line" colour={outline.color} step={outline.step} onSet={(to) => setOutline({ step: to }, true)} t={t} />
                    </Panel>
                  )}</Lingering>
                </div>
              </>
            )}
          </div>
        )}</Lingering>
      </div>
      <div
        ref={stage}
        className={`picture stage tool-${tool}${grabbing || gesture?.kind === "pan" ? " grab" : ""}${frozen ? " frozen" : ""}`}
        style={pointer}
        data-tour="stage"
        // The picture and the room around it alike: a drag may begin beside the picture.
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={() => setGesture(null)}
        // (The pointer taken away without being let go here: nothing is in hand any more.)
        onLostPointerCapture={() => hand.current && setGesture(null)}
        onWheel={onWheel}
        // (The middle button, pressed, would otherwise start the browser's own scrolling. And a
        // press that starts something written is not to take the keyboard from its field.)
        onMouseDown={(event) => (event.button === 1 || tool === "text") && !(event.target instanceof Element && event.target.closest(".holding")) && event.preventDefault()}
        // Something written, pressed twice with nothing in hand: written anew. (Not with Ctrl held:
        // those presses were about what is selected, wherever they were. Heard here, where the
        // pointer is held while pressed, as the presses are.)
        onDoubleClick={(event) => {
          if (event.ctrlKey || tool !== "select" || selected === null || edits[selected]?.kind !== "text") return;
          event.preventDefault();
          const index = selected;
          hold("text");
          rewrite(index);
        }}
      >
        <div
          className={`sheet tool-${tool}${frozen ? " frozen" : ""}${grabbing || gesture?.kind === "pan" ? " grab" : ""}`}
          style={shownSize}
        >
          {/* The picture is drawn on the canvas. Until it is there to draw, this shows it; and should
              it never come, this is what is seen (not once the image is looked at more closely or
              cut down: it would no longer be in its place; and never with a part of it made
              unreadable, which this would show as it was). */}
          {!bitmap && view === whole && !edits.some(isHidden) && (
            <img
              style={{
                left: (view.x - look.x) * scale,
                top: (view.y - look.y) * scale,
                width: view.width * scale,
                height: view.height * scale,
              }}
              src={imageUrl(capture, "image")}
              alt={t("preview.image", { width: capture.width, height: capture.height })}
              draggable={false}
            />
          )}
          <canvas
            ref={canvas}
            // Where the picture is on it, in pixels of the page: the canvas is the whole place, and the
            // picture somewhere in it (for whoever has to find it, as the checks do).
            data-picture={[(view.x - look.x) * scale, (view.y - look.y) * scale, view.width * scale, view.height * scale].map((n) => Math.round(n * 100) / 100).join(",")}
            style={pointer}
          />
          {typing && writing && (
            // Around the field, a band to take hold of it by: dragged, what is being written goes
            // along, to be put elsewhere, and is still being written. Pressed and let go where it
            // is, what is being written is held as a thing: the field gives up the keyboard, and
            // Delete takes all of it away.
            <div
              className={held ? "holding held" : "holding"}
              style={{ left: (typing.at[0] - look.x) * perPixel, top: (typing.at[1] - look.y) * perPixel }}
              onMouseDown={(event) => event.target === event.currentTarget && event.preventDefault()}
              onPointerDown={(event) => {
                if (event.target !== event.currentTarget || event.button !== 0 || perPixel === 0) return;
                event.currentTarget.setPointerCapture(event.pointerId);
                carrying.current = { from: [event.clientX, event.clientY], was: typing.at, far: false };
              }}
              onPointerMove={(event) => {
                const carried = carrying.current;
                // (Only while this band has the pointer: not a pointer that merely passes over it.)
                if (!carried || !event.currentTarget.hasPointerCapture(event.pointerId)) return;
                // (A hand is not that still: a few pixels are a press, not a carrying along.)
                if (!carried.far && Math.hypot(event.clientX - carried.from[0], event.clientY - carried.from[1]) < 4) return;
                carried.far = true;
                // By whole pixels of the image (its picture is laid in pixel for pixel), and never
                // out of the part of the picture that is shown: there it could not be taken hold of
                // again.
                const room = 16 / perPixel;
                const within = (value: number, least: number, most: number) => Math.round(Math.min(Math.max(least, most), Math.max(least, value)));
                const at: Place = [
                  within(carried.was[0] + (event.clientX - carried.from[0]) / perPixel, Math.max(look.x, view.x), Math.min(look.x + look.width, view.x + view.width) - room),
                  within(carried.was[1] + (event.clientY - carried.from[1]) / perPixel, Math.max(look.y, view.y), Math.min(look.y + look.height, view.y + view.height) - room),
                ];
                // (What was typed meanwhile stays typed.)
                setTyping((now) => now && { ...now, at });
              }}
              onPointerUp={() => {
                if (carrying.current && !carrying.current.far) {
                  // (The keyboard first: the field taking it is the field being written in. And
                  // nothing of it marked: a key typed next is typed after it, not over it.)
                  field.current?.focus({ preventScroll: true });
                  field.current?.setSelectionRange(field.current.value.length, field.current.value.length);
                  setHeld(true);
                }
                carrying.current = null;
              }}
              onPointerCancel={() => (carrying.current = null)}
              onLostPointerCapture={() => (carrying.current = null)}
            >
            <textarea
              ref={field}
              className="writing"
              value={typing.text}
              rows={1}
              spellCheck={false}
              aria-label={t("editor.tool.text")}
              onFocus={() => setHeld(false)}
              onPointerDown={() => setHeld(false)}
              onCompositionStart={() => setHeld(false)}
              style={{
                padding: writing.around * perPixel,
                fontFamily: fontsOf(letters.font),
                fontWeight: weightOf(letters),
                fontStyle: letters.italic ? "italic" : "normal",
                ...(letters.strike && {
                  textDecorationLine: "line-through",
                  textDecorationThickness: Math.max(1, typingSize * STRUCK.thick) * perPixel,
                  // (Seen where the letters have no inside, too: of the line around them there.)
                  textDecorationColor: letters.fill > 0 ? letters.color : (letters.outline ?? letters.color),
                }),
                fontSize: typingSize * perPixel,
                lineHeight: `${writing.line * perPixel}px`,
                // As it will be written: the inside of the letters as solid as it is, and the line
                // around them under it (as wide again inwards as it reaches outwards).
                color: faded(letters.color, letters.fill),
                ...(letters.outline &&
                  letters.edge > 0 && {
                    WebkitTextStroke: `${letters.edge * 2 * perPixel}px ${letters.outline}`,
                    paintOrder: "stroke fill",
                  }),
                // (Where the letters have next to no inside, the caret is of the line's colour.)
                caretColor: held ? "transparent" : letters.fill >= 0.4 ? letters.color : (letters.outline ?? letters.color),
              }}
              onChange={(event) => {
                const text = event.target.value;
                setTyping((now) => now && { ...now, text });
              }}
            />
            </div>
          )}
        </div>
      </div>
      {letteredAt &&
        createPortal(
          <Lettering
            // (Anew for each thing written: what was half typed into it is about the one before.)
            key={typing ? `typing ${typing.index}` : `chosen ${selected}`}
            sheet={canvas}
            at={letteredAt}
            now={lettersNow}
            set={setLettering}
            // Done in one of its fields: back to what is being written.
            back={() => field.current?.focus({ preventScroll: true })}
            t={t}
          />,
          document.body,
        )}
      {turned &&
        createPortal(
          <div className={turned.going ? "turned going" : "turned"} style={{ left: turned.x, top: turned.y }} aria-hidden="true">
            <ToolIcon tool={tool} look={looks[tool]} />
          </div>,
          document.body,
        )}
      {beside &&
        scale > 0 &&
        createPortal(
          // How closely the picture is looked at: a pixel of the image to one of the screen is 100 %.
          // (A press on it does not take the keyboard: Enter still means what it meant.)
          <div className="zoom" onMouseDown={(event) => event.preventDefault()}>
            <output title={t("editor.zoom")}>{Math.round(scale * (window.devicePixelRatio || 1) * 100)}%</output>
            <button
              title={`${t("editor.zoom.whole")} (0)`}
              aria-label={t("editor.zoom.whole")}
              disabled={times === 1 && looking.x === undefined && looking.y === undefined}
              onClick={() => setLooking({ times: 1 })}
            >
              <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10" />
              </svg>
            </button>
          </div>,
          beside,
        )}
    </>
  );
}

/** The settings of what is written, in a box by it: the font, the size of the letters in pixels of
    the image, and the colour. Over what is written, so that it stays where it is while that grows;
    under it where there is no room over it, beside it where there is none either way; never out
    of the window. The fonts and the colours open from it, away from what is written. */
function Lettering({
  sheet,
  at,
  now,
  set,
  back,
  t,
}: {
  /** The canvas (the whole place the picture is in): what `at` is measured from. */
  sheet: React.RefObject<HTMLCanvasElement | null>;
  at: { x: number; y: number; width: number; height: number };
  now: Writing;
  set: (what: Partial<Writing>, dragged?: boolean) => void;
  back: () => void;
  t: Translate;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number; side: "over" | "under" | "beside" } | null>(null);
  // Laid out after every drawing of it: what it is by may have moved.
  useLayoutEffect(() => {
    const [picture, own] = [sheet.current?.getBoundingClientRect(), box.current?.getBoundingClientRect()];
    if (!picture || !own) return;
    const [away, edge] = [14, 8];
    const within = (value: number, most: number) => Math.round(Math.max(edge, Math.min(value, most)));
    const [left, top] = [picture.left + at.x, picture.top + at.y];
    const [under, over] = [top + at.height + away, top - own.height - away];
    const [after, before] = [left + at.width + away, left - own.width - away];
    const across = within(left, window.innerWidth - own.width - edge);
    const down = (value: number) => within(value, window.innerHeight - own.height - edge);
    const next =
      over >= edge
        ? { left: across, top: down(over), side: "over" as const }
        : under + own.height <= window.innerHeight - edge
          ? { left: across, top: down(under), side: "under" as const }
          : {
              left: within(after + own.width <= window.innerWidth - edge || before < edge ? after : before, window.innerWidth - own.width - edge),
              top: within(top, window.innerHeight - own.height - edge),
              side: "beside" as const,
            };
    if (!place || place.left !== next.left || place.top !== next.top || place.side !== next.side) setPlace(next);
  });

  // What is typed into the field of the size is its own until it says something; and what it says
  // is the size at once, at each digit: a size that is being typed is seen as it is typed.
  const rounded = String(Math.round(now.size * 10) / 10);
  const [size, setSize] = useState(rounded);
  useEffect(() => {
    // (Not while what is typed comes to the same size: a second digit after the point is not
    // typed over.)
    if (Math.round(Number(size) * 10) / 10 !== Number(rounded)) setSize(rounded);
  }, [rounded]);
  const settle = (typed: string) => {
    const asked = Number(typed);
    if (typed.trim() !== "" && asked >= SIZES.least && asked <= SIZES.most && Math.round(asked * 10) / 10 !== Number(rounded)) {
      set({ size: Math.round(asked * 10) / 10 }, true);
    }
  };
  // What is open under (or over) the box: the fonts, the inside of the letters, or the line around
  // them.
  const [open, setOpen] = useState<"fonts" | "fill" | "outline" | null>(null);
  // (Each time one of them opens it is a new one: not the one that was on its way out, should it
  // open again that soon. What it does when it opens, it does then too.)
  const [turn, was] = [useRef(0), useRef(open)];
  if (was.current !== open) {
    was.current = open;
    if (open) turn.current += 1;
  }
  // "None" takes the place of the darkest grey among the colours: the rows stay six wide.
  const colours = PALETTE.filter((_, index) => index !== 5);
  // A press anywhere else puts it away.
  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => {
      if (!(event.target instanceof Node && box.current?.contains(event.target))) setOpen(null);
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  }, [open]);
  // They open away from what is written, which is to be seen while they are chosen from.
  const prefer = place?.side === "under" ? "down" : "up";

  return (
    <div
      ref={box}
      className="lettering"
      role="group"
      aria-label={t("editor.tool.text")}
      style={place ? { left: place.left, top: place.top } : { left: 0, top: 0, visibility: "hidden" }}
      // A press on it, where it is not one of its fields, does not take the keyboard from what is
      // being written.
      onMouseDown={(event) => !(event.target instanceof HTMLInputElement) && event.preventDefault()}
    >
      <div className="row">
        <button
          className="font"
          title={t("editor.font")}
          aria-label={t("editor.font")}
          aria-haspopup="listbox"
          aria-expanded={open === "fonts"}
          style={{ fontFamily: fontsOf(now.font) }}
          onClick={() => setOpen(open === "fonts" ? null : "fonts")}
        >
          <span>{now.font || t("editor.font.own")}</span>
          <Caret />
        </button>
        <label className="size" title={t("editor.size")}>
          <input
            type="number"
            min={SIZES.least}
            max={SIZES.most}
            step={1}
            value={size}
            aria-label={t("editor.size")}
            onChange={(event) => {
              const typed = event.target.value;
              setSize(typed);
              settle(typed);
            }}
            // (Left: it says the size as it is. What was no size, what came out as another, and
            // what could not be had are not left standing in it.)
            onBlur={(event) => {
              if (event.target.value !== rounded) setSize(rounded);
            }}
          />
          <span>px</span>
        </label>
      </div>
      <div className="row">
        {/* Bold, slanted, struck through: each on or off, and shown as it makes the letters. */}
        {(["bold", "italic", "strike"] as const).map((one) => (
          <button
            key={one}
            className={`shape ${one}`}
            title={t(`editor.${one}`)}
            aria-label={t(`editor.${one}`)}
            aria-pressed={now[one]}
            onClick={() => set({ [one]: !now[one] })}
          >
            {{ bold: "B", italic: "I", strike: "S" }[one]}
          </button>
        ))}
        <span className="gap" />
        {/* The inside of the letters (a disc of its colour), and the line around them (a ring of
            its colour); struck through when there is none. Each by its name: a disc and a ring
            alone do not say which is which. */}
        <button
          className="colour"
          title={t("editor.fill")}
          aria-label={t("editor.fill")}
          aria-haspopup="dialog"
          aria-expanded={open === "fill"}
          onClick={() => setOpen(open === "fill" ? null : "fill")}
        >
          <span className="name">{t("editor.fill")}</span>
          <span
            className={now.fill > 0 ? "swatch" : "swatch none"}
            style={{ "--swatch": faded(now.color, now.fill) } as React.CSSProperties}
          />
          <Caret />
        </button>
        <button
          className="colour"
          title={t("editor.text.outline")}
          aria-label={t("editor.text.outline")}
          aria-haspopup="dialog"
          aria-expanded={open === "outline"}
          onClick={() => setOpen(open === "outline" ? null : "outline")}
        >
          <span className="name">{t("editor.text.outline")}</span>
          <span
            className={now.outline ? "swatch ring" : "swatch ring none"}
            style={{ "--swatch": now.outline ?? "transparent" } as React.CSSProperties}
          />
          <Caret />
        </button>
      </div>
      <Lingering>
      {open === "fonts" && (
        <Drop key={turn.current} prefer={prefer} look="fonts">
          <Fonts
            now={now.font}
            pick={(font) => {
              set({ font });
              setOpen(null);
              back();
            }}
            close={() => {
              setOpen(null);
              back();
            }}
            t={t}
          />
        </Drop>
      )}
      </Lingering>
      <Lingering>
      {open === "fill" && (
        // (Looking as the colours do under the bars: what they look like is said of what is in a
        // bar of tools.) The inside of the letters: a colour or none, and how solid.
        <Drop key={turn.current} prefer={prefer} look="colours tools">
          <Colours
            none
            colours={colours}
            value={now.fill > 0 ? now.color : null}
            fallback={now.color}
            onPick={(one, dragged) => set(one === null ? { fill: 0 } : { color: one, ...(now.fill === 0 && { fill: 1 }) }, dragged)}
            // (A colour chosen is what they were opened for: they are put away.)
            onChosen={() => setOpen(null)}
            t={t}
          />
          <label className={now.fill > 0 ? "setting" : "setting off"}>
            <span className="heading">{t("editor.opacity")}</span>
            <span className="slider">
              <input
                type="range"
                min={5}
                max={100}
                step={5}
                disabled={now.fill === 0}
                value={Math.round((now.fill || 1) * 100)}
                aria-label={t("editor.opacity")}
                onChange={(event) => set({ fill: Number(event.target.value) / 100 }, true)}
              />
              <output>{Math.round(now.fill * 100)}%</output>
            </span>
          </label>
        </Drop>
      )}
      </Lingering>
      <Lingering>
      {open === "outline" && (
        // The line around the letters: a colour or none, and how far it reaches out of them, in
        // pixels of the image.
        <Drop key={turn.current} prefer={prefer} look="edges tools">
          <Colours
            none
            colours={colours}
            value={now.outline}
            fallback={now.outline ?? now.color}
            onPick={(one, dragged) => set({ outline: one }, dragged)}
            // (A colour chosen is what they were opened for: they are put away.)
            onChosen={() => setOpen(null)}
            t={t}
          />
          <label className={now.outline ? "setting" : "setting off"}>
            <span className="heading">{t("editor.outline.width")}</span>
            <span className="slider">
              <input
                type="range"
                min={EDGES.least}
                max={EDGES.most}
                step={1}
                disabled={!now.outline}
                value={Math.max(EDGES.least, Math.min(EDGES.most, Math.round(now.edge || firstEdge(now.size))))}
                aria-label={t("editor.outline.width")}
                onChange={(event) => set({ edge: Number(event.target.value) }, true)}
              />
              <output>{now.outline ? `${Math.round(now.edge * 10) / 10} px` : ""}</output>
            </span>
          </label>
        </Drop>
      )}
      </Lingering>
    </div>
  );
}

/** What opens from the box by what is written: under it or over it, the way that was asked for
    where there is room for it that way, the other way where there is more room. */
function Drop({ prefer, look, children }: { prefer: "up" | "down"; look: string; children: React.ReactNode }) {
  const own = useRef<HTMLDivElement>(null);
  // Which way it opens, and how tall it can be there: where it does not fit either way, it is as
  // tall as there is room, and what is in it is scrolled through.
  const [fit, setFit] = useState({ up: prefer === "up", most: 0 });
  useLayoutEffect(() => {
    // (From the box, not from what holds it a moment longer on its way out: that is no box.)
    const [from, all] = [own.current?.closest(".lettering")?.getBoundingClientRect(), own.current?.scrollHeight];
    if (!from || all === undefined) return;
    const [above, below, needed] = [from.top - 14, window.innerHeight - from.bottom - 14, all + 2];
    const up = prefer === "up" ? needed <= above || above >= below : !(needed <= below || below >= above);
    const most = Math.max(80, Math.floor(up ? above : below));
    if (up !== fit.up || most !== fit.most) setFit({ up, most });
  });
  return (
    <div ref={own} className={`drop ${look}${fit.up ? " up" : ""}`} style={fit.most ? { maxHeight: fit.most } : undefined}>
      {children}
    </div>
  );
}

/** The fonts the system has, asked for once: the same for every capture. */
let systemFonts: string[] | null = null;

/** The fonts to write in, to choose from: Perora's own first, then every family the system has,
    each written in itself. Typed into, the list is of those with that in their names; Enter takes
    the first of them, the arrows go through them, Escape puts the list away. */
function Fonts({ now, pick, close, t }: { now: string; pick: (font: string) => void; close: () => void; t: Translate }) {
  const [all, setAll] = useState(systemFonts ?? FONTS);
  useEffect(() => {
    if (systemFonts) return;
    let stopped = false;
    fontFamilies().then(
      (found) => {
        // (Nothing said, or nothing found: the few that are offered without asking stay.)
        if (!Array.isArray(found) || found.length === 0) return;
        systemFonts = found;
        if (!stopped) setAll(found);
      },
      () => {},
    );
    return () => {
      stopped = true;
    };
  }, []);
  const [sought, setSought] = useState("");
  const wanted = sought.trim().toLowerCase();
  // (Perora's own is the font named by no name.)
  const shown = [...(wanted === "" ? [""] : []), ...all.filter((font) => font.toLowerCase().includes(wanted))];
  // Which of them the keys are at: the one in use to start with.
  const [at, setAt] = useState(() => Math.max(0, shown.indexOf(now)));
  const here = Math.min(at, shown.length - 1);
  const list = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [here, shown.length]);

  return (
    <>
      <input
        className="seek"
        type="text"
        value={sought}
        placeholder={t("editor.font.seek")}
        aria-label={t("editor.font.seek")}
        spellCheck={false}
        autoFocus
        data-own-keys
        onChange={(event) => {
          setSought(event.target.value);
          setAt(0);
        }}
        onKeyDown={(event) => {
          const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
          if (step === 0 && event.key !== "Enter" && event.key !== "Escape") return;
          // (None of these is for anything else: not for the capture, not for what is written.)
          event.preventDefault();
          event.stopPropagation();
          if (step !== 0) return setAt(Math.max(0, Math.min(shown.length - 1, here + step)));
          if (event.key === "Enter" && shown[here] !== undefined) return pick(shown[here]);
          close();
        }}
      />
      <div ref={list} className="list" role="listbox" aria-label={t("editor.font")}>
        {shown.map((font, index) => (
          <button
            key={font}
            role="option"
            aria-selected={index === here}
            className={font === now ? "current" : undefined}
            style={{ fontFamily: fontsOf(font) }}
            onClick={() => pick(font)}
          >
            {font || t("editor.font.own")}
          </button>
        ))}
        {shown.length === 0 && <p className="none">{t("editor.font.none")}</p>}
      </div>
    </>
  );
}

/** The panels that open under buttons of the bars: the colours and size of the pen, of the marker,
    of a stroke that is selected, the inside and the outline of shapes, and the shapes the image can
    be cut down to. */
type PanelName = "pen" | "marker" | "stroke" | "fill" | "outline" | "ratio" | "letters" | "stamps";

/** Keeps what opens under a button inside the window: moved sideways as far as it would stick out,
    and no taller than the room below it (it scrolls then). For a ref: called when it appears. */
/** Keeps what was put where the pointer is inside the window: moved by as much as it is out. */
function inWindow(bar: HTMLElement | null) {
  if (!bar) return;
  const edge = 8;
  bar.style.margin = "0";
  const box = bar.getBoundingClientRect();
  const across = Math.max(edge - box.left, Math.min(0, window.innerWidth - edge - box.right));
  // (It is measured as it starts to come down into its place: that much higher than its place.)
  const down = Math.max(edge - box.top, Math.min(0, window.innerHeight - edge - (box.bottom + 14)));
  bar.style.margin = `${down}px 0 0 ${across}px`;
}

function inView(panel: HTMLElement | null) {
  placed(panel, true);
}
/** What is where the pointer was is placed again when it grows (the colours give way to the place a
    colour is mixed in, which is taller): on the side it opened to, moved as far as it takes. */
const grown = new ResizeObserver((entries) => {
  for (const { target } of entries) {
    if (target.isConnected) placed(target as HTMLElement, false, true);
    else grown.unobserve(target);
  }
});
function placed(panel: HTMLElement | null, anew: boolean, stays = false) {
  if (!panel) return;
  const edge = 8;
  panel.style.marginLeft = "0";
  panel.style.maxHeight = "none";
  const box = panel.getBoundingClientRect();
  const over = Math.max(0, box.right - (window.innerWidth - edge));
  const under = Math.max(0, edge - (box.left - over));
  panel.style.marginLeft = `${under - over}px`;
  // Of the settings that are where the pointer was (anywhere in the window, then): over what it
  // opens from, where it is too tall for the room under that and there is more room over it.
  const from = panel.closest(".afloat") && panel.closest(".anchor")?.getBoundingClientRect();
  if (from) {
    const [below, above] = [window.innerHeight - edge - (from.bottom + 8), from.top - 8 - edge];
    // (Said only when it changes: said again, it would come into its place again.)
    const up = stays ? panel.classList.contains("up") : box.height > below && above > below;
    if (panel.classList.contains("up") !== up) panel.classList.toggle("up", up);
    // Where it has no room either way, it is not made shorter (what is in it would have to be
    // scrolled through): it is moved, as far as it takes to be in the window, over what it opens
    // from. Only a window that is itself shorter than it makes it shorter.
    const most = window.innerHeight - edge * 2;
    const short = Math.max(0, Math.min(box.height, most) - (up ? above : below));
    panel.style.marginTop = up ? "" : `${-short}px`;
    panel.style.marginBottom = up ? `${-short}px` : "";
    panel.style.maxHeight = `${most}px`;
    // (And once more when what is in it has laid itself out: the colours are taller for a moment
    // than they will be, and it would be placed for that.)
    if (anew) {
      const again = () => panel.isConnected && placed(panel, false);
      queueMicrotask(again);
      requestAnimationFrame(again);
      grown.observe(panel);
    }
    return;
  }
  panel.style.maxHeight = `${Math.max(120, window.innerHeight - box.top - edge)}px`;
}

/** How long what is put away takes to go, in milliseconds (the style sheet's `go-up`). */
const GOING = 160;

/** Shows what it is given; and when that is gone, what it was a moment longer, on its way out: the
    second bar and the panels do not vanish, they go back up where they came from. (Nor does what
    opens from the box by what is written.) */
function Lingering({ children }: { children: React.ReactNode }) {
  const there = children !== null && children !== undefined && children !== false;
  const last = useRef<React.ReactNode>(null);
  if (there) last.current = children;
  const [going, setGoing] = useState(false);
  useEffect(() => {
    if (there) return setGoing(true);
    const gone = window.setTimeout(() => setGoing(false), GOING);
    return () => window.clearTimeout(gone);
  }, [there]);
  if (!there && !going) return null;
  // The same element while it is there and while it goes: what is in it stays what it is, and only
  // turns round.
  return <div className={there ? "lingering" : "lingering leaving"}>{there ? children : last.current}</div>;
}

function Panel({ label, narrow = false, children }: { label: string; narrow?: boolean; children: React.ReactNode }) {
  return (
    <div ref={inView} className={narrow ? "panel narrow" : "panel"} role="dialog" aria-label={label}>
      {children}
    </div>
  );
}

function Caret() {
  return (
    <svg className="caret" viewBox="0 0 8 5" width="8" height="5" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M1 1l3 3 3-3" />
    </svg>
  );
}

// ---------------------------------------------------------------------------------------------
// Choosing a colour and a size

interface Hsv {
  hue: number;
  saturation: number;
  value: number;
}

function toHex({ hue, saturation, value }: Hsv) {
  const part = (n: number) => {
    const k = (n + hue / 60) % 6;
    const level = value - value * saturation * Math.max(0, Math.min(k, 4 - k, 1));
    return Math.round(level * 255)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${part(5)}${part(3)}${part(1)}`;
}

function toHsv(hex: string): Hsv {
  const [red, green, blue] = [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16) / 255);
  const [most, least] = [Math.max(red, green, blue), Math.min(red, green, blue)];
  const spread = most - least;
  const hue =
    spread === 0
      ? 0
      : most === red
        ? ((green - blue) / spread + 6) % 6
        : most === green
          ? (blue - red) / spread + 2
          : (red - green) / spread + 4;
  return { hue: hue * 60, saturation: most === 0 ? 0 : spread / most, value: most };
}

/** The colours to choose from, six to a row; with `none`, also having no colour at all (`null`).
    Or, when asked for, a place to mix any other colour in their stead: one of the two is seen,
    so that mixing takes no more room than choosing.

    The two are one above the other, with the button that tells them apart between them, behind
    an opening as tall as one of them and the button: the colours to choose from are seen over the
    button, and pressed, the button slides up to the top of the opening and the mixing is seen
    under it. */
function Colours({
  none = false,
  colours,
  value,
  fallback = RED,
  onPick,
  onChosen,
  t,
}: {
  none?: boolean;
  colours: string[];
  value: string | null;
  /** Where mixing a colour starts while there is none. */
  fallback?: string;
  /** `dragged` for a colour that is one of many on the way to the one meant. */
  onPick: (colour: string | null, dragged?: boolean) => void;
  /** One of the colours offered was pressed (or "none"): that is the choice made, where mixing a
      colour is many choices on the way to one. */
  onChosen?: () => void;
  t: Translate;
}) {
  // Open from the start when the colour in use is none of those offered: it was mixed here.
  const [mixing, setMixing] = useState(value !== null && !colours.includes(value));
  // How tall the colours to choose from are: the mixing is made as tall, and the opening as tall
  // as that and the button.
  const grid = useRef<HTMLDivElement>(null);
  const [tall, setTall] = useState(0);
  useLayoutEffect(() => {
    const now = grid.current?.offsetHeight ?? 0;
    if (now !== tall) setTall(now);
  });
  return (
    <div className="setting">
      <span className="heading">{t("editor.colours")}</span>
      <div
        className={mixing ? "either mixing" : "either"}
        style={tall ? ({ "--tall": `${tall}px` } as React.CSSProperties) : undefined}
      >
      <div className="slide">
      {/* (What is out of sight is out of reach too: of the pointer, and of the keyboard.) */}
      <div ref={grid} className="grid" role="radiogroup" aria-label={t("editor.colours")} inert={mixing}>
        {none && (
          <button
            role="radio"
            aria-checked={value === null}
            className={value === null ? "swatch none current" : "swatch none"}
            title={t("editor.none")}
            aria-label={t("editor.none")}
            onClick={() => {
              onPick(null);
              onChosen?.();
            }}
          />
        )}
        {colours.map((one) => (
          <button
            key={one}
            role="radio"
            aria-checked={value === one}
            className={value === one ? "swatch current" : "swatch"}
            style={{ "--swatch": one } as React.CSSProperties}
            title={one}
            aria-label={one}
            onClick={() => {
              onPick(one);
              onChosen?.();
            }}
          />
        ))}
      </div>
      <button className="more" aria-expanded={mixing} onClick={() => setMixing(!mixing)}>
        <span className="swatch any" />
        <span>{t("editor.colours.more")}</span>
        <Caret />
      </button>
      <div className="mixed" inert={!mixing}>
        <Mixer value={value ?? fallback} onPick={onPick} t={t} />
      </div>
      </div>
      </div>
    </div>
  );
}

/** How wide the pen, the marker or a line is: a stroke as wide as it would be drawn, and a slider. */
function Size({
  kind,
  colour,
  step,
  onSet,
  t,
}: {
  kind: "pen" | "marker" | "line";
  colour: string;
  step: number;
  onSet: (step: number) => void;
  t: Translate;
}) {
  // On the screen, a step is about a pixel; a marker is as much wider as it is drawn.
  const wide = Math.min(40, step * (kind === "marker" ? MARKER.width : 1) * 1.2 + 1);
  return (
    <label className="setting">
      <span className="heading">{t("editor.size")}</span>
      <svg className="stroke" viewBox="0 0 240 64" aria-hidden="true">
        <path
          d={kind === "line" ? "M24 32H216" : "M24 40C64 4 100 12 124 34S184 60 216 26"}
          fill="none"
          stroke={colour}
          strokeWidth={wide}
          strokeLinecap={kind === "marker" ? "butt" : "round"}
          opacity={kind === "marker" ? MARKER.opacity + 0.3 : 1}
        />
      </svg>
      <span className="slider">
        <input
          type="range"
          min={WIDTHS.least}
          max={WIDTHS.most}
          value={Math.min(WIDTHS.most, Math.max(WIDTHS.least, step))}
          aria-label={t("editor.size")}
          onChange={(event) => onSet(Number(event.target.value))}
        />
        <output>{step}</output>
      </span>
    </label>
  );
}

/** Any colour: a field of every shade of one hue, the hues below it, and the colour written out,
    as it is written in a page (`#rrggbb`) and as its red, green and blue (each 0 to 255). */
function Mixer({
  value,
  onPick,
  t,
}: {
  value: string;
  onPick: (colour: string, dragged?: boolean) => void;
  t: Translate;
}) {
  // The hue is kept apart from the colour itself: black has no hue of its own, and the field is not
  // to jump to red when the pointer reaches its bottom. What is kept counts only while it is still
  // the colour in use; a colour that came from elsewhere (one of those offered) is taken as it is.
  const [kept, setKept] = useState(() => toHsv(value));
  const mixed = toHex(kept) === value ? kept : toHsv(value);
  // What is being written, until it is a colour; then, and whenever the colour changes, the colour.
  const [typed, setTyped] = useState({ text: value, of: value });
  const written = typed.of === value ? typed.text : value;
  useEffect(() => setTyped({ text: value, of: value }), [value]);
  // The same for its red, green and blue: three numbers, a colour once each of them is one.
  const parts = (colour: string) => [1, 3, 5].map((at) => String(parseInt(colour.slice(at, at + 2), 16)));
  const [typedParts, setTypedParts] = useState({ texts: parts(value), of: value });
  const writtenParts = typedParts.of === value ? typedParts.texts : parts(value);
  useEffect(() => setTypedParts({ texts: parts(value), of: value }), [value]);

  const mix = (next: Hsv) => {
    setKept(next);
    onPick(toHex(next), true);
  };
  const shade = (event: React.PointerEvent) => {
    const box = event.currentTarget.getBoundingClientRect();
    const within = (n: number) => Math.min(1, Math.max(0, n));
    mix({
      ...mixed,
      saturation: within((event.clientX - box.left) / box.width),
      value: 1 - within((event.clientY - box.top) / box.height),
    });
  };

  return (
    <div className="mixer">
      <div
        className="shades"
        style={{ "--hue": toHex({ hue: mixed.hue, saturation: 1, value: 1 }) } as React.CSSProperties}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          shade(event);
        }}
        onPointerMove={(event) => {
          if (event.buttons & 1) shade(event);
        }}
      >
        <span
          className="at"
          style={{ left: `${mixed.saturation * 100}%`, top: `${(1 - mixed.value) * 100}%`, background: value }}
        />
      </div>
      <input
        className="hues"
        type="range"
        min={0}
        max={359}
        value={Math.round(mixed.hue)}
        aria-label={t("editor.colours.hue")}
        onChange={(event) => mix({ ...mixed, hue: Number(event.target.value) })}
      />
      <div className="values">
        <input
          className="written"
          type="text"
          value={written}
          maxLength={7}
          spellCheck={false}
          aria-label={t("editor.colours.written")}
          onChange={(event) => {
            const text = event.target.value.trim().toLowerCase();
            const colour = text.startsWith("#") ? text : `#${text}`;
            const whole = /^#[0-9a-f]{6}$/.test(colour);
            setTyped({ text, of: whole ? colour : value });
            if (whole) onPick(colour);
          }}
        />
        {(["red", "green", "blue"] as const).map((part, index) => (
          <label key={part} className="part" title={t(`editor.colours.${part}`)}>
            <span aria-hidden="true">{part[0].toUpperCase()}</span>
            <input
              type="text"
              inputMode="numeric"
              value={writtenParts[index]}
              maxLength={3}
              spellCheck={false}
              aria-label={t(`editor.colours.${part}`)}
              onChange={(event) => {
                const texts = writtenParts.map((text, at) => (at === index ? event.target.value.trim() : text));
                const whole = texts.every((text) => /^\d{1,3}$/.test(text) && Number(text) <= 255);
                const colour = whole ? `#${texts.map((text) => Number(text).toString(16).padStart(2, "0")).join("")}` : value;
                setTypedParts({ texts, of: colour });
                if (whole && colour !== value) onPick(colour);
              }}
            />
          </label>
        ))}
      </div>
    </div>
  );
}

/** How a tool's button shows what it would draw with: the colour of the pen or the marker, the
    outline of shapes and lines, and the inside of a shape (with how solid it is; none at 0). */
interface Look {
  colour?: string;
  inside?: { colour: string; solid: number };
}

/** The inside of a shape in an icon: its colour as solid as it is drawn, or nothing. */
const insideOf = (look: Look) =>
  look.inside && look.inside.solid > 0
    ? { fill: look.inside.colour, fillOpacity: look.inside.solid }
    : { fill: "none" };

/** The shapes and lines, as one tool of the first bar: drawn with their outline and their inside. */
function ShapesIcon({ look }: { look: Look }) {
  return (
    <svg className="tint" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke={look.colour ?? "currentColor"} strokeWidth="1.5" strokeLinejoin="round" aria-hidden="true">
      <circle cx="6.2" cy="6.2" r="3.9" {...insideOf(look)} />
      <rect x="7.2" y="7.2" width="6.6" height="6.6" rx="1" {...insideOf(look)} />
    </svg>
  );
}

/** The icon of a tool. With a look, it shows what the tool would draw with: the pen and the marker
    have their colour on their tip and in a line under them, as the pens of Windows do; a shape or a
    line is drawn in its outline's colour, a shape with its inside. */
function ToolIcon({ tool, look = {} }: { tool: Tool; look?: Look }) {
  const drawn = look.colour ?? "currentColor";
  // A pen and the line under it: the pen drawn a little smaller, to leave the line its room.
  const withLine = (pen: React.ReactNode, tip: string) => (
    <>
      <g transform="translate(1.5 0) scale(0.82)">
        {pen}
        {look.colour && <path className="tint" d={tip} fill={look.colour} />}
      </g>
      {look.colour && <path className="tint" d="M2.5 14.6h11" stroke={look.colour} strokeWidth="2" />}
    </>
  );
  const shapes: Record<Tool, React.ReactNode> = {
    select: <path d="M4 2.5v10.2l2.9-2.6 1.9 4.1 1.6-.7-1.9-4h3.8z" fill="currentColor" stroke="none" />,
    pen: withLine(
      <path d="M2.5 13.5l.8-3.2 7.6-7.6a1.4 1.4 0 0 1 2 0l.4.4a1.4 1.4 0 0 1 0 2l-7.6 7.6zM9.8 3.8l2.4 2.4" />,
      "M2.5 13.5l.8-3.2 2.4 2.4z",
    ),
    marker: withLine(
      <path d="M5.2 9.2l5.5-6 2.6 2.4-5.5 6zM5.2 9.2l-1.4 3 3.9-.7" />,
      "M5.2 9.2l-1.4 3 3.9-.7z",
    ),
    line: <path className="tint" d="M3 13L13 3" stroke={drawn} />,
    arrow: <path className="tint" d="M3 13L12.5 3.5M7 3.2h5.8V9" stroke={drawn} />,
    blur: (
      <>
        <path d="M8 2.2C6 5 4.2 7.2 4.2 9.6a3.8 3.8 0 0 0 7.6 0C11.8 7.2 10 5 8 2.2z" />
        <path d="M6.6 10.4a1.6 1.6 0 0 0 1.5 1.3" opacity="0.6" />
      </>
    ),
    mosaic: (
      <>
        <rect x="2.5" y="2.5" width="11" height="11" rx="1" />
        <path d="M2.5 6.2h3.7V2.5M9.8 2.5v3.7h3.7M2.5 9.8h3.7v3.7M9.8 13.5V9.8h3.7M6.2 6.2h3.6v3.6H6.2z" fill="currentColor" fillOpacity="0.35" stroke="none" />
        <path d="M6.2 2.5v11M9.8 2.5v11M2.5 6.2h11M2.5 9.8h11" strokeWidth="0.8" opacity="0.6" />
      </>
    ),
    crop: <path d="M4.5 1.5v10h10M1.5 4.5h10v10" />,
    // A letter, with a line under it in the colour it is written in; a number in its disc.
    text: (
      <>
        <path d="M4 4.2V2.8h8v1.4M8 2.8v8.4M6.4 11.2h3.2" />
        {look.colour && <path className="tint" d="M2.5 14.6h11" stroke={look.colour} strokeWidth="2" />}
      </>
    ),
    stamp: (
      <>
        <circle className="tint" cx="8" cy="8" r="6" fill={look.colour ?? "none"} stroke={look.colour ? "none" : "currentColor"} />
        <path d="M6.9 6.3L8.4 5.2v5.6" stroke={look.colour ? (isLight(look.colour) ? "#000000" : "#ffffff") : "currentColor"} />
      </>
    ),
    rectangle: <rect className="tint" x="2.5" y="3.5" width="11" height="9" rx="0.5" stroke={drawn} {...insideOf(look)} />,
    ellipse: <ellipse className="tint" cx="8" cy="8" rx="5.5" ry="4.5" stroke={drawn} {...insideOf(look)} />,
  };
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {shapes[tool]}
    </svg>
  );
}
