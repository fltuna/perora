// A part of a monitor, as shares of its width and height: set as the editor's crop is, or only shown.
// The part that brings the selection screen's mode bar back is one (its own screen sets it, the
// settings show it).
import { useLayoutEffect, useRef, useState } from "react";

/** A part of a monitor, as shares of its width and height (0 to 1) from its top left corner. */
export interface Zone {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Where the pointer brings the selection screen's mode bar back at first (as the application has
    it): a thin strip right at the top, under the bar. */
export const BAR_REVEAL: Zone = { x: 0.3, y: 0, width: 0.4, height: 0.01 };

/** A place on the monitor, as shares of its width and height. */
type Spot = { x: number; y: number };

/** What a press on the monitor does to the part: pull an edge or a corner of it (which way on
    each side: -1 the left or top one, 1 the right or bottom one), move it, or draw a new one. */
type Grip = { x: -1 | 0 | 1; y: -1 | 0 | 1 } | "move" | "new";

/** The pointer that says what a press would do: the arrows of an edge or a corner, the four arrows
    of moving, the cross of drawing anew. */
function gripCursor(grip: Grip) {
  if (grip === "move") return "move";
  if (grip === "new") return "crosshair";
  if (grip.x === 0) return "ns-resize";
  if (grip.y === 0) return "ew-resize";
  return grip.x === grip.y ? "nwse-resize" : "nesw-resize";
}

/** A part of a monitor set the way the editor's crop is: drag outside it to draw a new one, pull its
    edges and corners, drag it by its inside to move it (owner, 2026-11-02: 「切り抜きツールみたいな
    感じに調整もできるようにしておきたい」). From the keyboard too. What is inside (`children`) is
    drawn under the part; the element itself is the monitor, which the part's shares are of. */
