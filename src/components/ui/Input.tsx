import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

type TextInputProps = InputHTMLAttributes<HTMLInputElement> & {
  label?: string;
  hint?: string;
  error?: string;
  variant?: "text" | "search" | "form" | "file";
};

type TextAreaProps = TextareaHTMLAttributes<HTMLTextAreaElement> & {
  label?: string;
  hint?: string;
  error?: string;
};

export function TextInput({ label, hint, error, variant = "text", className = "", id, ...props }: TextInputProps) {
  const inputId = id ?? props.name ?? label;
  return (
    <label className={`ui-field ui-field-${variant} ${className}`.trim()} htmlFor={inputId}>
      {label ? <span className="ui-field-label">{label}</span> : null}
      <input id={inputId} className="ui-input" aria-invalid={Boolean(error)} {...props} />
      {error ? <span className="ui-field-error">{error}</span> : hint ? <span className="ui-field-hint">{hint}</span> : null}
    </label>
  );
}

export function TextArea({ label, hint, error, className = "", id, ...props }: TextAreaProps) {
  const inputId = id ?? props.name ?? label;
  return (
    <label className={`ui-field ${className}`.trim()} htmlFor={inputId}>
      {label ? <span className="ui-field-label">{label}</span> : null}
      <textarea id={inputId} className="ui-input ui-textarea" aria-invalid={Boolean(error)} {...props} />
      {error ? <span className="ui-field-error">{error}</span> : hint ? <span className="ui-field-hint">{hint}</span> : null}
    </label>
  );
}
