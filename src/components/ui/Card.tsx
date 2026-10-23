import type { HTMLAttributes, ReactNode } from "react";

type CardVariant = "default" | "info" | "status" | "ai";

type CardProps = HTMLAttributes<HTMLElement> & {
  as?: "article" | "section" | "div";
  variant?: CardVariant;
  title?: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
};

export function Card({
  as: Component = "section",
  variant = "default",
  title,
  meta,
  actions,
  className = "",
  children,
  ...props
}: CardProps) {
  const classes = ["ui-card", `ui-card-${variant}`, className].filter(Boolean).join(" ");
  return (
    <Component className={classes} {...props}>
      {title || meta || actions ? (
        <div className="ui-card-head">
          <div>
            {meta ? <span className="ui-card-meta">{meta}</span> : null}
            {title ? <strong>{title}</strong> : null}
          </div>
          {actions ? <div className="ui-card-actions">{actions}</div> : null}
        </div>
      ) : null}
      {children}
    </Component>
  );
}
