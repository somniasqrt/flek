import { createSignal, onCleanup, onMount } from "solid-js";
import { t, plural } from "./i18n";
import { clamp } from "./panel-sizes";

export default function ResizeHandle(props: {
  side: "left" | "right";
  label: string;
  controls: string;
  value: number;
  min: number;
  max: number;
  initial: number;
  onResize: (width: number) => void;
  onCommit: () => void;
}) {
  const [dragging, setDragging] = createSignal(false);
  let handle!: HTMLDivElement;
  let drag: { pointer: number; x: number; width: number } | null = null;
  const direction = () => (props.side === "left" ? 1 : -1);
  const resize = (width: number) => props.onResize(clamp(width, props.min, props.max));
  const finish = (cancel = false) => {
    if (!drag) return;
    const { pointer, width } = drag;
    drag = null;
    setDragging(false);
    document.body.classList.remove("resizing-panels");
    if (cancel) props.onResize(width);
    else props.onCommit();
    if (handle.hasPointerCapture(pointer)) handle.releasePointerCapture(pointer);
  };
  onMount(() => {
    const blur = () => finish();
    window.addEventListener("blur", blur);
    onCleanup(() => {
      finish();
      window.removeEventListener("blur", blur);
    });
  });
  return (
    <div
      ref={handle}
      class={`resize-handle resize-handle-${props.side}`}
      classList={{ dragging: dragging() }}
      role="separator"
      aria-label={props.label}
      aria-orientation="vertical"
      aria-controls={props.controls}
      aria-valuemin={props.min}
      aria-valuemax={Math.round(props.max)}
      aria-valuenow={Math.round(props.value)}
      aria-valuetext={plural("pixels", Math.round(props.value))}
      tabIndex={0}
      title={t("{name} · drag or use arrow keys · double-click to reset", { name: props.label })}
      onPointerDown={(e) => {
        if (e.button !== 0 || !e.isPrimary) return;
        e.preventDefault();
        handle.focus({ preventScroll: true });
        drag = { pointer: e.pointerId, x: e.clientX, width: props.value };
        handle.setPointerCapture(e.pointerId);
        setDragging(true);
        document.body.classList.add("resizing-panels");
      }}
      onPointerMove={(e) => {
        if (drag?.pointer === e.pointerId) resize(drag.width + (e.clientX - drag.x) * direction());
      }}
      onPointerUp={(e) => {
        if (drag?.pointer === e.pointerId) finish();
      }}
      onPointerCancel={(e) => {
        if (drag?.pointer === e.pointerId) finish(true);
      }}
      onLostPointerCapture={() => finish()}
      onDblClick={() => {
        resize(props.initial);
        props.onCommit();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape" && drag) {
          e.preventDefault();
          e.stopPropagation();
          finish(true);
          return;
        }
        if (drag || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Home") resize(props.min);
        else if (e.key === "End") resize(props.max);
        else
          resize(
            props.value + (e.key === "ArrowRight" ? 1 : -1) * direction() * (e.shiftKey ? 40 : 10),
          );
        props.onCommit();
      }}
    />
  );
}
