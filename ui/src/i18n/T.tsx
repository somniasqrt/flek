import type { JSX } from "solid-js";
import { t, type Key, type Params } from "./index";
import { renderRichMessage } from "./rich-message";

// Renderers are supplied by the component, never selected from arbitrary HTML.
// Parameters are strings/numbers and stay text nodes, including tag-like content.
export default function T(props: {
  k: Key;
  params?: Params;
  tags?: Record<string, (children: JSX.Element) => JSX.Element>;
}) {
  return <>{renderRichMessage<JSX.Element>(t(props.k), props.params, props.tags)}</>;
}
