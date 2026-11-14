import { createEffect, createSignal, onCleanup, Show, type Accessor, type JSX } from "solid-js";
import { api, fileUrl } from "../api";

type PreviewFile = { id: string; local_path: string | null; preview_path?: string | null };

// Core has already selected SVG image context. Prefer its cached PNG, otherwise
// request bounded cached bytes only when the reserved tile approaches the view.
export default function SvgPreview(props: {
  file: PreviewFile;
  fallback: JSX.Element;
  children: (src: Accessor<string | undefined>, onError: () => void) => JSX.Element;
}) {
  const [src, setSrc] = createSignal<string>();
  const [failed, setFailed] = createSignal(false);
  let tile!: HTMLSpanElement;
  let imageError = () => {};
  createEffect(() => {
    const file = props.file;
    let closed = false;
    let attempted = false;
    let url: string | undefined;
    setSrc(undefined);
    setFailed(false);
    const fail = () => {
      if (closed) return;
      if (url) URL.revokeObjectURL(url);
      url = undefined;
      setFailed(true);
    };
    const loadSvg = () => {
      if (attempted || !file.local_path) {
        fail();
        return;
      }
      attempted = true;
      api.documentData(file.id, true).then((bytes) => {
        if (closed) return;
        url = URL.createObjectURL(new Blob([bytes], { type: "image/svg+xml" }));
        setSrc(url);
      }).catch(fail);
    };
    imageError = loadSvg;
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      if (file.preview_path) setSrc(fileUrl(file.id, true));
      else loadSvg();
    }, { rootMargin: "400px 0px" });
    observer.observe(tile);
    onCleanup(() => {
      closed = true;
      observer.disconnect();
      if (url) URL.revokeObjectURL(url);
    });
  });
  return (
    <span ref={tile} class="svg-preview">
      <Show when={!failed()} fallback={props.fallback}>
        {props.children(src, () => imageError())}
      </Show>
    </span>
  );
}
