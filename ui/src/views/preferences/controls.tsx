import { For, Show, type JSX } from "solid-js";
import { t } from "../../i18n/index";
import { Minus, Plus } from "../../components/icons";

// Building blocks shared by the Preferences sections.

export function Card(props: { title?: string; danger?: boolean; children: JSX.Element }) {
  return (
    <section class="pref-card-group">
      <Show when={props.title}>
        <h4 class="pref-card-title">{props.title}</h4>
      </Show>
      <div class="pref-card" classList={{ danger: props.danger }}>
        {props.children}
      </div>
    </section>
  );
}

export function Row(props: {
  label: string;
  description?: string;
  stacked?: boolean;
  children?: JSX.Element;
}) {
  return (
    <div class="pref-row" classList={{ stacked: props.stacked }}>
      <div class="pref-row-text">
        <div class="pref-row-label">{props.label}</div>
        <Show when={props.description}>
          <div class="pref-row-desc">{props.description}</div>
        </Show>
      </div>
      <Show when={props.children}>
        <div class="pref-row-control">{props.children}</div>
      </Show>
    </div>
  );
}

export function Switch(props: {
  checked: boolean;
  label: string;
  disabled?: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      class="pref-switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
    >
      <span class="pref-switch-thumb" />
    </button>
  );
}

export type Options<V extends string> = readonly (readonly [V, string])[];

export function Segmented<V extends string>(props: {
  label: string;
  value: V;
  options: Options<V>;
  disabled?: boolean;
  onChange: (value: V) => void;
}) {
  return (
    <div class="pref-segmented" role="radiogroup" aria-label={props.label}>
      <For each={props.options}>
        {([value, text]) => (
          <button
            type="button"
            role="radio"
            aria-checked={props.value === value}
            disabled={props.disabled}
            onClick={() => props.onChange(value)}
          >
            {text}
          </button>
        )}
      </For>
    </div>
  );
}

export function SelectBox<V extends string>(props: {
  label: string;
  value: V;
  options: Options<V>;
  disabled?: boolean;
  onChange: (value: V) => void;
}) {
  return (
    <select
      class="pref-select"
      aria-label={props.label}
      value={props.value}
      disabled={props.disabled}
      onChange={(e) => props.onChange(e.currentTarget.value as V)}
    >
      <For each={props.options}>{([value, text]) => <option value={value}>{text}</option>}</For>
    </select>
  );
}

export function Stepper(props: {
  label: string;
  value: number;
  min: number;
  max: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div class="pref-stepper" role="group" aria-label={props.label}>
      <button
        type="button"
        aria-label={t("Decrease")}
        disabled={props.disabled || props.value <= props.min}
        onClick={() => props.onChange(props.value - 1)}
      >
        <Minus size={16} />
      </button>
      <input
        type="number"
        aria-label={props.label}
        min={props.min}
        max={props.max}
        step="1"
        value={props.value}
        disabled={props.disabled}
        onChange={(e) => {
          const n = e.currentTarget.valueAsNumber;
          if (Number.isInteger(n) && n >= props.min && n <= props.max) props.onChange(n);
          else e.currentTarget.value = String(props.value);
        }}
      />
      <button
        type="button"
        aria-label={t("Increase")}
        disabled={props.disabled || props.value >= props.max}
        onClick={() => props.onChange(props.value + 1)}
      >
        <Plus size={16} />
      </button>
    </div>
  );
}

// Theme choices drawn as miniature windows so the result is visible before choosing.
export function ThemeCards(props: {
  label: string;
  value: "system" | "light" | "dark";
  options: Options<"system" | "light" | "dark">;
  disabled?: boolean;
  onChange: (value: "system" | "light" | "dark") => void;
}) {
  return (
    <div class="pref-themes" role="radiogroup" aria-label={props.label}>
      <For each={props.options}>
        {([value, text]) => (
          <button
            type="button"
            role="radio"
            class="pref-theme"
            aria-checked={props.value === value}
            disabled={props.disabled}
            onClick={() => props.onChange(value)}
          >
            <span class={`pref-swatch ${value}`} aria-hidden="true">
              <span class="pref-swatch-side" />
              <span class="pref-swatch-main">
                <i />
                <i />
                <i />
              </span>
            </span>
            <span class="pref-theme-name">{text}</span>
          </button>
        )}
      </For>
    </div>
  );
}
