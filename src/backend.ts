// What the windows know of the application: its state, asked for again whenever it says it changed,
// and the handful of things the user can choose. Everything is decided on the other side; nothing here
// keeps a capture, a link or a token.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

export type Progress = "waiting" | "uploading" | "failed";

/** A place in the image, in its pixels from its top-left corner: across, then down. */
export type Place = [number, number];

/** One thing drawn on the capture, as the application knows it (`perora_core::edit`). Colours are
    written `#rrggbb`; widths are in pixels of the image. Of a shape, `color` is the outline's,
    `fill` how solid its inside is, from 0 (only the outline) to 1, and `fillColor` the colour of
    that (the outline's when none is named). */
export type Edit =
  | { kind: "pen" | "marker"; points: Place[]; color: string; width: number }
  | { kind: "line" | "arrow"; from: Place; to: Place; color: string; width: number }
  | {
      kind: "rectangle" | "ellipse";
      from: Place;
      to: Place;
      color: string;
      width: number;
      fill: number;
      fillColor?: string;
    }
  // What is between two corners made unreadable: blurred (each pixel spread `radius` pixels), or
  // broken into blocks `cell` pixels wide. As it is at that point of the drawing: what is drawn
  // after it is not.
  | { kind: "blur"; from: Place; to: Place; radius: number }
  | { kind: "mosaic"; from: Place; to: Place; cell: number }
  // What the image is cut down to. The last of these counts, and it is done last.
  | { kind: "crop"; from: Place; to: Place }
  // Something written, and a number in a disc, between two corners: what was written, its colour,
  // the size of its letters in pixels of the image, and the family of the font it is written in
  // (none for the one Perora writes in when none is chosen). `fill` is how solid the inside of its
  // letters is (1 when nothing is said, 0 for none); `outline` the colour of a line around them,
  // when they have one, and `outlineWidth` how far that reaches out of them, in pixels of the image.
  // `bold`, `italic` and `strike` are said of letters that are bold, slanted, struck through.
  | {
      kind: "text";
      from: Place;
      to: Place;
      text: string;
      color: string;
      size: number;
      font?: string;
      fill?: number;
      outline?: string;
      outlineWidth?: number;
      bold?: boolean;
      italic?: boolean;
      strike?: boolean;
    }
  | { kind: "stamp"; from: Place; to: Place; number: number; color: string };

export interface Capture {
  id: number;
  width: number;
  height: number;
  progress: Progress;
  /** With "failed": why. */
  message: string | null;
  /** Whether the image is on the clipboard; why not, when it is not. */
  copied: boolean;
  copyFailure: string | null;
  /** What is drawn on it so far. */
  edits: Edit[];
  /** Whether it was drawn on since it went to the clipboard: what is there is an older look of it. */
  copyIsOld: boolean;
  /** What it was made into as it went there (the kind of file, the square it was to fit): asked
      for as something else, it is not what is there. */
  copiedAs: { format: string; fit: number };
  /** What was chosen for this capture alone, of each way it can leave, in either window. */
  once?: Partial<Record<Way, Once>>;
  /** Which making of the small picture is the one to show: it is made anew when the small window
      is to show a drawing it does not show yet. */
  thumbnail: number;
  /** How large the image is as it would leave: cut down, when it was. */
  keptWidth: number;
  keptHeight: number;
}

export interface View {
  language: string;
  texts: Record<string, string>;
  capture: Capture | null;
  /** Whether the small window is on screen with no capture, only to show where it appears. */
  sample?: boolean;
  /** Whether uploading is among what can be done with a capture: the user chose so, and someone
      is signed in. */
  offersUpload: boolean;
  /** The colour the user chose for their system, as `#rrggbb`. */
  accent: string | null;
  /** The kinds of file a capture can leave as (`png`, `jpg`, `webp`), the squares it can be made
      to fit (pixels a side), and what the settings say of each way of leaving. */
  formats: string[];
  fits: number[];
  leaves: Record<Way, Required<Once>>;
  /** The tours that were seen, by their names. */
  toursSeen: string[];
}

/** A way a capture leaves. */
export type Way = "upload" | "save" | "copy";

/** What is asked for one time, in place of what the settings say: the kind of file, the square it
    is to fit (pixels a side; 0 for as large as it is), or neither. */
export interface Once {
  format?: string;
  /** Several kinds at once, for saving: one file of each. */
  formats?: string[];
  fit?: number;
}

/** Which of the two windows this is: the small one in the corner, or the large one. */
export const large = getCurrentWindow().label === "preview-large";

/** The state of the preview, kept up to date. `null` until it has been asked for once. */
export function useView(): View | null {
  const [view, setView] = useState<View | null>(null);

  useEffect(() => {
    let stopped = false;
    const load = () =>
      invoke<View>("preview_state").then((next) => {
        if (!stopped) setView(next);
      });

    void load();
    const listening = listen("preview-changed", () => void load());
    return () => {
      stopped = true;
      void listening.then((stop) => stop());
    };
  }, []);

  return view;
}

