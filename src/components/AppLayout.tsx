import { type ReactNode } from "react";

type AppLayoutProps = {
  className?: string;
  sidebar?: ReactNode;
  sidebarOpen?: boolean;
  sidebarBackdrop?: ReactNode;
  header?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
};

export function AppLayout({
  className = "",
  sidebar,
  sidebarOpen = false,
  sidebarBackdrop,
  header,
  children,
  footer,
}: AppLayoutProps) {
  const classes = ["app-layout", sidebarOpen ? "app-layout-sidebar-open" : "", className]
    .filter(Boolean)
    .join(" ");

  return (
    <main className={classes}>
      {sidebar}
      {sidebarBackdrop}
      {header}
      <section className="app-layout-main">{children}</section>
      {footer}
    </main>
  );
}
