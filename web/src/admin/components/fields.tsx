// Form fields of the admin design system: a labelled wrapper with hint and error, text, number, select, checkbox,
// textarea, and a chip group (single choice) used for reason presets and duration chips.
import {
  cloneElement,
  useId,
  type InputHTMLAttributes,
  type ReactElement,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';

export interface FieldProps {
  readonly label: ReactNode;
  readonly hint?: ReactNode;
  readonly error?: ReactNode;
  readonly required?: boolean;
  /** The control: one element that takes `id` and `aria-describedby` (the field wires both). */
  readonly children: ReactElement<{
    id?: string;
    'aria-describedby'?: string;
    'aria-invalid'?: boolean;
  }>;
}

/** A label above a control, with an optional hint and error text wired through `aria-describedby`. */
export function Field({ label, hint, error, required, children }: FieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errId = `${id}-err`;
  const described = [hint ? hintId : '', error ? errId : ''].filter(Boolean).join(' ');
  return (
    <div className="adm-field">
      <label className="adm-field__label" htmlFor={id}>
        {label}
        {required === true && (
          <span className="adm-field__req" aria-hidden="true">
            {' '}
            *
          </span>
        )}
      </label>
      {cloneElement(children, {
        id,
        ...(described ? { 'aria-describedby': described } : {}),
        ...(error ? { 'aria-invalid': true } : {}),
      })}
      {hint ? (
        <div id={hintId} className="adm-field__hint">
          {hint}
        </div>
      ) : null}
      {error ? (
        <div id={errId} className="adm-field__error" role="alert">
          {error}
        </div>
      ) : null}
    </div>
  );
}

export function TextInput({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={`adm-input ${className ?? ''}`} {...rest} />;
}

export function NumberInput({
  className,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'>) {
  return (
    <input
      className={`adm-input adm-input--number ${className ?? ''}`}
      type="number"
      inputMode="numeric"
      {...rest}
    />
  );
}

export function TextArea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={`adm-input adm-textarea ${className ?? ''}`} {...rest} />;
}

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={`adm-input adm-select ${className ?? ''}`} {...rest}>
      {children}
    </select>
  );
}

export interface CheckboxProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> {
  readonly label: ReactNode;
}

export function Checkbox({ label, className, ...rest }: CheckboxProps) {
  return (
    <label className={`adm-check ${className ?? ''}`}>
      <input type="checkbox" {...rest} />
      <span>{label}</span>
    </label>
  );
}

export interface Chip<V extends string> {
  readonly value: V;
  readonly label: string;
}

export interface ChipGroupProps<V extends string> {
  readonly label: string;
  readonly chips: readonly Chip<V>[];
  readonly value: V | null;
  readonly onChange: (value: V | null) => void;
}

/** Single-choice chips (reason presets, duration chips). Clicking the chosen chip again clears the choice. */
export function ChipGroup<V extends string>({ label, chips, value, onChange }: ChipGroupProps<V>) {
  return (
    <div className="adm-chips" role="group" aria-label={label}>
      {chips.map((c) => {
        const on = c.value === value;
        return (
          <button
            key={c.value}
            type="button"
            className={`adm-chip ${on ? 'is-on' : ''}`}
            aria-pressed={on}
            onClick={() => {
              onChange(on ? null : c.value);
            }}
          >
            {c.label}
          </button>
        );
      })}
    </div>
  );
}