/** The address of the capture's image: the whole of it, or the small version for the corner. */
export const imageUrl = (capture: Capture, which: "image" | "thumbnail") =>
  which === "image"
    ? convertFileSrc(`capture/${capture.id}/image.png`, "perora")
    : // Asked for by its making: a new one is another address, and is fetched.
      `${convertFileSrc(`capture/${capture.id}/thumbnail.png`, "perora")}?made=${capture.thumbnail}`;

// Each of these names the capture it is about. The application does nothing when that is no longer
// the one it previews: a click acts on what was on screen when it was made.
const about = (capture: Capture) => ({ id: capture.id });

/** The capture's image as a file. Asked of the application as bytes, not shown from its address: a
    picture drawn on a canvas from another address cannot be read back from the canvas, and the
    window that is drawn in makes the image that leaves by reading it back. */
export const captureFile = (capture: Capture) =>
  invoke<ArrayBuffer>("preview_image", about(capture)).then((bytes) => new Blob([bytes], { type: "image/png" }));

/** Tells the application what is drawn on the capture, whenever that changed: for a window that
    asks anew. Refused with `tooMuch` when it is more than a drawing is. */
export const tellEdits = (capture: Capture, edits: Edit[]) =>
  invoke<void>("preview_edits", { ...about(capture), edits });

/** What the window that is drawn in hands over when the capture is to leave: what is drawn, and the
    image it made of the capture with that drawn into it (a PNG file; none when nothing is drawn). */
export interface Leaving {
  edits: Edit[];
  picture: Blob | null;
}

/** What is asked of the application about a capture. The small window names the capture, and what
    the application has of it is used. The large one hands over the drawing and its image in one:
    four bytes that say how long what follows them is (least first), that (the capture and what is
    drawn), and then the image. */
async function asked(
  capture: Capture,
  drawing?: Leaving,
  once?: Once,
): Promise<Record<string, unknown> | Uint8Array> {
  // (How it is to leave this once goes along with the name of the capture, when something is
  // asked.)
  const as =
    once && (once.format !== undefined || once.formats !== undefined || once.fit !== undefined) ? { as: once } : {};
  if (!drawing) return { ...about(capture), ...as };
  const named = new TextEncoder().encode(JSON.stringify({ ...about(capture), ...as, edits: drawing.edits }));
  const picture = drawing.picture ? new Uint8Array(await drawing.picture.arrayBuffer()) : new Uint8Array();
  const bytes = new Uint8Array(4 + named.length + picture.length);
  new DataView(bytes.buffer).setUint32(0, named.length, true);
  bytes.set(named, 4);
  bytes.set(picture, 4 + named.length);
  return bytes;
}

// Refused (and nothing done) with `tooMuch` when the drawing is more than a drawing is, and with
// `notMade` when there is no image of the capture as it is drawn on.
export const upload = async (capture: Capture, drawing?: Leaving, once?: Once) =>
  invoke<void>("preview_upload", await asked(capture, drawing, once));
export const save = async (capture: Capture, drawing?: Leaving, once?: Once) =>
  invoke<void>("preview_save", await asked(capture, drawing, once));
/** Puts the capture on the clipboard again, as it is drawn on now. Refused with the reason when
    it could not be put there. */
export const copy = async (capture: Capture, drawing?: Leaving, once?: Once) =>
  invoke<void>("preview_copy", await asked(capture, drawing, once));
/** The large window, about to hand over the capture to be uploaded or saved, is taken out of sight
    at once: making the image that leaves takes a moment, and a choice closes the window at once.
    `appear` when it is done, whatever came of it: should the window still be there (nothing left),
    it is seen again. */
export const vanish = (capture: Capture, to: "upload" | "save") =>
  invoke<void>("preview_vanish", { ...about(capture), to });
export const appear = () => invoke<void>("preview_appear");

/** The fonts the system has, by the names of their families, in order. */
export const fontFamilies = () => invoke<string[]>("preview_fonts");

export const discard = (capture: Capture) => invoke<void>("preview_discard", about(capture));
export const enlarge = (capture: Capture) => invoke<void>("preview_enlarge", about(capture));
/** Tells that the large window has drawn the capture: shown for it at once, it is shown then. */
export const drawn = (capture: Capture) => invoke<void>("preview_drawn", about(capture));
/** Tells what was chosen for this capture alone: the other window shows the same. */
export const choose = (capture: Capture, once: Partial<Record<Way, Once>>) =>
  invoke<void>("preview_choose", { ...about(capture), once });

/** A text in the user's language, with each `{name}` replaced by its value. */
export function translator(texts: Record<string, string>) {
  return (key: string, values: Record<string, string | number> = {}) =>
    (texts[key] ?? key).replace(/\{(\w+)\}/g, (blank, name: string) =>
      name in values ? String(values[name]) : blank,
    );
}
