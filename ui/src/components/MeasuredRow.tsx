import { onCleanup, onMount, type JSX } from "solid-js";

export default function MeasuredRow(props: {
  id: string;
  observer: ResizeObserver;
  children: JSX.Element;
}) {
  let element!: HTMLDivElement;
  onMount(() => props.observer.observe(element));
  onCleanup(() => props.observer.unobserve(element));
  return (
    <div ref={element} class="virtual-row" data-virtual-key={props.id}>
      {props.children}
    </div>
  );
}
