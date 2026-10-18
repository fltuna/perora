import { useEffect, useRef, useState } from "react";
import { Editor, type EditorShow, MOST_EDITS } from "./editor";
import {
  type Capture,
  type Leaving,
  type Once,
  type View,
  type Way,
  appear,
  choose,
  copy,
  discard,
  enlarge,
  imageUrl,
  large,
  save,
  translator,
  upload,
  useView,
  vanish,
} from "./backend";
import { Tour, TourButton, touring, useTour } from "./tour";
import { editorSteps, previewSteps } from "./tours";

type Translate = ReturnType<typeof translator>;

/** Whether the choice of what the capture leaves as is open: the keys are about that then, and
    nothing else acts on them (asked of the page, so that whoever listens need not be told). */
export const choosingNow = () => document.querySelector(".actions .menu") !== null;

export function Preview() {
  const view = useView();
  // The small window, on screen for a moment with no capture: it shows where it appears (the user
  // just chose the corner), looking as it does with a capture in it. What it shows then is no
  // capture, and nothing in it does anything.
  const sampling = !large && !!view && !view.capture && !!view.sample;
  const capture = view?.capture ?? (sampling ? sampleOf(view) : null);
  // Without uploading on offer, saving is what a capture is for: the button that stands out, and
  // Ctrl+Enter.
  const offersUpload = view?.offersUpload ?? false;
  // Why the image could not be saved, when it could not: the one choice that can fail on the spot.
  const [saveFailure, setSaveFailure] = useState<string | null>(null);
  // The drawing is as long as a drawing gets: nothing more was taken, and the user is told.
  const [full, setFull] = useState(false);
  // Why it could not be copied again, when it could not.
  const [copyFailure, setCopyFailure] = useState<string | null>(null);
  // The image of the capture as it is drawn on could not be made: nothing left, and the user is told.
  const [unmade, setUnmade] = useState(false);
  // Asked before anything leaves, of the large window: what is half done there (a frame set for
  // cutting the picture down, something still being written) is done, and what is drawn then comes
  // back with the image made of it. Or "stay", when that could not be done: then nothing leaves, and
  // the window says why. The small window has nothing to say: what the application has is used.
  const settle = useRef<(() => Promise<Leaving | "stay">) | null>(null);
  const leaving = async () => (settle.current ? settle.current() : undefined);
  // What the tour of the large window has it do.
  const show = useRef<EditorShow | null>(null);
  // A place beside the status, which the large window fills in: how closely the picture is looked at.
  const [beside, setBeside] = useState<HTMLElement | null>(null);

  useEffect(() => {
    if (view) document.documentElement.lang = view.language;
  }, [view?.language]);

  // The large window in sight, whenever its page starts: should it have been left out of sight by
  // a page before this one that never got to say it was done.
  useEffect(() => {
    if (large) appear().catch(() => {});
  }, []);

  // How many things are drawn on the capture, as the window that is drawn in last said: it tells
  // each time, while the application says anew only when something else turns on it.
  const [drawn, setDrawn] = useState<number | null>(null);

  // What is asked for this once, of each way the capture leaves, in place of what the settings
  // say: chosen from the little menu beside each, and for this capture only. And which of those
  // menus is open. (Chosen in one window, it is chosen in the other: the application is told,
  // and what it says of a capture is what the window starts from.)
  const [once, setOnce] = useState<Partial<Record<Way, Once>>>({});
  const onceNow = useRef(once);
  onceNow.current = once;
  const [choosing, setChoosing] = useState<Way | null>(null);
  // (What the application says is taken as it comes, but for one moment: while it is still being
  // told what was chosen here, what it says is from before that. Of another capture, it is
  // always taken: nothing chosen for the one before is left on the next.)
  const telling = useRef(0);
  const told = useRef(capture?.id);
  const chosenElsewhere = JSON.stringify(capture?.once ?? {});
  useEffect(() => {
    if (telling.current > 0 && told.current === capture?.id) return;
    told.current = capture?.id;
    const chosen = JSON.parse(chosenElsewhere) as Partial<Record<Way, Once>>;
    onceNow.current = chosen;
    setOnce(chosen);
  }, [capture?.id, chosenElsewhere]);
  const tell = (capture: Capture, chosen: Partial<Record<Way, Once>>) => {
    telling.current += 1;
    void choose(capture, chosen)
      .catch(() => {})
      .finally(() => {
        telling.current -= 1;
      });
  };
  const pick = (way: Way, chosen: Once) => {
    if (!capture) return;
    const next = { ...onceNow.current, [way]: { ...onceNow.current[way], ...chosen } };
    onceNow.current = next;
    setOnce(next);
    tell(capture, next);
  };
  // A window that goes out of sight with a choice open comes back without it.
  useEffect(() => {
    const hidden = () => {
      if (document.hidden) setChoosing(null);
    };
    document.addEventListener("visibilitychange", hidden);
    return () => document.removeEventListener("visibilitychange", hidden);
  }, []);
  useEffect(() => {
    if (!choosing) return;
    const away = (event: PointerEvent) => {
      // (Opened by the tour, it is the tour's to put away.)
      if (touring()) return;
      if (event.target instanceof Element && event.target.closest(".as")) return;
      setChoosing(null);
      // (In the small window the choice lies over the picture, and a press beside it is a press
      // on the picture: that press puts the choice away, and does not also open the large window.)
      if (large || (event.target instanceof Element && event.target.closest(".actions"))) return;
      const swallow = (click: MouseEvent) => {
        click.preventDefault();
        click.stopPropagation();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      window.setTimeout(() => window.removeEventListener("click", swallow, true), 600);
    };
    // Escape puts the choice away, and nothing else: not what is in hand in the drawing, and
    // never the capture. (Whoever else listens for keys leaves them alone while a choice is
    // open: see `choosingNow`.)
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      setChoosing(null);
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [choosing]);

  // Whether what is on the clipboard is the capture as it is drawn now; and whether it came to be
  // so while this capture was on screen (a copy just made, which is worth a flourish), not with it.
  // (And as what it would be copied as now: with another kind of file or another size chosen,
  // what is there is not that, and copying is on offer again.)
  const copyAs = view ? { ...view.leaves.copy, ...once.copy } : null;
  const copiedAsAsked =
    capture !== null &&
    copyAs !== null &&
    capture.copiedAs.format === copyAs.format &&
    capture.copiedAs.fit === copyAs.fit;
  const copiedNow = capture !== null && capture.copied && !capture.copyIsOld && copiedAsAsked;
  // (A copy just made is one the application says was made: what is on the clipboard came to be
  // the capture as it is, or another kind or size of it. Not a choice made here that happens to
  // be what is there already.)
  const there =
    capture !== null && capture.copied && !capture.copyIsOld
      ? `${capture.copiedAs.format} ${capture.copiedAs.fit}`
      : null;
  const copiedBefore = useRef({ id: capture?.id, there });
  const [ticked, setTicked] = useState(false);
  useEffect(() => {
    const before = copiedBefore.current;
    if (before.id !== capture?.id || there === null) setTicked(false);
    else if (before.there !== there) setTicked(copiedNow);
    copiedBefore.current = { id: capture?.id, there };
  }, [capture?.id, there]);
  useEffect(() => {
    if (!copiedNow) setTicked(false);
  }, [copiedNow]);

  // Each capture starts clean.
  useEffect(() => {
    setChoosing(null);
    setDrawn(null);
    setSaveFailure(null);
    setFull(false);
    setUnmade(false);
    setCopyFailure(null);
  }, [capture?.id]);

  // While the capture is on its way out (the image of it is being made, the application asked):
  // nothing more is drawn on it and nothing else is asked, so that what leaves is what was on
  // screen when it was asked for.
  const [busy, setBusy] = useState(false);
  const leavingNow = useRef(false);
  /** Has the capture leave by `ask`, with what the large window makes of it. `failed` is told a
      refusal that has no words of its own. With `closes` (the capture is uploaded or saved, and the
      preview closes), the large window is out of sight from this moment on: what it still has to
      make takes a moment, and is not to be waited for with the window open. */
  const leave = (
    ask: (drawing?: Leaving) => Promise<void>,
    failed: (detail: string) => void,
    closes?: { capture: Capture; to: "upload" | "save" },
  ) => {
    if (leavingNow.current) return;
    leavingNow.current = true;
    setBusy(true);
    const gone = closes && large ? vanish(closes.capture, closes.to).then(() => true, () => false) : Promise.resolve(false);
    gone
      .then(() => leaving())
      .then((drawing) => (drawing === "stay" ? undefined : ask(drawing).then(() => setCopyFailure(null))))
      .catch((detail: unknown) => {
        const said = detail instanceof Error ? detail.message : String(detail);
        if (said === "tooMuch") setFull(true);
        else if (said === "notMade" || said === "notAsked" || detail instanceof Error) setUnmade(true);
        else failed(said);
      })
      .finally(() => {
        leavingNow.current = false;
        setBusy(false);
        // (In sight again, should the window still be there: nothing left, and it says why.)
        void gone.then((was) => {
          if (was) appear().catch(() => {});
        });
      });
  };
  const saveIt = (capture: Capture) =>
    leave((drawing) => save(capture, drawing, onceNow.current.save), setSaveFailure, { capture, to: "save" });
  const uploadIt = (capture: Capture) =>
    leave((drawing) => upload(capture, drawing, onceNow.current.upload), setSaveFailure, { capture, to: "upload" });
  const copyIt = (capture: Capture) =>
    leave((drawing) => copy(capture, drawing, onceNow.current.copy).then(() => setUnmade(false)), setCopyFailure);

  // The large window has the keyboard: Ctrl+Enter uploads (or saves, when uploading is not on
  // offer), Ctrl+S saves, Escape discards. The small one never has it. (Enter alone leaves
  // nothing: it is a key of what is written on the capture, and pressed once too often there.)
  useEffect(() => {
    if (!large || !capture || capture.progress === "uploading") return;
    const onKey = (event: KeyboardEvent) => {
      // (A key that only goes into letters being put together is not a key to act on. Nor is one
      // pressed while the capture is on its way out: the window is out of sight then.)
      if (event.repeat || event.isComposing || leavingNow.current || choosingNow() || touring()) return;
      if (event.key === "Escape") void discard(capture);
      if (event.ctrlKey && event.key.toLowerCase() === "s") saveIt(capture);
      // (Ctrl+C is not the capture's: it copies what is selected of what is drawn on it. The
      // capture is copied by its button.)
      // On a button the key is that button's: it means "upload" (or "save") only when nothing
      // else has it.
      const onControl =
        event.target instanceof HTMLElement && event.target.closest("button, input, textarea") !== null;
      if (event.key === "Enter" && event.ctrlKey && !event.altKey && !event.shiftKey && !onControl)
        (offersUpload ? uploadIt : saveIt)(capture);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [capture?.id, capture?.progress, offersUpload]);

  // The tour of the window: of the large one where the capture is drawn on, or of the small one.
  // (Not of the small one while it only shows where it appears.)
  const tour = useTour(
    large ? "editor" : "preview",
    view?.toursSeen,
    !!view?.capture && view.capture.progress !== "uploading" && !sampling,
  );
  useEffect(() => {
    if (!tour.on) return;
    setChoosing(null);
    // (Shown, something is being done about the capture: the application does not give it up on
    // its own meanwhile.)
    if (!large && capture) tell(capture, onceNow.current);
  }, [tour.on, capture?.id]);

  // While it uploads the windows are hidden; there is nothing to draw then either.
  if (!view || !capture || capture.progress === "uploading") return null;
  const t = translator(view.texts);
  /** One of the things to do with the capture: its name on it in the small window, its icon alone
      in the large one, with the name and the key that does the same in a tooltip. */
  /** What the capture leaves as, that way: asked for this once, or as the settings say. */
  const as = (way: Way): Required<Once> => ({ ...view.leaves[way], ...once[way] });
  const said = (way: Way) => {
    const { format, formats, fit } = as(way);
    // (Saving: every kind that is saved, one file of each.)
    const kind = (way === "save" ? formats : [format]).map((one) => t(`main.output.format.${one}`)).join(" + ");
    return fit === 0 ? kind : `${kind}, ${t("main.output.fit.within", { side: fit })}`;
  };
  /** Beside one of the things to do: a small arrow that opens the choice of what the capture
      leaves as this once (the kind of file, how large at most). A list under the row in the large
      window; in the small one, which has no room for a list, two rows over the row. */
  const choice = (way: Way, name: string) => (
      <div className="as" data-tour="as">
        <button
          className={choosing === way ? "narrow chevron open" : "narrow chevron"}
          title={t("preview.as", { what: name })}
          aria-label={t("preview.as", { what: name })}
          aria-haspopup="menu"
          aria-expanded={choosing === way}
          onClick={() => {
            setChoosing(choosing === way ? null : way);
            // (Opened, something is being done about the capture: the application is told, and
            // does not give it up on its own meanwhile.)
            if (choosing !== way && capture) tell(capture, onceNow.current);
          }}
        >
          <span className="icon" aria-hidden="true">
            {"\uE70D"}
          </span>
        </button>
        {choosing === way && (
          <div className="menu" role="menu" aria-label={t("preview.as", { what: name })} data-tour="menu">
            <p>{t("main.output.format")}</p>
            {view.formats.map((format) =>
              way === "save" ? (
                // Saved as several kinds at once, one file of each: ticked, not chosen. The last
                // one ticked stays so.
                <button
                  key={format}
                  role="menuitemcheckbox"
                  aria-checked={as(way).formats.includes(format)}
                  onClick={() => {
                    const now = as(way).formats;
                    const formats = view.formats.filter((one) => now.includes(one) !== (one === format));
                    if (formats.length > 0) pick(way, { formats });
                  }}
                >
                  {t(`main.output.format.${format}`)}
                </button>
              ) : (
                <button
                  key={format}
                  role="menuitemradio"
                  aria-checked={as(way).format === format}
                  onClick={() => pick(way, { format })}
                >
                  {t(`main.output.format.${format}`)}
                </button>
              ),
            )}
            <p>{t("main.output.fit")}</p>
            {[0, ...view.fits].map((fit) => (
              <button
                key={fit}
                role="menuitemradio"
                aria-checked={as(way).fit === fit}
                // (In the small window there is room for the number alone: the rest is its tooltip.)
                title={large ? undefined : fit === 0 ? t("main.output.fit.none") : t("main.output.fit.within", { side: fit })}
                onClick={() => pick(way, { fit })}
              >
                {!large
                  ? fit === 0
                    ? t("preview.fit.none")
                    : String(fit)
                  : fit === 0
                    ? t("main.output.fit.none")
                    : t("main.output.fit.within", { side: fit })}
              </button>
            ))}
          </div>
        )}
      </div>
  );
  const act = (look: string, icon: string, name: string, key: string, onClick: () => void, way: Way) =>
    large ? (
      <button
        className={`narrow ${look}`.trim()}
        title={`${name} (${key}) · ${said(way)}`}
        aria-label={name}
        onClick={onClick}
      >
        <span className="icon" aria-hidden="true">
          {icon}
        </span>
      </button>
    ) : (
      <button className={look || undefined} title={`${name} · ${said(way)}`} aria-label={name} onClick={onClick}>
        {name}
      </button>
    );

  return (
    <main className={large ? "preview large" : sampling ? "preview small sample" : "preview small"}>
      {large ? (
        <Editor
          // (Another capture is another drawing, from nothing: nothing of the one before is shown.)
          key={capture.id}
          capture={capture}
          settle={settle}
          show={show}
          beside={beside}
          frozen={busy}
          defaultHider={view?.defaultHider ?? "last"}
          t={t}
          onCount={setDrawn}
          onEdits={(edits) => {
            setDrawn(edits.length);
            setFull(false);
            setUnmade(false);
          }}
          onFull={() => setFull(true)}
          onUnmade={() => setUnmade(true)}
        />
      ) : sampling ? (
        <div className="picture">
          <p>{t("preview.here")}</p>
        </div>
      ) : (
        <Picture capture={capture} t={t} />
      )}
      <div className="below">
        <Status
          capture={copiedAsAsked ? capture : { ...capture, copied: false }}
          saveFailure={saveFailure}
          copyFailure={copyFailure}
          full={full}
          unmade={unmade}
          offersUpload={offersUpload}
          drawn={drawn ?? capture.edits.length}
          t={t}
        />
        {large && <div className="beside" ref={setBeside} />}
      </div>
      {large && <TourButton onClick={tour.start} t={t} />}
      <div
        className="actions"
        data-tour="actions"
        // A press on one of these does not take the keyboard: after copying, the keys still mean
        // what they meant.
        onMouseDown={(event) => event.preventDefault()}
      >
        {!large && <TourButton className="narrow" onClick={tour.start} t={t} />}
        {/* In the large window each is its icon alone, in a row at the top right; what it is and
            the key that does the same are in its tooltip. In the small one uploading and saving are
            spelled out. */}
        {offersUpload && choice("upload", t("preview.upload"))}
        {offersUpload &&
          act(
            "primary",
            "\uE898",
            t(capture.progress === "failed" ? "preview.retry" : "preview.upload"),
            "Ctrl+Enter",
            () => uploadIt(capture),
            "upload",
          )}
        {choice("save", t("preview.save"))}
        {act(
          offersUpload ? "" : "primary",
          "\uE74E",
          t("preview.save"),
          offersUpload ? "Ctrl+S" : "Ctrl+Enter",
          () => saveIt(capture),
          "save",
        )}
        {choice("copy", t("preview.copy"))}
        {/* A tick, while what is on the clipboard is the capture as it is drawn now. */}
        <button
          className="narrow"
          title={copiedNow ? t("preview.copied") : large ? `${t("preview.copy")} · ${said("copy")}` : t("preview.copy")}
          aria-label={t("preview.copy")}
          onClick={() => copyIt(capture)}
        >
          {/* Both are there, one over the other: copied, the sheets shrink away and the tick
              comes in their place (with a bounce, when it comes from a copy made just now). */}
          <span className={`swap${copiedNow ? " done" : ""}${ticked ? " ticked" : ""}`} aria-hidden="true">
            <span className="icon was">{"\uE8C8"}</span>
            <span className="icon good now">{"\uE73E"}</span>
          </span>
        </button>
        {/* (A bin in both windows.) */}
        <button
          className="narrow away"
          title={large ? `${t("preview.discard")} (Esc)` : t("preview.discard")}
          aria-label={t("preview.discard")}
          onClick={() => void discard(capture)}
        >
          <span className="icon" aria-hidden="true">
            {"\uE74D"}
          </span>
        </button>
      </div>
      {tour.on && (
        <Tour
          steps={large ? editorSteps(offersUpload, show, setChoosing, t) : previewSteps(offersUpload, setChoosing, t)}
          t={t}
          onEnd={tour.end}
        />
      )}
    </main>
  );
}

/** Something said that changes with `what` it is about: what was said before fades out where it
    is, and what is said now fades in, coming from the left. As it is first said, it is just there.
    (Without the motion where the system asks for less of it.) */
function Changing({ what, children }: { what: string; children: React.ReactNode }) {
  const [now, setNow] = useState(what);
  const [before, setBefore] = useState<React.ReactNode>(null);
  const [coming, setComing] = useState(false);
  const said = useRef<React.ReactNode>(children);
  if (now !== what) {
    setNow(what);
    if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setBefore(said.current);
      setComing(true);
    }
  }
  useEffect(() => {
    said.current = children;
  });
  useEffect(() => {
    if (before === null) return;
    const gone = window.setTimeout(() => setBefore(null), 220);
    return () => window.clearTimeout(gone);
  }, [before]);
  return (
    <span className="changing">
      {before !== null && (
        <span className="before" aria-hidden="true">
          {before}
        </span>
      )}
      <span key={now} className={coming ? "coming" : undefined} onAnimationEnd={() => setComing(false)}>
        {children}
      </span>
    </span>
  );
}

/** What the small window shows in place of a capture while it only shows where it appears: of
    the usual shape, and on the clipboard as the settings would have it. */
const sampleOf = (view: View): Capture => ({
  id: 0,
  width: 1920,
  height: 1080,
  keptWidth: 1920,
  keptHeight: 1080,
  progress: "waiting",
  message: null,
  copied: true,
  copyFailure: null,
  edits: [],
  copyIsOld: false,
  copiedAs: { format: view.leaves.copy.format, fit: view.leaves.copy.fit },
  thumbnail: 0,
});

/** The picture in the corner: the way to the large window, where it is seen whole and drawn on. */
function Picture({ capture, t }: { capture: Capture; t: Translate }) {
  // Whether the pointer is over the picture, as the pointer itself says: the window is hidden
  // under it when the picture is clicked, and is never told that the pointer left. So a click
  // ends it, and what the pointer does in the moment the window takes to go is not listened to;
  // nor is it over the picture of another capture until it moves there.
  const [over, setOver] = useState(false);
  const clicked = useRef(-Infinity);
  useEffect(() => setOver(false), [capture.id]);
  const moved = () => {
    if (performance.now() - clicked.current > 500) setOver(true);
  };
  return (
    <button
      className="picture"
      data-tour="picture"
      aria-label={t("preview.clickToEdit")}
      onPointerEnter={moved}
      onPointerMove={moved}
      onPointerLeave={() => setOver(false)}
      onClick={() => {
        clicked.current = performance.now();
        setOver(false);
        void enlarge(capture);
      }}
    >
      <img
        src={imageUrl(capture, "thumbnail")}
        alt={t("preview.image", { width: capture.keptWidth, height: capture.keptHeight })}
        draggable={false}
      />
      {/* With the pointer over it, the picture darkens and says what a click does: a pen, and
          the words. */}
      <span className={over ? "over shown" : "over"} aria-hidden="true">
        <span className="icon">{""}</span>
        {t("preview.clickToEdit")}
      </span>
    </button>
  );
}

function Status({
  capture,
  saveFailure,
  copyFailure,
  full,
  unmade,
  offersUpload,
  drawn,
  t,
}: {
  capture: Capture;
  saveFailure: string | null;
  copyFailure: string | null;
  full: boolean;
  unmade: boolean;
  offersUpload: boolean;
  /** How many things are drawn on the capture. */
  drawn: number;
  t: Translate;
}) {
  // What went wrong last, if anything did: saving just now, the upload before that, the copy to the
  // clipboard when the preview appeared.
  const failure = full
    ? // (What can be done with it as it is depends on whether uploading is on offer.)
      t(offersUpload ? "editor.full" : "editor.fullWithoutUpload")
    : unmade
      ? t("editor.unmade")
      : saveFailure !== null
      ? t("preview.saveFailed", { detail: saveFailure })
      : copyFailure !== null
        ? t("preview.copyFailed", { detail: copyFailure })
        : (capture.message ??
          (capture.copyFailure !== null
            ? t("preview.copyFailed", { detail: capture.copyFailure })
            : null));

  // Drawn on, the capture on the clipboard is not the one on screen: said plainly, until saving or
  // uploading puts the drawn one (or its link) there.
  // (Each with its mark, in its colour: a tick for what is there as it is, a cross for what is
  // not there any more.)
  const copied =
    capture.copied &&
    (capture.copyIsOld ? (
      <span className="bad">
        <span className="icon mark" aria-hidden="true">
          {""}
        </span>
        {t("preview.copiedBefore")}
      </span>
    ) : (
      <span className="good">
        <span className="icon mark" aria-hidden="true">
          {""}
        </span>
        {t("preview.copied")}
      </span>
    ));

  // (Said anew, it is seen to be: what was said fades where it is, and what is said now comes in
  // from the left. Not from one capture to the next, which starts with what it has to say.)
  const changing = (
    <Changing key={capture.id} what={!capture.copied ? "nothing" : capture.copyIsOld ? "old" : "there"}>
      {copied}
    </Changing>
  );

  // The large window has two lines: what there is to say (what went wrong, what is on the
  // clipboard), and under it what the capture is: how large, and how much is drawn on it of how much
  // a drawing can hold.
  if (large) {
    return (
      <div className="lines" data-tour="status">
        {failure ? (
          <p className="status bad" role="alert" title={failure}>
            {failure}
          </p>
        ) : (
          <p className="status">{changing}</p>
        )}
        <p className="status about">
          {capture.keptWidth} × {capture.keptHeight}
          {" · "}
          {t("editor.layers", { count: drawn, most: MOST_EDITS })}
        </p>
      </div>
    );
  }

  if (failure) {
    return (
      <p className="status bad" role="alert" title={failure}>
        {failure}
      </p>
    );
  }
  return (
    <p className="status">
      {changing}
      {capture.copied && " · "}
      {capture.keptWidth} × {capture.keptHeight}
    </p>
  );
}
