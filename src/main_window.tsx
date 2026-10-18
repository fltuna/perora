// The main window: a list of pages on the left, the chosen page on the right. Everything it shows is
// asked of the application, and what is set here takes effect at once: there is nothing to save.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { translator } from "./backend";

interface View {
  language: string;
  languages: { code: string; name: string }[];
  texts: Record<string, string>;
  account: {
    state: "signedOut" | "signingIn" | "signedIn";
    name: string | null;
    /** Whether this account may upload; null when the server could not be asked. */
    mayUpload: boolean | null;
  };
  settings: {
    uploadImmediately: boolean;
    enableUpload: boolean;
    startMode: string;
    /** Whether Perora's own windows are hidden before the screen is captured. */
    hideOwnWindows: boolean;
    copyToClipboard: boolean;
    clearClipboardOnDiscard: boolean;
    saveDirectory: string;
    saveDirectoryIsDefault: boolean;
    askWhereToSave: boolean;
    /** Whether every capture is saved as it was taken, and whether discarding takes that away;
        whether it is saved as kinds of file and a size of its own, and those. */
    saveOnCapture: boolean;
    deleteSavedOnDiscard: boolean;
    saveOnCaptureCustom: boolean;
    saveOnCaptureFormats: string[];
    saveOnCaptureFit: number;
    /** `system`, `light` or `dark`. */
    theme: string;
    /** The kind of file a capture leaves as, each way it leaves (`png`, `jpg` or `webp`), and the
        square it is made to fit (pixels a side; 0 for as large as it is). */
    copyFormat: string;
    copyFit: number;
    uploadFormat: string;
    uploadFit: number;
    saveFormat: string;
    saveFit: number;
    /** The kinds of file a capture is saved as, one file of each. */
    saveFormats: string[];
    /** How good a file that loses something is. */
    quality: string;
    startAtSignIn: boolean;
    startMinimized: boolean;
    /** How long a notice stays, in seconds; 0 for until it is clicked. */
    noticeSeconds: number;
    noticeCorner: string;
    /** What becomes of a capture once it is taken: "preview", "edit" or "copy". */
    afterCapture: string;
    /** Which way of making a part unreadable the editor starts with: "last", "mosaic" or "blur". */
    defaultHider: string;
    /** How long the small preview stays, in seconds; 0 for until something is done with it. */
    previewSeconds: number;
    previewCorner: string;
    /** Each of the sounds: `none`, `custom`, or `system:` and the name of one of the system's;
        and the file of the user's own that can be that sound, when one was chosen. */
    noticeSound: string;
    noticeSoundFile: string | null;
    captureSound: string;
    captureSoundFile: string | null;
  };
  /** The modes the selection screen can start in. */
  modes: string[];
  /** The kinds of file a capture can leave as, the squares it can be made to fit (pixels a side),
      and how good a file that loses something can be. */
  formats: string[];
  fits: number[];
  qualities: string[];
  /** The looks the windows can have. */
  looks: string[];
  /** The corners of the screen a notice can appear in. */
  corners: string[];
  /** The sounds the system comes with, by name. */
  sounds: string[];
  shortcut: {
    label: string;
    registered: boolean;
    /** The system's own combination for snipping (Win+Shift+S): whether it is to capture with
        Perora, whether it does, and whether the system's shell has to start anew first. */
    takeSnipping: boolean;
    snippingRegistered: boolean;
    shellIsBehind: boolean;
    /** The shell is being started anew. */
    shellRestarting: boolean;
  };
  version: string;
  update: {
    /** The newer version there is, if one was found. */
    available: string | null;
    /** Whether the band at the top shows it (not when skipped, or closed this run). */
    band: boolean;
    /** What the last look the user asked for came to. */
    asked: "nothing" | "looking" | "upToDate" | "failed";
    /** Being fetched: bytes so far, and of how many when known; or being installed. */
    got: number | null;
    of: number | null;
    installing: boolean;
    /** Installing would throw away the capture in the preview: it waits until that is done with. */
    captureInHand: boolean;
  };
  /** The colour the user chose for their system, as #rrggbb. */
  accent: string | null;
}

interface Upload {
  id: string;
  url: string;
  imageUrl: string;
  width: number;
  height: number;
  createdAt: string;
}

type Translate = ReturnType<typeof translator>;
type Page = "home" | "capture" | "save" | "editor" | "shortcut" | "general" | "account";
const PAGES: Page[] = ["home", "capture", "save", "editor", "shortcut", "general", "account"];

// Glyphs of "Segoe Fluent Icons", the icon font Windows 11 has ("Segoe MDL2 Assets" on Windows 10
// has the same ones at the same places).
const ICONS: Record<Page | "camera" | "copy" | "done", string> = {
  home: "",
  capture: "",
  save: "",
  editor: "\uE70F",
  shortcut: "",
  general: "",
  account: "",
  camera: "",
  copy: "",
  done: "",
};

/** The state of the application, kept up to date; and a number that grows whenever it changed, for
    what has to be fetched separately. */