export function ZoneEditor({
  label,
  value,
  onChange,
  near = 6,
  className,
  style,
  children,
}: {
  label: string;
  value: Zone;
  onChange: (zone: Zone) => void;
  /** How near an edge of the part, in pixels of the page, a press takes hold of that edge. */
  near?: number;
  className?: string;
  style?: React.CSSProperties;
  children?: React.ReactNode;
}) {
  const GRIP_NEAR = near;
  const box = useRef<HTMLDivElement>(null);
  const [gesture, setGesture] = useState<{ grip: Grip; from: Spot; was: Zone } | null>(null);
  const [draft, setDraft] = useState<Zone | null>(null);
  const [hover, setHover] = useState<Grip>("new");
  // Shown at once, before the application says it back.
  const [shown, setShown] = useState(value);
  // (Before it is drawn: a part set from outside, as by a button, shows at once.)
  useLayoutEffect(() => setShown(value), [value.x, value.y, value.width, value.height]);

  // The inside of the monitor, inside its border: where the part is drawn, and so measured.
  const room = () => {
    const monitor = box.current!;
    const { left, top } = monitor.getBoundingClientRect();
    return { left: left + monitor.clientLeft, top: top + monitor.clientTop, width: monitor.clientWidth, height: monitor.clientHeight };
  };
  const clamp = (n: number) => Math.min(1, Math.max(0, n));
  const share = (event: React.PointerEvent): Spot => {
    const { left, top, width, height } = room();
    return { x: clamp((event.clientX - left) / width), y: clamp((event.clientY - top) / height) };
  };
  const gripAt = (event: React.PointerEvent): Grip => {
    const { left, top, width, height } = room();
    const [px, py] = [event.clientX - left, event.clientY - top];
    const [x0, y0] = [shown.x * width, shown.y * height];
    const [x1, y1] = [x0 + shown.width * width, y0 + shown.height * height];
    const across = px >= x0 - GRIP_NEAR && px <= x1 + GRIP_NEAR;
    const along = py >= y0 - GRIP_NEAR && py <= y1 + GRIP_NEAR;
    // Which edge a press takes, along one side. Of a part too thin for holds inside it (the strip at
    // the top is a few pixels), only from outside: a press inside moves it.
    const edge = (p: number, a: number, b: number): -1 | 0 | 1 => {
      if (b - a < GRIP_NEAR * 3) return p <= a && a - p <= GRIP_NEAR ? -1 : p >= b && p - b <= GRIP_NEAR ? 1 : 0;
      return Math.abs(p - a) <= GRIP_NEAR ? -1 : Math.abs(p - b) <= GRIP_NEAR ? 1 : 0;
    };
    const x = along ? edge(px, x0, x1) : 0;
    const y = across ? edge(py, y0, y1) : 0;
    if (x !== 0 || y !== 0) return { x, y };
    return px >= x0 && px <= x1 && py >= y0 && py <= y1 ? "move" : "new";
  };
  const between = (a: Spot, b: Spot): Zone => ({
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  });
  /** The part as the gesture leaves it with the pointer at `at`. */
  const pulled = (grip: Grip, was: Zone, from: Spot, at: Spot): Zone => {
    if (grip === "new") return between(from, at);
    if (grip === "move") {
      const x = Math.min(1 - was.width, Math.max(0, was.x + at.x - from.x));
      const y = Math.min(1 - was.height, Math.max(0, was.y + at.y - from.y));
      return { ...was, x, y };
    }
    // The edge held goes as far as the pointer does (not to where it is: a press on a hold beside
    // the edge moves nothing); the one across from it stays, and passing it turns the part.
    const [dx, dy] = [at.x - from.x, at.y - from.y];
    let [left, right, top, bottom] = [was.x, was.x + was.width, was.y, was.y + was.height];
    if (grip.x === -1) left = clamp(left + dx);
    if (grip.x === 1) right = clamp(right + dx);
    if (grip.y === -1) top = clamp(top + dy);
    if (grip.y === 1) bottom = clamp(bottom + dy);
    return between({ x: left, y: top }, { x: right, y: bottom });
  };
  const zone = draft ?? shown;
  const percent = (n: number) => `${n * 100}%`;
  const end = () => {
    setGesture(null);
    setDraft(null);
  };
  /** Sets what was made, when it is a part to reach and not what is set already. */
  const take = (made: Zone | null) => {
    const changed = made && (["x", "y", "width", "height"] as const).some((side) => Math.abs(made[side] - shown[side]) > 1e-6);
    if (made && changed && made.width > 0.005 && made.height > 0.002) {
      setShown(made);
      onChange(made);
    }
  };
  // From the keyboard too: the arrows move it, with Shift they make it larger or smaller (to the right
  // and down), a hundredth of the monitor a press.
  const onKey = (event: React.KeyboardEvent) => {
    const step = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!step) return;
    event.preventDefault();
    const [dx, dy] = [step[0] * 0.01, step[1] * 0.01];
    const was = shown;
    take(
      event.shiftKey
        ? {
            ...was,
            width: Math.min(1 - was.x, Math.max(0.01, was.width + dx)),
            height: Math.min(1 - was.y, Math.max(0.01, was.height + dy)),
          }
        : {
            ...was,
            x: Math.min(1 - was.width, Math.max(0, was.x + dx)),
            y: Math.min(1 - was.height, Math.max(0, was.y + dy)),
          },
    );
  };
  const round = (n: number) => Math.round(n * 100);

  return (
    <div
      ref={box}
      className={className ? `zone-editor ${className}` : "zone-editor"}
      role="group"
      tabIndex={0}
      aria-label={`${label}: ${round(zone.x)}%, ${round(zone.y)}%, ${round(zone.width)}% × ${round(zone.height)}%`}
      style={{ ...style, cursor: gripCursor(gesture?.grip ?? hover) }}
      onKeyDown={onKey}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        const grip = gripAt(event);
        setGesture({ grip, from: share(event), was: shown });
        // (A new one is drawn from the first move: a click leaves the one there is in sight.)
      }}
      onPointerMove={(event) => {
        if (!gesture) return setHover(gripAt(event));
        setDraft(pulled(gesture.grip, gesture.was, gesture.from, share(event)));
      }}
      onPointerUp={(event) => {
        if (event.button !== 0 || !gesture) return;
        // (Where it was let go, not the last move drawn. A click, a part too thin to reach, or one
        // that did not change, sets nothing.)
        take(draft && pulled(gesture.grip, gesture.was, gesture.from, share(event)));
        end();
      }}
      onPointerCancel={end}
    >
      {children}
      <span
        className={draft ? "zone drawing" : "zone"}
        style={{ left: percent(zone.x), top: percent(zone.y), width: percent(zone.width), height: percent(zone.height) }}
      >
        {/* The holds at its corners and the middles of its edges, as the crop's. */}
        {(["nw", "n", "ne", "e", "se", "s", "sw", "w"] as const).map((hold) => (
          <span key={hold} className={`hold ${hold}`} />
        ))}
      </span>
    </div>
  );
}

/** A part of a monitor, only shown (the settings' small monitor). */
export function ZoneView({ value, label, children }: { value: Zone; label: string; children?: React.ReactNode }) {
  const percent = (n: number) => `${n * 100}%`;
  const round = (n: number) => Math.round(n * 100);
  return (
    <div
      className="zone-view"
      role="img"
      aria-label={`${label}: ${round(value.x)}%, ${round(value.y)}%, ${round(value.width)}% × ${round(value.height)}%`}
    >
      {children}
      <span
        className="zone"
        style={{ left: percent(value.x), top: percent(value.y), width: percent(value.width), height: percent(value.height) }}
      />
    </div>
  );
}

