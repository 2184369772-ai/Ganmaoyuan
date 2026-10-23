import type { HTMLAttributes } from "react";

type BadgeTone = "success" | "warning" | "error" | "pending" | "high" | "medium" | "low" | "neutral";

type BadgeProps = HTMLAttributes<HTMLSpanElement> & {
  tone?: BadgeTone;
};

export function Badge({ tone = "neutral", className = "", children, ...props }: BadgeProps) {
  return (
    <span className={["ui-badge", `ui-badge-${tone}`, className].filter(Boolean).join(" ")} {...props}>
      {children}
    </span>
  );
}

export function confidenceTone(level?: string): BadgeTone {
  if (level === "high") return "high";
  if (level === "medium") return "medium";
  if (level === "low") return "low";
  return "neutral";
}