function useMain(): [View | null, number] {
  const [view, setView] = useState<View | null>(null);
  const [changes, setChanges] = useState(0);

  useEffect(() => {
    let stopped = false;
    const load = () =>
      invoke<View>("main_state").then((next) => {
        if (stopped) return;
        setView(next);
        setChanges((count) => count + 1);
      });

    void load();
    const listening = listen("main-changed", () => void load());
    // The accent colour, and the uploads, can change while the window is out of sight.
    window.addEventListener("focus", load);
    return () => {
      stopped = true;
      window.removeEventListener("focus", load);
      void listening.then((stop) => stop());
    };
  }, []);

  return [view, changes];
}

export function MainWindow() {
  const [view, changes] = useMain();
  const [page, setPage] = useState<Page | null>(null);
  const drawn = view !== null;

  useEffect(() => {
    if (!view) return;
    document.documentElement.lang = view.language;
    if (view.accent) document.documentElement.style.setProperty("--system-accent", view.accent);
  }, [view?.language, view?.accent]);

  // Shown only once there is something to show.
  useEffect(() => {
    if (drawn) void invoke("main_ready");
  }, [drawn]);

  if (!view) return null;
  const t = translator(view.texts);
  const current = page ?? "home";

  return (
    <div className="frame">
      <UpdateBand view={view} t={t} />
      <div className="main">
        <nav className="sidebar" aria-label="Perora">
          <ul>
            {PAGES.map((item) => (
              <li key={item}>
                <button
                  className={item === current ? "current" : undefined}
                  aria-current={item === current ? "page" : undefined}
                  onClick={() => setPage(item)}
                >
                  <span className="icon" aria-hidden="true">
                    {ICONS[item]}
                  </span>
                  {t(`main.nav.${item}`)}
                </button>
              </li>
            ))}
          </ul>
          <p className="version">{t("main.about.version", { version: view.version })}</p>
        </nav>
        <main className="page">
          <h1>{t(`main.nav.${current}`)}</h1>
          {current === "home" && <Home view={view} t={t} />}
          {current === "capture" && <Capture view={view} t={t} />}
          {current === "save" && <Save view={view} t={t} />}
          {current === "editor" && <Editing view={view} t={t} />}
          {current === "shortcut" && <Shortcut view={view} t={t} />}
          {current === "general" && <General view={view} t={t} />}
          {current === "account" && <Account view={view} changes={changes} t={t} />}
        </main>
      </div>
    </div>
  );
}

/** Installs the newer version: Perora closes, and the new one starts in its place. What could not
    be done is said (`failed`). */
function install(failed: (reason: string) => void) {
  invoke("main_update_install").catch((reason) => failed(String(reason)));
}

/** A newer version, across the top of the window, whichever page is open: to install now, to skip,
    or to put away until Perora starts again. While it is fetched, how far that is. */
function UpdateBand({ view, t }: { view: View; t: Translate }) {
  const update = view.update;
  const [failure, setFailure] = useState<string | null>(null);
  // (What failed is said until the next try, or until another version is offered.)
  useEffect(() => setFailure(null), [update.available]);
  if (!update.band || !update.available) return null;

  const getting = update.got !== null || update.installing;
  let said: string | null = failure;
  if (update.installing) said = t("main.update.installing");
  else if (update.got !== null)
    said = update.of
      ? t("main.update.downloading", {
          percent: String(Math.min(100, Math.floor((update.got / update.of) * 100))),
        })
      : t("main.update.downloadingSome");
  else if (update.captureInHand) said = t("main.update.captureInHand");

  return (
    <div className="band" role="status">
      <span className="icon" aria-hidden="true">
        {""}
      </span>
      <div className="words">
        <strong>{t("main.update.title", { version: update.available })}</strong>
        {said && <span>{said}</span>}
      </div>
      {!getting && (
        <div className="buttons">
          <button
            className="accent"
            disabled={update.captureInHand}
            onClick={() => {
              setFailure(null);
              install(setFailure);
            }}
          >
            {t("main.update.install")}
          </button>
          <button onClick={() => void invoke("main_update_skip", { version: update.available })}>
            {t("main.update.skip")}
          </button>
          <button
            className="close"
            aria-label={t("main.update.close")}
            title={t("main.update.close")}
            onClick={() => void invoke("main_update_close", { version: update.available })}
          >
            <span aria-hidden="true">{""}</span>
          </button>
        </div>
      )}
    </div>
  );
}

const set = (setting: string, value: boolean | string | number | string[]) =>
  void invoke("main_set", { setting, value });

/** Capturing, and nothing else: that is what Perora is for. */
function Home({ view, t }: { view: View; t: Translate }) {
  const { shortcut } = view;
  return (
    <>
      <section className="card hero">
        <div>
          <p className="hint">{t("main.home.captureHint", { shortcut: shortcut.label })}</p>
        </div>
        <button className="accent" onClick={() => void invoke("main_capture")}>
          <span className="icon" aria-hidden="true">
            {ICONS.camera}
          </span>
          {t("main.home.capture")}
        </button>
      </section>
      {!shortcut.registered && (
        <p className="notice warning" role="status">
          {t("main.home.shortcutTaken", { shortcut: shortcut.label })}
        </p>
      )}
    </>
  );
}

