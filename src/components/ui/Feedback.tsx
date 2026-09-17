import type { ReactNode } from "react";

type FeedbackTone = "info" | "success" | "warning" | "error" | "loading";

export function Feedback({
  tone = "info",
  title,
  children,
}: {
  tone?: FeedbackTone;
  title?: string;
  children?: ReactNode;
}) {
  return (
    <div className={`ui-feedback ui-feedback-${tone}`} role={tone === "error" ? "alert" : "status"}>
      {tone === "loading" ? <span className="ui-spinner" aria-hidden="true" /> : null}
      <div>
        {title ? <strong>{title}</strong> : null}
        {children ? <p>{children}</p> : null}
      </div>
    </div>
  );
}