/** The user's latest uploads, on the account page: asked for when the page appears and whenever
    something changed (an upload finished, the window came back to the front). */
function Recent({ language, changes, t }: { language: string; changes: number; t: Translate }) {
  const [uploads, setUploads] = useState<Upload[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    let stopped = false;
    invoke<Upload[]>("main_recent").then(
      (recent) => {
        if (stopped) return;
        setUploads(recent);
        setFailure(null);
      },
      (reason: unknown) => {
        if (!stopped) setFailure(String(reason));
      },
    );
    return () => {
      stopped = true;
    };
  }, [changes]);

  const copy = (upload: Upload) => {
    void invoke("main_copy_link", { url: upload.url }).then(() => {
      setCopied(upload.id);
      window.setTimeout(() => setCopied((id) => (id === upload.id ? null : id)), 1600);
    });
  };
  const when = (upload: Upload) => {
    const date = new Date(upload.createdAt);
    return Number.isNaN(date.getTime())
      ? ""
      : date.toLocaleString(language, { dateStyle: "medium", timeStyle: "short" });
  };

  return (
    <section className="recent">
      <h2>{t("main.account.recent")}</h2>
      {failure !== null && uploads === null && (
        <p className="notice">{t("main.account.recentUnreachable")}</p>
      )}
      {failure === null && uploads === null && <span className="ring" role="progressbar" />}
      {uploads !== null && uploads.length === 0 && <p className="hint">{t("main.account.recentNone")}</p>}
      {uploads !== null && uploads.length > 0 && (
        <ul>
          {uploads.map((upload) => (
            <li key={upload.id}>
              <button
                className="shot"
                title={t("main.account.openPage")}
                onClick={() => void invoke("main_open_page", { url: upload.url })}
              >
                <img src={upload.imageUrl} alt="" loading="lazy" draggable={false} />
              </button>
              <div className="about">
                <span className="when">{when(upload)}</span>
                <button
                  className="icon-button"
                  title={t("main.account.copyLink")}
                  aria-label={t("main.account.copyLink")}
                  onClick={() => copy(upload)}
                >
                  <span className="icon" aria-hidden="true">
                    {copied === upload.id ? ICONS.done : ICONS.copy}
                  </span>
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function Capture({ view, t }: { view: View; t: Translate }) {
  const { settings } = view;
  // Uploading is an extra, to be switched on: which takes a sign-in. What goes with it is shown
  // under it, while it is on.
  const signedIn = view.account.state === "signedIn";
  const uploads = signedIn && settings.enableUpload;
  // Captures are copied: as they are shown, or straight after being taken (then with no preview,
  // and nothing to discard).
  const copies = settings.copyToClipboard || settings.afterCapture === "copy";
  return (
    <>
      <Section title={t("main.capture.section.general")} />
      <Setting title={t("main.capture.startMode")} hint={t("main.capture.startModeHint")}>
        <select
          value={settings.startMode}
          aria-label={t("main.capture.startMode")}
          onChange={(event) => set("startMode", event.target.value)}
        >
          {view.modes.map((mode) => (
            <option key={mode} value={mode}>
              {t(`overlay.mode.${mode}`)}
            </option>
          ))}
        </select>
      </Setting>
      <Setting title={t("main.capture.hideOwnWindows")} hint={t("main.capture.hideOwnWindowsHint")}>
        <Switch
          label={t("main.capture.hideOwnWindows")}
          on={settings.hideOwnWindows}
          onChange={(on) => set("hideOwnWindows", on)}
        />
      </Setting>
      <Setting title={t("main.capture.copy")} hint={t("main.capture.copyHint")}>
        <Switch
          label={t("main.capture.copy")}
          on={settings.copyToClipboard}
          disabled={settings.afterCapture === "copy"}
          onChange={(on) => set("copyToClipboard", on)}
        />
      </Setting>
      {/* What goes with copying, under it: shown while captures are copied (owner, 2026-10-14). */}
      <Reveal shown={copies}>
        <Leaving view={view} t={t} of="copy" under />
        <Setting under title={t("main.capture.clear")} hint={t("main.capture.clearHint")}>
          <Switch
            label={t("main.capture.clear")}
            on={settings.clearClipboardOnDiscard}
            disabled={settings.afterCapture === "copy"}
            onChange={(on) => set("clearClipboardOnDiscard", on)}
          />
        </Setting>
      </Reveal>
      <Section title={t("main.capture.section.after")} />
      <Setting title={t("main.capture.after")} hint={t("main.capture.afterHint")}>
        <select
          value={settings.afterCapture}
          aria-label={t("main.capture.after")}
          onChange={(event) => set("afterCapture", event.target.value)}
        >
          {["preview", "edit", "copy"].map((after) => (
            <option key={after} value={after}>
              {t(`main.capture.after.${after}`)}
            </option>
          ))}
        </select>
      </Setting>
      <Reveal shown={settings.afterCapture === "preview"}>
          <Setting under title={t("main.capture.previewSeconds")} hint={t("main.capture.previewSecondsHint")}>
            <Seconds
              label={t("main.capture.previewSeconds")}
              unit={t("main.general.seconds")}
              value={settings.previewSeconds}
              onChange={(seconds) => set("previewSeconds", seconds)}
            />
          </Setting>
          <Setting under title={t("main.capture.previewCorner")} hint={t("main.capture.previewCornerHint")}>
            <CornerChoice view={view} t={t} of="preview" label={t("main.capture.previewCorner")} />
          </Setting>
      </Reveal>
      <SoundSetting
        view={view}
        t={t}
        of="capture"
        title={t("main.capture.sound")}
        hint={t("main.capture.soundHint")}
      />
      <Section title={t("main.capture.section.other")} />
      <Setting title={t("main.output.quality")} hint={t("main.output.qualityHint")}>
        <select
          value={settings.quality}
          aria-label={t("main.output.quality")}
          onChange={(event) => set("quality", event.target.value)}
        >
          {view.qualities.map((quality) => (
            <option key={quality} value={quality}>
              {t(`main.output.quality.${quality}`)}
            </option>
          ))}
        </select>
      </Setting>
      <Setting
        title={t("main.capture.enableUpload")}
        hint={t(signedIn ? "main.capture.enableUploadHint" : "main.capture.enableUploadSignedOut")}
      >
        <Switch
          label={t("main.capture.enableUpload")}
          on={uploads}
          disabled={!signedIn}
          onChange={(on) => set("enableUpload", on)}
        />
      </Setting>
      <Reveal shown={uploads}>
        <Setting
          under
          title={t("main.capture.uploadImmediately")}
          hint={t("main.capture.uploadImmediatelyHint")}
        >
          <Switch
            label={t("main.capture.uploadImmediately")}
            on={settings.uploadImmediately}
            onChange={(on) => set("uploadImmediately", on)}
          />
        </Setting>
        <Leaving view={view} t={t} of="upload" under />
      </Reveal>
    </>
  );
}

/** How the editor is, where a capture is drawn on (owner, 2026-10-19: a page of its own). */
function Editing({ view, t }: { view: View; t: Translate }) {
  const { settings } = view;
  return (
    <>
      <Section title={t("main.editor.section.tools")} />
      <Setting title={t("main.editor.defaultHider")} hint={t("main.editor.defaultHiderHint")}>
        <select
          value={settings.defaultHider}
          aria-label={t("main.editor.defaultHider")}
          onChange={(event) => set("defaultHider", event.target.value)}
        >
          {["last", "mosaic", "blur"].map((hider) => (
            <option key={hider} value={hider}>
              {t(`main.editor.defaultHider.${hider}`)}
            </option>
          ))}
        </select>
      </Setting>
    </>
  );
}

function Save({ view, t }: { view: View; t: Translate }) {
  const { settings } = view;
  return (
    <>
      <Section title={t("main.save.section.place")} />
      <section className="card">
        <div>
          <h2>{t("main.save.folder")}</h2>
          <p className="hint path" title={settings.saveDirectory}>
            {settings.saveDirectory}
          </p>
        </div>
        <div className="buttons">
          {!settings.saveDirectoryIsDefault && (
            <button onClick={() => void invoke("main_choose_save_directory", { reset: true })}>
              {t("main.save.reset")}
            </button>
          )}
          <button onClick={() => void invoke("main_choose_save_directory", { reset: false })}>
            {t("main.save.choose")}
          </button>
        </div>
      </section>
      <Setting title={t("main.save.ask")} hint={t("main.save.askHint")}>
        <Switch
          label={t("main.save.ask")}
          on={settings.askWhereToSave}
          onChange={(on) => set("askWhereToSave", on)}
        />
      </Setting>
      <Section title={t("main.save.section.detail")} />
      <Leaving view={view} t={t} of="save" />
      <Section title={t("main.save.section.onCapture")} />
      <Setting title={t("main.save.onCapture")} hint={t("main.save.onCaptureHint")}>
        <Switch
          label={t("main.save.onCapture")}
          on={settings.saveOnCapture}
          onChange={(on) => set("saveOnCapture", on)}
        />
      </Setting>
      {/* What goes with it, under it, while it is on: what the files are (as saving has them, or
          their own, chosen under that), and whether discarding takes them away (owner, 2026-10-16). */}
      <Reveal shown={settings.saveOnCapture}>
        <Setting under title={t("main.save.onCaptureAs")} hint={t("main.save.onCaptureAsHint")}>
          <select
            value={settings.saveOnCaptureCustom ? "custom" : "save"}
            aria-label={t("main.save.onCaptureAs")}
            onChange={(event) => set("saveOnCaptureCustom", event.target.value === "custom")}
          >
            <option value="save">{t("main.save.onCaptureAs.save")}</option>
            <option value="custom">{t("main.save.onCaptureAs.custom")}</option>
          </select>
        </Setting>
        <Reveal shown={settings.saveOnCaptureCustom}>
          <Leaving view={view} t={t} of="saveOnCapture" under />
        </Reveal>
        <Setting under title={t("main.save.deleteOnDiscard")} hint={t("main.save.deleteOnDiscardHint")}>
          <Switch
            label={t("main.save.deleteOnDiscard")}
            on={settings.deleteSavedOnDiscard}
            onChange={(on) => set("deleteSavedOnDiscard", on)}
          />
        </Setting>
      </Reveal>
    </>
  );
}

// The keys a global shortcut can end in, as the browser names them: the ones the application can
// register with the system. Others there are (the keys only a Japanese keyboard has, the menu key)
// are not among them, and pressing one while recording is as if nothing was pressed.
const SHORTCUT_KEYS =
  /^(Key[A-Z]|Digit\d|F([1-9]|1\d|2[0-4])|Numpad(\d|Add|Decimal|Divide|Enter|Equal|Multiply|Subtract)|Arrow(Down|Left|Right|Up)|Backquote|Backslash|BracketLeft|BracketRight|Comma|Equal|Minus|Period|Quote|Semicolon|Slash|Backspace|CapsLock|Enter|Space|Tab|Delete|End|Home|Insert|PageDown|PageUp|PrintScreen|ScrollLock|NumLock|Pause)$/;

/** The key combination of a key press, as the application names it (`Ctrl+Shift+H`); null for a
    press that is no combination to use (a modifier alone, a plain key that typing needs, or a key
    that cannot be part of a shortcut). */
function combination(event: KeyboardEvent): string | null {
  const { code } = event;
  if (!SHORTCUT_KEYS.test(code)) return null;
  const key = code.replace(/^Key/, "").replace(/^Digit/, "");
  const free = /^F\d{1,2}$/.test(key) || key === "PrintScreen";
  if (!event.ctrlKey && !event.altKey && !event.metaKey && !free) return null;
  const modifiers = [
    event.ctrlKey && "Ctrl",
    event.altKey && "Alt",
    event.shiftKey && "Shift",
    event.metaKey && "Super",
  ].filter(Boolean);
  return [...modifiers, key].join("+");
}

function Shortcut({ view, t }: { view: View; t: Translate }) {
  const { shortcut } = view;
  const [recording, setRecording] = useState(false);
  const [refusal, setRefusal] = useState<{ reason: string; shortcut: string } | null>(null);

  // While a new one is recorded the old one is not a shortcut: pressing it is an answer, not a
  // capture. Whatever ends the recording gives it back.
  useEffect(() => {
    if (!recording) return;
    const stop = () => setRecording(false);
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") return stop();
      const pressed = combination(event);
      if (pressed === null) return;
      stop();
      invoke("main_set_shortcut", { shortcut: pressed }).then(
        () => setRefusal(null),
        (reason: unknown) => setRefusal({ reason: String(reason), shortcut: pressed }),
      );
    };
    const suspended = invoke("main_suspend_shortcut", { suspended: true });
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", stop);
      // After the suspending was done, so that the two cannot arrive the other way round.
      void suspended.then(() => invoke("main_suspend_shortcut", { suspended: false }));
    };
  }, [recording]);

  return (
    <>
      <Section title={t("main.shortcut.section.keyboard")} />
      <section className="card">
        <div>
          <h2>{t("main.shortcut.capture")}</h2>
          <p className="hint">
            {recording ? t("main.shortcut.recording") : t("main.shortcut.captureHint")}
          </p>
        </div>
        <div className="buttons">
          <kbd className={recording ? "keys recording" : "keys"}>
            {recording ? "…" : shortcut.label}
          </kbd>
          {/* Not while the system's combination for snipping is to capture: that is the shortcut
              then, and this one is not asked about. */}
          <button disabled={shortcut.takeSnipping && !recording} onClick={() => setRecording(!recording)}>
            {recording ? t("main.account.cancel") : t("main.shortcut.change")}
          </button>
        </div>
      </section>
      {refusal !== null && (
        <p className="notice warning" role="alert">
          {t(refusal.reason === "taken" ? "main.shortcut.taken" : "main.shortcut.invalid", {
            shortcut: refusal.shortcut,
          })}
        </p>
      )}
      {refusal === null && !shortcut.registered && (
        <p className="notice warning" role="status">
          {t("main.home.shortcutTaken", { shortcut: shortcut.label })}
        </p>
      )}
      <Section title={t("main.shortcut.section.advanced")} />
      <Setting title={t("main.shortcut.snipping")} hint={t("main.shortcut.snippingHint")}>
        <Switch
          label={t("main.shortcut.snipping")}
          on={shortcut.takeSnipping}
          disabled={shortcut.shellRestarting}
          onChange={(on) => set("takeSnipping", on)}
        />
      </Setting>
      <Reveal shown={shortcut.shellIsBehind}>
        <section className="card under">
          <div>
            <h2>{t(shortcut.takeSnipping ? "main.shortcut.snippingWaits" : "main.shortcut.snippingGivenBack")}</h2>
            <p className="hint">{t("main.shortcut.restartShellHint")}</p>
          </div>
          {/* While the shell starts anew the button turns, and nothing here is to be pressed. */}
          <button
            className={shortcut.shellRestarting ? "turning" : undefined}
            disabled={shortcut.shellRestarting}
            aria-busy={shortcut.shellRestarting}
            onClick={() => void invoke("main_restart_shell")}
          >
            {shortcut.shellRestarting && <span className="ring" aria-hidden="true" />}
            {t("main.shortcut.restartShell")}
          </button>
        </section>
      </Reveal>
    </>
  );
}

function General({ view, t }: { view: View; t: Translate }) {
  return (
    <>
      <Section title={t("main.general.section.ui")} />
      <Setting title={t("main.general.language")} hint={t("main.general.languageHint")}>
        <select
          value={view.language}
          aria-label={t("main.general.language")}
          onChange={(event) => void invoke("main_set_language", { code: event.target.value })}
        >
          {view.languages.map((language) => (
            <option key={language.code} value={language.code}>
              {language.name}
            </option>
          ))}
        </select>
      </Setting>
      <Setting title={t("main.general.theme")} hint={t("main.general.themeHint")}>
        <select
          value={view.settings.theme}
          aria-label={t("main.general.theme")}
          onChange={(event) => set("theme", event.target.value)}
        >
          {view.looks.map((look) => (
            <option key={look} value={look}>
              {t(`main.general.theme.${look}`)}
            </option>
          ))}
        </select>
      </Setting>
      <Section title={t("main.general.section.notice")} />
      <Setting title={t("main.general.noticeSeconds")} hint={t("main.general.noticeSecondsHint")}>
        <Seconds
          label={t("main.general.noticeSeconds")}
          unit={t("main.general.seconds")}
          value={view.settings.noticeSeconds}
          onChange={(seconds) => set("noticeSeconds", seconds)}
        />
      </Setting>
      <Setting title={t("main.general.noticeCorner")} hint={t("main.general.noticeCornerHint")}>
        <CornerChoice view={view} t={t} of="notice" label={t("main.general.noticeCorner")} />
      </Setting>
      <SoundSetting
        view={view}
        t={t}
        of="notice"
        title={t("main.general.noticeSound")}
        hint={t("main.general.noticeSoundHint")}
      />
      <Section title={t("main.general.section.other")} />
      <Setting title={t("main.general.startAtSignIn")} hint={t("main.general.startAtSignInHint")}>
        <Switch
          label={t("main.general.startAtSignIn")}
          on={view.settings.startAtSignIn}
          onChange={(on) => set("startAtSignIn", on)}
        />
      </Setting>
      <Reveal shown={view.settings.startAtSignIn}>
        <Setting under title={t("main.general.startMinimized")} hint={t("main.general.startMinimizedHint")}>
          <Switch
            label={t("main.general.startMinimized")}
            on={view.settings.startMinimized}
            onChange={(on) => set("startMinimized", on)}
          />
        </Setting>
      </Reveal>
      <Section title={t("main.general.section.update")} />
      <Updates view={view} t={t} />
    </>
  );
}

/** The version this is, and whether there is a newer one: looked for when asked. A newer version
    can be installed from here too, when its band was skipped or closed. */
function Updates({ view, t }: { view: View; t: Translate }) {
  const update = view.update;
  const [failure, setFailure] = useState<string | null>(null);
  const getting = update.got !== null || update.installing;
  const looking = update.asked === "looking";

  let hint = t("main.general.versionHint");
  if (looking) hint = t("main.general.versionHint.looking");
  else if (update.available) hint = t("main.general.versionHint.available", { version: update.available });
  else if (update.asked === "upToDate") hint = t("main.general.versionHint.upToDate");
  else if (update.asked === "failed") hint = t("main.general.versionHint.failed");
  if (failure) hint = failure;

  return (
    <Setting title={t("main.general.version", { version: view.version })} hint={hint}>
      <div className="buttons">
        {update.available && !getting && (
          <button
            className="accent"
            disabled={update.captureInHand}
            title={update.captureInHand ? t("main.update.captureInHand") : undefined}
            onClick={() => {
              setFailure(null);
              install(setFailure);
            }}
          >
            {t("main.update.install")}
          </button>
        )}
        <button disabled={looking || getting} onClick={() => void invoke("main_update_look")}>
          {t("main.general.check")}
        </button>
      </div>
    </Setting>
  );
}

/** The most seconds a notice can be told to stay. */
const MOST_SECONDS = 3600;

/** A number of seconds, typed: set as soon as what is typed is one (a whole number, not more than
    can be chosen). What is not one is not set, and is gone when the field is left. */
function Seconds({
  label,
  unit,
  value,
  onChange,
}: {
  label: string;
  unit: string;
  value: number;
  onChange: (seconds: number) => void;
}) {
  const [typed, setTyped] = useState(String(value));
  // What was last set from here: the application says it back a moment later, and what is typed
  // meanwhile is compared with this, not with what the application said before. Nor is the field
  // written over while it is typed into.
  const set = useRef(value);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (document.activeElement === field.current) return;
    set.current = value;
    setTyped(String(value));
  }, [value]);
  return (
    <label className="seconds">
      <input
        ref={field}
        type="number"
        inputMode="numeric"
        min={0}
        max={MOST_SECONDS}
        step={1}
        aria-label={label}
        value={typed}
        onChange={(event) => {
          const now = event.target.value;
          setTyped(now);
          if (!/^\d+$/.test(now) || Number(now) > MOST_SECONDS || Number(now) === set.current) return;
          set.current = Number(now);
          onChange(set.current);
        }}
        onBlur={() => setTyped(String(set.current))}
      />
      <span>{unit}</span>
    </label>
  );
}

/** What a capture is made into as it leaves one way (copied, uploaded, saved): the kind of file,
    and the square it is made to fit when it is larger. */
function Leaving({
  view,
  t,
  of,
  under = false,
}: {
  view: View;
  t: Translate;
  of: "copy" | "upload" | "save" | "saveOnCapture";
  under?: boolean;
}) {
  // Saved, as several kinds at once: one file of each. (Saved as it is taken, too, when that has
  // kinds of its own.)
  const formatsOf = of === "saveOnCapture" ? "saveOnCaptureFormats" : "saveFormats";
  const several = of === "save" || of === "saveOnCapture";
  // The kinds saved, as last set from here: shown at once, and what the next tick starts from
  // (two quick ticks are two kinds, not the second in place of a first that had not come back).
  // (What the application says back of an earlier tick is not shown over a later one: for a
  // moment after the last tick only what was ticked is shown, and then what the application says.)
  const [saved, setSaved] = useState(view.settings[formatsOf]);
  const latest = useRef(saved);
  latest.current = saved;
  const said = useRef(view.settings[formatsOf]);
  said.current = view.settings[formatsOf];
  const awaited = useRef(false);
  const settling = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(settling.current), []);
  useEffect(() => {
    if (!awaited.current) setSaved(view.settings[formatsOf]);
  }, [view.settings[formatsOf].join()]);
  return (
    <Setting under={under} title={t(`main.output.${of}`)} hint={t(`main.output.${of}Hint`)}>
      <div className="buttons pair">
        {several ? (
          // Saved as several kinds at once, one file of each: ticked. The last one stays ticked.
          view.formats.map((format) => (
            <label key={format} className="check">
              <input
                type="checkbox"
                checked={saved.includes(format)}
                onChange={() => {
                  const now = latest.current;
                  const next = view.formats.filter((one) => now.includes(one) !== (one === format));
                  if (next.length === 0) return;
                  latest.current = next;
                  setSaved(next);
                  awaited.current = true;
                  set(formatsOf, next);
                  window.clearTimeout(settling.current);
                  settling.current = window.setTimeout(() => {
                    awaited.current = false;
                    setSaved(said.current);
                  }, 1500);
                }}
              />
              {t(`main.output.format.${format}`)}
            </label>
          ))
        ) : (
          <select
            value={view.settings[`${of as "copy" | "upload"}Format`]}
            aria-label={t("main.output.format")}
            onChange={(event) => set(`${of}Format`, event.target.value)}
          >
            {view.formats.map((format) => (
              <option key={format} value={format}>
                {t(`main.output.format.${format}`)}
              </option>
            ))}
          </select>
        )}
        <select
          value={view.settings[`${of}Fit`]}
          aria-label={t("main.output.fit")}
          onChange={(event) => set(`${of}Fit`, Number(event.target.value))}
        >
          <option value={0}>{t("main.output.fit.none")}</option>
          {view.fits.map((side) => (
            <option key={side} value={side}>
              {t("main.output.fit.within", { side: String(side) })}
            </option>
          ))}
        </select>
      </div>
    </Setting>
  );
}

/** The corner of the screen something appears in: a notice, or the small preview. */
function CornerChoice({
  view,
  t,
  of,
  label,
}: {
  view: View;
  t: Translate;
  of: "notice" | "preview";
  label: string;
}) {
  return (
    <select
      value={view.settings[`${of}Corner`]}
      aria-label={label}
      onChange={(event) => set(`${of}Corner`, event.target.value)}
    >
      {view.corners.map((corner) => (
        <option key={corner} value={corner}>
          {t(`main.general.corner.${corner}`)}
        </option>
      ))}
    </select>
  );
}

/** The sound something comes with (a notice, a capture): none, one of the system's, or a file of the
    user's own, which is chosen under it. */
function SoundSetting({
  view,
  t,
  of,
  title,
  hint,
}: {
  view: View;
  t: Translate;
  of: "notice" | "capture";
  title: string;
  hint: string;
}) {
  const sound = view.settings[`${of}Sound`];
  const file = view.settings[`${of}SoundFile`];
  // A sound of the system's is called what its file is, without the "Windows" they all begin with.
  const called = (sound: string) => sound.replace(/^Windows /, "");
  return (
    <>
      <Setting title={title} hint={hint}>
        <select
          value={sound}
          aria-label={title}
          onChange={(event) => set(`${of}Sound`, event.target.value)}
        >
          <option value="none">{t("main.general.sound.none")}</option>
          <option value="custom">{t("main.general.sound.custom")}</option>
          <optgroup label={t("main.general.sound.system")}>
            {view.sounds.map((sound) => (
              <option key={sound} value={`system:${sound}`}>
                {called(sound)}
              </option>
            ))}
          </optgroup>
        </select>
      </Setting>
      <Reveal shown={sound === "custom"}>
        <section className="card under">
          <div>
            <h2>{t("main.general.noticeSoundFile")}</h2>
            <p className="hint path" title={file ?? undefined}>
              {file ?? t("main.general.noticeSoundFileNone")}
            </p>
          </div>
          <button onClick={() => void invoke("main_choose_sound", { of })}>
            {t("main.general.noticeSoundChoose")}
          </button>
        </section>
      </Reveal>
    </>
  );
}

function Account({ view, changes, t }: { view: View; changes: number; t: Translate }) {
  const { account } = view;

  if (account.state === "signingIn") {
    return (
      <section className="card">
        <div>
          <h2>{t("main.account.waiting")}</h2>
          <progress aria-label={t("main.account.waiting")} />
        </div>
        <div className="buttons">
          <button onClick={() => void invoke("main_sign_in")}>{t("main.account.reopen")}</button>
          <button onClick={() => void invoke("main_cancel_sign_in")}>{t("main.account.cancel")}</button>
        </div>
      </section>
    );
  }

  if (account.state === "signedIn") {
    return (
      <>
        <section className="card">
          <div>
            <h2>
              {account.name
                ? t("main.account.signedInAs", { name: account.name })
                : t("main.account.signedIn")}
            </h2>
          </div>
          <button onClick={() => void invoke("main_sign_out")}>{t("main.account.signOut")}</button>
        </section>
        {account.mayUpload === false && (
          <p className="notice warning" role="status">
            {t("main.account.mayNotUpload")}
          </p>
        )}
        <Recent language={view.language} changes={changes} t={t} />
      </>
    );
  }

  return (
    <section className="card">
      <div>
        <h2>{t("main.account.signedOut")}</h2>
        <p className="hint">{t("main.account.signedOutHint")}</p>
      </div>
      <button className="accent" onClick={() => void invoke("main_sign_in")}>
        {t("main.account.signIn")}
      </button>
    </section>
  );
}

/** One setting: what it is on the left, the control on the right. With `under`, one that goes with
    the setting above it: set in from the left. */
/** How long what goes with a setting takes to open under it, or to close (as the stylesheet has it). */
const REVEALED_IN = 200;

/** What goes with a setting, under it, for as long as it is `shown`: it opens out as it comes
    (taller and taller, and seen more and more) and closes as it goes, so that what is under it
    moves out of the way and back, and is not suddenly elsewhere. As a page opens, it is just
    there or not. */
function Reveal({ shown, children }: { shown: boolean; children: React.ReactNode }) {
  // Whether it is in the page at all, and whether it is open: in the page a moment before it
  // opens (from nothing), and until a moment after it closed.
  const [there, setThere] = useState(shown);
  const [open, setOpen] = useState(shown);
  useEffect(() => {
    if (shown) {
      setThere(true);
      // (A frame as it is, closed, and then open: there is nothing to open out from otherwise.)
      let second = 0;
      const first = requestAnimationFrame(() => {
        second = requestAnimationFrame(() => setOpen(true));
      });
      return () => {
        cancelAnimationFrame(first);
        cancelAnimationFrame(second);
      };
    }
    setOpen(false);
    const gone = window.setTimeout(() => setThere(false), REVEALED_IN + 40);
    return () => window.clearTimeout(gone);
  }, [shown]);
  if (!there) return null;
  return (
    <div className={open ? "reveal open" : "reveal"} inert={!open}>
      <div className="within">{children}</div>
    </div>
  );
}

/** A part of a page: its name, small, over a line, and under it the settings that belong to it. */
function Section({ title }: { title: string }) {
  return <h3 className="section">{title}</h3>;
}

function Setting({
  title,
  hint,
  under = false,
  children,
}: {
  title: string;
  hint?: string;
  under?: boolean;
  children: React.ReactNode;
}) {
  return (
    <section className={under ? "card under" : "card"}>
      <div>
        <h2>{title}</h2>
        {hint && <p className="hint">{hint}</p>}
      </div>
      {children}
    </section>
  );
}

function Switch({
  label,
  on: set,
  disabled = false,
  onChange,
}: {
  label: string;
  on: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  // What is shown follows the click at once, and what the application says after: two quick clicks
  // are on and off again, not twice "on" computed from a value that had not come back yet.
  const [on, setOn] = useState(set);
  useEffect(() => setOn(set), [set]);
  const latest = useRef(on);
  latest.current = on;
  // (And should the application not have done as asked (the system would not have it), what it
  // says stays what it was, and nothing tells the switch: a moment after the last click, it shows
  // what the application says, whatever that is.)
  const said = useRef(set);
  said.current = set;
  const settling = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(settling.current), []);
  const toggle = () => {
    const next = !latest.current;
    latest.current = next;
    setOn(next);
    onChange(next);
    window.clearTimeout(settling.current);
    settling.current = window.setTimeout(() => setOn(said.current), 1500);
  };

  return (
    <button
      className="switch"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={toggle}
    >
      <span />
    </button>
  );
}
